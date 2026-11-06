[CmdletBinding()]
param(
    [ValidateSet("x64", "arm64")]
    [string]$Architecture = "x64",
    [ValidateSet("Prepare", "Build", "All")]
    [string]$Phase = "All",
    [string]$BuildDirectory,
    [string]$WindowsSdkRoot,
    [string]$VisualStudioDirectory,
    [string]$Python = "python",
    [ValidateRange(1, 4)]
    [int]$Jobs = 2,
    [switch]$RunTests
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
if ($env:OS -ne "Windows_NT" -or $PSVersionTable.PSVersion.Major -lt 7) {
    throw "This engine build requires Windows and PowerShell 7."
}
if ($RunTests -and $Phase -ne "Prepare" -and
    ($Architecture -ne "x64" -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() -ne "X64")) {
    throw "This full engine test configuration requires a native x64 Windows host; cross compilation is not execution."
}
$PSNativeCommandUseErrorActionPreference = $false
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$identityTool = Join-Path $PSScriptRoot "flutter_windows_engine.py"
$pythonCommand = (Get-Command $Python -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
if ($pythonCommand -like "*\Microsoft\WindowsApps\*") {
    throw "Pass -Python with an installed Python executable instead of the WindowsApps alias."
}
$recipeText = & $pythonCommand $identityTool recipe
if ($LASTEXITCODE -ne 0) { throw "Windows engine recipe validation failed." }
$recipeInfo = ($recipeText -join "`n") | ConvertFrom-Json
$recipe = $recipeInfo.recipe
if ([string]::IsNullOrWhiteSpace($BuildDirectory)) {
    $BuildDirectory = Join-Path $repositoryRoot "target\flutter-engine"
}
$buildRoot = [IO.Path]::GetFullPath($BuildDirectory)
$marker = Join-Path $buildRoot "usque-engine-build-root.json"
if (Test-Path -LiteralPath $buildRoot) {
    if (-not (Test-Path -LiteralPath $marker -PathType Leaf)) {
        throw "Existing build directory has no Usque ownership marker; choose a fresh directory."
    }
    $owner = Get-Content -LiteralPath $marker -Raw | ConvertFrom-Json
    if ($owner.Kind -ne "usque-flutter-engine-v1" -or $owner.RecipeSha256 -ne $recipeInfo.recipe_sha256) {
        throw "Build directory belongs to a different recipe; preserve it and choose a fresh directory."
    }
}
elseif ($Phase -eq "Build") {
    throw "Run the Prepare phase before building."
}
else {
    New-Item -ItemType Directory -Path $buildRoot | Out-Null
    [ordered]@{ Kind = "usque-flutter-engine-v1"; RecipeSha256 = $recipeInfo.recipe_sha256 } |
        ConvertTo-Json | Set-Content -LiteralPath $marker -Encoding utf8NoBOM
}
$checkout = Join-Path $buildRoot "flutter"
$depot = Join-Path $buildRoot "depot_tools"
$logs = Join-Path $buildRoot "logs"
foreach ($directory in @($logs, (Join-Path $buildRoot "temp"), (Join-Path $buildRoot "cipd-cache"), (Join-Path $buildRoot "vpython"))) {
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
}
$git = (Get-Command git.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$vswhereCommand = Get-Command vswhere.exe -ErrorAction SilentlyContinue | Select-Object -First 1
$vswhere = if ($null -ne $vswhereCommand) { $vswhereCommand.Source } else {
    Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
}
$visualStudio = (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace([string]$visualStudio)) {
    throw "Visual Studio C++ Build Tools are required."
}
$visualStudio = ([string]$visualStudio).Trim()
$visualStudioVersion = (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationVersion)
if ($LASTEXITCODE -ne 0) { throw "Could not identify the Visual Studio version." }
$visualStudioYear = switch (([Version]([string]$visualStudioVersion).Trim()).Major) {
    17 { "2022" }
    18 { "2026" }
    default { throw "The engine recipe supports Visual Studio 2022 and 2026." }
}
$versionFile = "VC\Auxiliary\Build\Microsoft.VCToolsVersion.default.txt"
$msvcVersion = (Get-Content -LiteralPath (Join-Path $visualStudio $versionFile) -Raw).Trim()
if (-not [string]::IsNullOrWhiteSpace($VisualStudioDirectory)) {
    $overlay = (Resolve-Path -LiteralPath $VisualStudioDirectory).Path
    if ((Get-Content -LiteralPath (Join-Path $overlay $versionFile) -Raw).Trim() -ne $msvcVersion) {
        throw "Visual Studio override must match the selected installation's MSVC toolset."
    }
    $visualStudio = $overlay
}
$atlHeader = Join-Path $visualStudio "VC\Tools\MSVC\$msvcVersion\atlmfc\include\atlbase.h"
$atlLibrary = Join-Path $visualStudio "VC\Tools\MSVC\$msvcVersion\atlmfc\lib\$Architecture\atls.lib"
if (-not (Test-Path -LiteralPath $atlHeader -PathType Leaf) -or -not (Test-Path -LiteralPath $atlLibrary -PathType Leaf)) {
    throw "The selected C++ toolset lacks ATL headers or $Architecture libraries. Install its matching ATL component or pass a prepared VisualStudioDirectory overlay."
}
foreach ($library in @("delayimp.lib", "msvcrt.lib", "libcmt.lib")) {
    $libraryPath = Join-Path $visualStudio "VC\Tools\MSVC\$msvcVersion\lib\$Architecture\$library"
    if (-not (Test-Path -LiteralPath $libraryPath -PathType Leaf)) {
        throw "The selected C++ toolset lacks $Architecture library $library. Install its matching C++ libraries or pass a prepared VisualStudioDirectory overlay."
    }
}
if ([string]::IsNullOrWhiteSpace($WindowsSdkRoot)) {
    $WindowsSdkRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10"
}
$sdk = (Resolve-Path -LiteralPath $WindowsSdkRoot).Path
foreach ($relative in @("Include\$($recipe.windows_sdk_version)\um\Windows.h", "Lib\$($recipe.windows_sdk_version)\um\$Architecture\kernel32.lib", "Debuggers\x64\dbghelp.dll", "Debuggers\$Architecture\dbghelp.dll")) {
    if (-not (Test-Path -LiteralPath (Join-Path $sdk $relative) -PathType Leaf)) {
        throw "Windows SDK prerequisite missing: $relative. Use a complete SDK or a prepared SDK overlay."
    }
}
if ((Get-ItemProperty -LiteralPath HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem -Name LongPathsEnabled).LongPathsEnabled -ne 1) {
    throw "Windows long-path support is required by the official engine build."
}

function Invoke-EngineStep {
    param([string]$Name, [string]$Command, [string[]]$Arguments, [string]$Directory)
    $log = Join-Path $logs ("$Name-" + [Guid]::NewGuid().ToString("N") + ".log")
    Write-Information "Engine step $Name; log: $log" -InformationAction Continue
    Push-Location $Directory
    try {
        & $Command @Arguments 2>&1 | Tee-Object -FilePath $log | Out-Host
        $code = $LASTEXITCODE
        [ordered]@{ Step = $Name; ExitCode = $code } | ConvertTo-Json |
            Set-Content -LiteralPath ($log + ".json") -Encoding utf8NoBOM
        if ($code -ne 0) { throw "Engine step $Name failed ($code). See $log" }
    }
    finally { Pop-Location }
}

function Initialize-PinnedCheckout {
    param([string]$Path, [string]$Url, [string]$Revision, [string]$Name)
    if (-not (Test-Path -LiteralPath (Join-Path $Path ".git") -PathType Container)) {
        Invoke-EngineStep "$Name-init" $git @("init", "--template=", $Path) $buildRoot
        Invoke-EngineStep "$Name-remote" $git @("-C", $Path, "remote", "add", "origin", $Url) $buildRoot
    }
    $head = & $git -C $Path rev-parse --verify HEAD 2>$null
    if ($LASTEXITCODE -ne 0) {
        Invoke-EngineStep "$Name-fetch" $git @("-C", $Path, "fetch", "--depth", "1", "origin", $Revision) $buildRoot
        Invoke-EngineStep "$Name-checkout" $git @("-C", $Path, "checkout", "--detach", $Revision) $buildRoot
        $head = & $git -C $Path rev-parse HEAD
    }
    if ($LASTEXITCODE -ne 0 -or ([string]$head).Trim() -ne $Revision) {
        throw "$Name checkout has a different source revision."
    }
}

$environmentNames = @("PATH", "Path", "TEMP", "TMP", "CL", "_CL_", "DEPOT_TOOLS_UPDATE", "DEPOT_TOOLS_WIN_TOOLCHAIN", "DEPOT_TOOLS_METRICS", "GYP_MSVS_OVERRIDE_PATH", "GYP_MSVS_VERSION", "WINDOWSSDKDIR", "VPYTHON_VIRTUALENV_ROOT", "CIPD_CACHE_DIR", "PUB_CACHE", "GIT_CONFIG_COUNT", "GIT_CONFIG_KEY_0", "GIT_CONFIG_VALUE_0", "GIT_CONFIG_KEY_1", "GIT_CONFIG_VALUE_1", "GIT_CONFIG_KEY_2", "GIT_CONFIG_VALUE_2", "GIT_CONFIG_NOSYSTEM", "GIT_CONFIG_GLOBAL")
$environmentNames += @(Get-ChildItem Env: | Where-Object { $_.Name -like "GIT_*" } | ForEach-Object { $_.Name })
$savedEnvironment = @{}
foreach ($name in ($environmentNames | Select-Object -Unique)) {
    $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
}
try {
    Get-ChildItem Env: | Where-Object { $_.Name -like "GIT_*" } | ForEach-Object {
        Remove-Item -LiteralPath ("Env:" + $_.Name)
    }
    $env:GIT_CONFIG_NOSYSTEM = "1"
    $isolatedGitConfig = Join-Path $buildRoot "git-config"
    [IO.File]::WriteAllText($isolatedGitConfig, "[depot-tools]`n`tallowGlobalGitConfig = false`n", [Text.UTF8Encoding]::new($false))
    $env:GIT_CONFIG_GLOBAL = $isolatedGitConfig
    $env:GIT_CONFIG_COUNT = "3"
    $env:GIT_CONFIG_KEY_0 = "core.longpaths"
    $env:GIT_CONFIG_VALUE_0 = "true"
    $env:GIT_CONFIG_KEY_1 = "core.autocrlf"
    $env:GIT_CONFIG_VALUE_1 = "false"
    $env:GIT_CONFIG_KEY_2 = "core.eol"
    $env:GIT_CONFIG_VALUE_2 = "lf"
    $taskPath = $depot + ";" + (Split-Path -Parent $pythonCommand) + ";" + $env:PATH
    Remove-Item -LiteralPath Env:PATH -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath Env:Path -ErrorAction SilentlyContinue
    $env:PATH = $taskPath
    $env:TEMP = Join-Path $buildRoot "temp"
    $env:TMP = $env:TEMP
    $env:DEPOT_TOOLS_UPDATE = "0"
    $env:DEPOT_TOOLS_WIN_TOOLCHAIN = "0"
    $env:DEPOT_TOOLS_METRICS = "0"
    $env:GYP_MSVS_OVERRIDE_PATH = $visualStudio
    $env:GYP_MSVS_VERSION = $visualStudioYear
    $env:WINDOWSSDKDIR = $sdk
    $env:VPYTHON_VIRTUALENV_ROOT = Join-Path $buildRoot "vpython"
    $env:CIPD_CACHE_DIR = Join-Path $buildRoot "cipd-cache"
    $env:PUB_CACHE = Join-Path $buildRoot "pub-cache"
    Remove-Item -LiteralPath Env:CL -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath Env:_CL_ -ErrorAction SilentlyContinue

    if ($Phase -ne "Build") {
        Initialize-PinnedCheckout $checkout $recipe.engine_repository $recipe.engine_commit "source"
        Initialize-PinnedCheckout $depot $recipe.depot_tools_repository $recipe.depot_tools_commit "depot"
        # Keep the depot checkout fixed while explicitly generating the official
        # Windows git.bat/Python wrappers that GN expects.
        Invoke-EngineStep "depot-bootstrap" (Join-Path $depot "bootstrap\win_tools.bat") @() $depot
        # The gclient configuration is created only within this owned checkout.
        $customVariables = ($recipe.gclient_custom_vars | ConvertTo-Json -Compress).Replace(":false", ":False").Replace(":true", ":True")
        $configuration = "solutions = [{`"name`": `".`", `"url`": `"$($recipe.engine_repository)`", `"managed`": False, `"deps_file`": `"DEPS`", `"custom_deps`": {}, `"custom_vars`": $customVariables}]`n"
        [IO.File]::WriteAllText((Join-Path $checkout ".gclient"), $configuration, [Text.UTF8Encoding]::new($false))
        Invoke-EngineStep "sync" (Join-Path $depot "gclient.bat") @("sync", "--no-history", "--nohooks", "--jobs=4") $checkout
        foreach ($item in $recipe.patches) {
            $patchPath = Join-Path $repositoryRoot $item.path
            & $git -C $checkout apply --reverse --check $patchPath 2>$null
            if ($LASTEXITCODE -ne 0) {
                Invoke-EngineStep "patch-check-$($item.kind)" $git @("-C", $checkout, "apply", "--check", $patchPath) $buildRoot
                Invoke-EngineStep "patch-$($item.kind)" $git @("-C", $checkout, "apply", $patchPath) $buildRoot
            }
        }
        Invoke-EngineStep "source-identity" $pythonCommand @($identityTool, "verify-source", $checkout) $buildRoot
        Invoke-EngineStep "hooks" (Join-Path $depot "gclient.bat") @("runhooks") $checkout
        [ordered]@{
            RecipeSha256 = $recipeInfo.recipe_sha256
            GclientSha256 = (Get-FileHash -LiteralPath (Join-Path $checkout ".gclient") -Algorithm SHA256).Hash
            DepotToolsCommit = $recipe.depot_tools_commit
            HooksExitCode = 0
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $buildRoot "prepared-state.json") -Encoding utf8NoBOM
    }
    if ($Phase -eq "Prepare") { return }
    $prepared = Join-Path $buildRoot "prepared-state.json"
    if (-not (Test-Path -LiteralPath $prepared -PathType Leaf)) {
        throw "A successful Prepare phase for this recipe is required."
    }
    $preparedState = Get-Content -LiteralPath $prepared -Raw | ConvertFrom-Json
    if (-not (Test-Path -LiteralPath (Join-Path $depot "git.bat") -PathType Leaf)) {
        throw "The fixed depot_tools Windows bootstrap is incomplete; rerun Prepare."
    }
    $depotHead = (& $git -C $depot rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $preparedState.RecipeSha256 -ne $recipeInfo.recipe_sha256 -or
        $preparedState.GclientSha256 -ne (Get-FileHash -LiteralPath (Join-Path $checkout ".gclient") -Algorithm SHA256).Hash -or
        $preparedState.DepotToolsCommit -ne $depotHead -or $depotHead -ne $recipe.depot_tools_commit -or $preparedState.HooksExitCode -ne 0) {
        throw "Prepared dependency inputs differ from this recipe."
    }
    Invoke-EngineStep "source-identity" $pythonCommand @($identityTool, "verify-source", $checkout) $buildRoot
    $engineSource = Join-Path $checkout "engine\src"
    $ninja = Join-Path $checkout "third_party\ninja\ninja.exe"
    $configurationName = $recipe.architectures.$Architecture
    $gnArguments = @("-B", "flutter/tools/gn") + @($recipe.release_gn_args)
    if ($Architecture -eq "arm64") { $gnArguments += @("--windows-cpu", "arm64") }
    Invoke-EngineStep "gn-$Architecture" $pythonCommand $gnArguments $engineSource
    $ninjaArguments = @("-C", "out/$configurationName", "-j$Jobs") + @($recipe.release_ninja_targets)
    Invoke-EngineStep "compile-$Architecture" $ninja $ninjaArguments $engineSource
    if ($RunTests) {
        Invoke-EngineStep "gn-tests" $pythonCommand (@("-B", "flutter/tools/gn") + @($recipe.test_gn_args)) $engineSource
        Invoke-EngineStep "compile-tests" $ninja (@("-C", "out/host_debug_test", "-j$Jobs") + @($recipe.test_ninja_targets)) $engineSource
        Invoke-EngineStep "engine-tests" $pythonCommand (@("-B", "flutter/testing/run_tests.py") + @($recipe.engine_test_args)) $engineSource
    }
    $buildManifest = Join-Path $buildRoot "engine-build-$Architecture.json"
    $recordArguments = @($identityTool, "record-build", $checkout, "--architecture", $Architecture, "--output", $buildManifest, "--expected-recipe-sha256", $recipeInfo.recipe_sha256)
    if ($RunTests) { $recordArguments += "--tests-passed" }
    Invoke-EngineStep "build-identity" $pythonCommand $recordArguments $buildRoot
    Write-Output "EngineSource=$engineSource"
    Write-Output "LocalEngine=$configurationName"
    Write-Output "LocalEngineHost=$configurationName"
    Write-Output "BuildManifest=$buildManifest"
}
finally {
    foreach ($name in $savedEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name], "Process")
    }
}
