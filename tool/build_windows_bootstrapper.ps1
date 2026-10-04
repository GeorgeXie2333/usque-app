[CmdletBinding()]
param(
    [ValidateSet("x64-v2", "arm64")]
    [string]$Variant = "x64-v2",
    [string]$OutputDirectory,
    [string]$SdkDirectory,
    [string]$PythonPath = "python",
    [switch]$Test,
    [switch]$Preview
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$sourceRoot = Join-Path $repositoryRoot "packaging\windows\bootstrapper"
$runnerRoot = Join-Path $repositoryRoot "apps\usque_gui\windows\runner"
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $repositoryRoot "target\bootstrapper-$Variant"
}
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
if ([string]::IsNullOrWhiteSpace($SdkDirectory)) {
    $SdkDirectory = Join-Path $repositoryRoot "target\bootstrapper-sdk"
}
$sdkRoot = [IO.Path]::GetFullPath($SdkDirectory)
New-Item -ItemType Directory -Path $sdkRoot -Force | Out-Null

# The dependency manifest is shared with SBOM generation. Exact hashes lock
# both the API and native dependency; the upstream nuspec's wider dependency
# range must never select another SDK.
$dependencyPath = Join-Path $repositoryRoot "packaging\windows\setup\dependencies.json"
$packages = @(Get-Content -LiteralPath $dependencyPath -Raw | ConvertFrom-Json)
$expectedPackages = @{
    "WixToolset.BootstrapperApplicationApi" = "api"
    "WixToolset.DUtil" = "dutil"
}
if ($packages.Count -ne 2 -or @($packages.name | Select-Object -Unique).Count -ne 2) {
    throw "The bootstrapper requires exactly its two pinned native WiX dependencies."
}
foreach ($package in $packages) {
    if (-not $expectedPackages.ContainsKey($package.name) -or $package.version -cne "5.0.2" -or
        $package.license -cne "MS-RL" -or $package.folder -cne $expectedPackages[$package.name] -or
        $package.sha256 -cnotmatch '^[a-f0-9]{64}$') {
        throw "Invalid pinned bootstrapper dependency metadata."
    }
    $packageId = $package.name.ToLowerInvariant()
    $expectedUrl = "https://api.nuget.org/v3-flatcontainer/$packageId/5.0.2/$packageId.5.0.2.nupkg"
    if ($package.url -cne $expectedUrl) { throw "Unexpected bootstrapper dependency URL." }
    $archive = Join-Path $sdkRoot "$($package.folder).zip"
    if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) {
        Invoke-WebRequest -Uri $package.url -OutFile $archive
    }
    $actual = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
    if ($actual -ne $package.sha256) {
        throw "Pinned WiX 5.0.2 package hash mismatch: $($package.name)."
    }
    # Re-extraction overwrites only this verified package's members. There is no
    # recursive deletion or trust in previously extracted libraries/headers.
    Expand-Archive -LiteralPath $archive -DestinationPath (Join-Path $sdkRoot $package.folder) -Force
}

$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) { throw "vswhere.exe was not found." }
$component = if ($Variant -eq "arm64") {
    "Microsoft.VisualStudio.Component.VC.Tools.ARM64"
} else { "Microsoft.VisualStudio.Component.VC.Tools.x86.x64" }
$visualStudioMatch = & $vswhere -latest -products * -requires $component -property installationPath
$visualStudio = if ($null -eq $visualStudioMatch) { "" } else { ([string]$visualStudioMatch).Trim() }
if ([string]::IsNullOrWhiteSpace($visualStudio)) { throw "Visual Studio C++ tools for $Variant were not found." }
$hostArchitecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
$vcvarsName = if ($Variant -eq "arm64") {
    if ($hostArchitecture -eq "Arm64") { "vcvarsarm64.bat" } else { "vcvarsamd64_arm64.bat" }
} else { "vcvars64.bat" }
$vcvars = Join-Path $visualStudio "VC\Auxiliary\Build\$vcvarsName"
if (-not (Test-Path -LiteralPath $vcvars -PathType Leaf)) { throw "$vcvarsName was not found." }
$environmentLines = & $env:ComSpec /d /s /c "call `"$vcvars`" >nul && set"
if ($LASTEXITCODE -ne 0) { throw "$vcvarsName failed: $LASTEXITCODE." }
$developerPath = $null
foreach ($line in $environmentLines) {
    $separator = $line.IndexOf('=')
    if ($separator -le 0) { continue }
    $name = $line.Substring(0, $separator)
    $value = $line.Substring($separator + 1)
    if ($name -ieq "Path") {
        if ($value.IndexOf($visualStudio, [StringComparison]::OrdinalIgnoreCase) -ge 0) { $developerPath = $value }
    } else { Set-Item -LiteralPath "Env:$name" -Value $value }
}
if ([string]::IsNullOrWhiteSpace($developerPath)) { throw "The native environment did not provide PATH." }
Remove-Item -LiteralPath "Env:PATH" -ErrorAction SilentlyContinue
Remove-Item -LiteralPath "Env:Path" -ErrorAction SilentlyContinue
Set-Item -LiteralPath "Env:PATH" -Value $developerPath

$generated = Join-Path $outputRoot "generated"
New-Item -ItemType Directory -Path $generated -Force | Out-Null
& $PythonPath (Join-Path $PSScriptRoot "render_windows_setup_localization.py") `
    --source (Join-Path $repositoryRoot "packaging\windows\setup\strings.json") `
    --cpp (Join-Path $generated "setup_l10n.h")
if ($LASTEXITCODE -ne 0) { throw "Setup localization generation failed: $LASTEXITCODE." }

$architecture = if ($Variant -eq "arm64") { "ARM64" } else { "x64" }
$apiRoot = Join-Path $sdkRoot "api\build\native"
$dutilRoot = Join-Path $sdkRoot "dutil\build\native"
$executable = Join-Path $outputRoot "usque-setup.exe"
$resource = Join-Path $outputRoot "bootstrapper.res"
$arguments = @(
    "/nologo", "/std:c++20", "/EHsc", "/O2", "/MT", "/W4", "/WX", "/utf-8",
    "/DUNICODE", "/D_UNICODE", "/DNOMINMAX", "/D_WIN32_WINNT=0x0A00",
    "/I$generated", "/I$sourceRoot", "/I$runnerRoot", "/external:I$apiRoot\include", "/external:I$dutilRoot\include",
    "/external:W0", "/Fo$outputRoot\", "/Fe$executable",
    (Join-Path $sourceRoot "main.cpp"), (Join-Path $sourceRoot "platform.cpp"),
    (Join-Path $runnerRoot "shell_integration.cpp"), $resource,
    "/link", "/SUBSYSTEM:WINDOWS", "/MANIFEST:NO", "/DYNAMICBASE", "/NXCOMPAT", "/HIGHENTROPYVA",
    (Join-Path $apiRoot "v14\$architecture\balutil.lib"),
    (Join-Path $dutilRoot "v14\$architecture\dutil.lib"),
    "comctl32.lib", "dwmapi.lib", "uxtheme.lib", "user32.lib", "gdi32.lib", "ole32.lib",
    "oleaut32.lib", "shell32.lib", "shlwapi.lib", "advapi32.lib", "msi.lib", "wintrust.lib",
    "crypt32.lib", "version.lib", "rpcrt4.lib", "uuid.lib", "wininet.lib", "winhttp.lib", "oleacc.lib", "propsys.lib"
)
Push-Location $sourceRoot
try {
    & rc.exe /nologo "/fo$resource" "bootstrapper.rc"
    if ($LASTEXITCODE -ne 0) { throw "Bootstrapper resource compilation failed: $LASTEXITCODE." }
    & cl.exe @arguments
    if ($LASTEXITCODE -ne 0) { throw "Bootstrapper compilation failed: $LASTEXITCODE." }
    if ($Preview) {
        $previewExecutable = Join-Path $outputRoot "usque-setup-preview.exe"
        $previewArguments = foreach ($argument in $arguments) {
            if ($argument -eq "/Fe$executable") { "/Fe$previewExecutable" } else { $argument }
        }
        & cl.exe /DUSQUE_PREVIEW_ONLY @previewArguments
        if ($LASTEXITCODE -ne 0) { throw "Bootstrapper preview compilation failed: $LASTEXITCODE." }
        Write-Output "BOOTSTRAPPER_PREVIEW_EXE=$previewExecutable"
    }
    if ($Test) {
        $testExecutable = Join-Path $outputRoot "bootstrapper-state-test.exe"
        & cl.exe /nologo /std:c++20 /EHsc /O2 /MT /W4 /WX "/Fo$outputRoot\" "/Fe$testExecutable" "state_test.cpp"
        if ($LASTEXITCODE -ne 0) { throw "Bootstrapper state-test compilation failed: $LASTEXITCODE." }
        $native = ($Variant -eq "arm64" -and $hostArchitecture -eq "Arm64") -or ($Variant -eq "x64-v2" -and $hostArchitecture -eq "X64")
        if ($native) {
            & $testExecutable
            if ($LASTEXITCODE -ne 0) { throw "Bootstrapper state tests failed: $LASTEXITCODE." }
        } else { Write-Output "BOOTSTRAPPER_STATE_TESTS=not_run/cross-architecture" }
    }
} finally { Pop-Location }

Copy-Item -LiteralPath (Join-Path $repositoryRoot "packaging\windows\LICENSE.rtf") `
    -Destination (Join-Path $outputRoot "license.rtf") -Force
Copy-Item -LiteralPath (Join-Path $repositoryRoot "packaging\windows\setup\WIX-LICENSE.txt") `
    -Destination (Join-Path $outputRoot "wix-license.txt") -Force
Copy-Item -LiteralPath (Join-Path $repositoryRoot "packaging\windows\setup\THIRD-PARTY-NOTICES.txt") `
    -Destination (Join-Path $outputRoot "third-party-notices.txt") -Force
Write-Output "BOOTSTRAPPER_EXE=$executable"
