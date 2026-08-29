[CmdletBinding()]
param(
    [ValidateSet("x64-v1", "x64-v2", "arm64")]
    [string]$Variant = "x64-v2",
    [ValidateSet("build", "test", "clippy")]
    [string]$CargoAction = "build"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$platform = if ($Variant -eq "arm64") {
    @{
        RustTarget = "aarch64-pc-windows-msvc"
        VisualStudioComponent = "Microsoft.VisualStudio.Component.VC.Tools.ARM64"
        VcVarsName = "vcvarsarm64.bat"
    }
}
else {
    @{
        RustTarget = "x86_64-pc-windows-msvc"
        VisualStudioComponent = "Microsoft.VisualStudio.Component.VC.Tools.x86.x64"
        VcVarsName = "vcvars64.bat"
    }
}

$vswhereCommand = Get-Command vswhere.exe -ErrorAction SilentlyContinue
$vswhere = if ($null -ne $vswhereCommand) {
    $vswhereCommand.Source
}
else {
    Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
}
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
    throw "vswhere.exe was not found: $vswhere"
}
$visualStudioMatch = & $vswhere `
    -latest `
    -products * `
    -requires $platform.VisualStudioComponent `
    -property installationPath
$visualStudio = if ($null -eq $visualStudioMatch) {
    ""
}
else {
    ([string]$visualStudioMatch).Trim()
}
if ([string]::IsNullOrWhiteSpace($visualStudio)) {
    throw "Visual Studio C++ Build Tools for $Variant were not found."
}
$vcvars = Join-Path $visualStudio "VC\Auxiliary\Build\$($platform.VcVarsName)"
if (-not (Test-Path -LiteralPath $vcvars -PathType Leaf)) {
    throw "$($platform.VcVarsName) was not found: $vcvars"
}

# Import the complete MSVC environment into this PowerShell process. CMake
# 3.22 cannot name the Visual Studio 18 generator, so BoringSSL must use Ninja
# while cl.exe/link.exe come from this initialized developer environment.
$environmentLines = & $env:ComSpec /d /s /c "call `"$vcvars`" >nul && set"
if ($LASTEXITCODE -ne 0) {
    throw "$($platform.VcVarsName) failed with exit code $LASTEXITCODE."
}
$developerPath = $null
foreach ($line in $environmentLines) {
    $separator = $line.IndexOf('=')
    if ($separator -le 0) {
        continue
    }
    $name = $line.Substring(0, $separator)
    $value = $line.Substring($separator + 1)
    if ($name -ieq "Path") {
        # Codex can inject both PATH and Path into cmd.exe. vcvars updates only
        # one of them, so retain the entry that actually contains this Visual
        # Studio installation and discard the stale duplicate below.
        if ($value.IndexOf($visualStudio, [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
            $developerPath = $value
        }
        continue
    }
    Set-Item -LiteralPath "Env:$name" -Value $value
}
if ([string]::IsNullOrWhiteSpace($developerPath)) {
    throw "$($platform.VcVarsName) did not provide a Visual Studio PATH."
}
Remove-Item -LiteralPath "Env:PATH" -ErrorAction SilentlyContinue
Remove-Item -LiteralPath "Env:Path" -ErrorAction SilentlyContinue
Set-Item -LiteralPath "Env:PATH" -Value $developerPath

$localProperties = Join-Path $repositoryRoot "apps\usque_gui\android\local.properties"
$sdkRoot = $null
$cmakeExecutable = $null
$ninjaExecutable = $null
if (Test-Path -LiteralPath $localProperties -PathType Leaf) {
    $sdkLine = Get-Content -LiteralPath $localProperties |
        Where-Object { $_.StartsWith("sdk.dir=") } |
        Select-Object -First 1
    if (-not [string]::IsNullOrWhiteSpace($sdkLine)) {
        $sdkRoot = $sdkLine.Substring("sdk.dir=".Length).Replace('\:', ':').Replace('\\', '\')
        $cmakeRoot = Join-Path $sdkRoot "cmake"
        if (Test-Path -LiteralPath $cmakeRoot -PathType Container) {
            $cmakeDirectory = Get-ChildItem -LiteralPath $cmakeRoot -Directory |
                Where-Object {
                    (Test-Path -LiteralPath (Join-Path $_.FullName "bin\cmake.exe")) -and
                    (Test-Path -LiteralPath (Join-Path $_.FullName "bin\ninja.exe"))
                } |
                Sort-Object { [version]$_.Name } -Descending |
                Select-Object -First 1
            if ($null -ne $cmakeDirectory) {
                $cmakeExecutable = Join-Path $cmakeDirectory.FullName "bin\cmake.exe"
                $ninjaExecutable = Join-Path $cmakeDirectory.FullName "bin\ninja.exe"
            }
        }
    }
}
if ($null -eq $cmakeExecutable -or $null -eq $ninjaExecutable) {
    $cmakeCommand = Get-Command cmake.exe -ErrorAction SilentlyContinue
    $ninjaCommand = Get-Command ninja.exe -ErrorAction SilentlyContinue
    if ($null -eq $cmakeCommand -or $null -eq $ninjaCommand) {
        throw "CMake and Ninja were not found for the native $Variant build."
    }
    $cmakeExecutable = $cmakeCommand.Source
    $ninjaExecutable = $ninjaCommand.Source
}
$cmakeBin = Split-Path -Parent $cmakeExecutable
$ninjaBin = Split-Path -Parent $ninjaExecutable
$env:CMAKE = $cmakeExecutable
$env:CMAKE_GENERATOR = "Ninja"
$env:CMAKE_MAKE_PROGRAM = $ninjaExecutable
$nativeToolPaths = @($cmakeBin, $ninjaBin) | Select-Object -Unique
$env:PATH = "$(($nativeToolPaths -join ';'));$env:PATH"

# boring-sys runs bindgen even for a native Windows release build. Prefer an
# explicitly supplied libclang, then Visual Studio's LLVM component, and finally
# an available Android NDK. This keeps a clean
# shell from succeeding only because a previous build happened to populate the
# bindgen output cache.
$libClangDirectory = $null
if (-not [string]::IsNullOrWhiteSpace($env:LIBCLANG_PATH) -and
    (Test-Path -LiteralPath (Join-Path $env:LIBCLANG_PATH "libclang.dll") -PathType Leaf)) {
    $libClangDirectory = $env:LIBCLANG_PATH
}
if ($null -eq $libClangDirectory) {
    $visualStudioLibClang = & $vswhere -latest -products * -find "**\libclang.dll" |
        Where-Object {
            if ($Variant -eq "arm64") {
                $_ -match '[\\/]ARM64[\\/]'
            }
            else {
                $_ -notmatch '[\\/]ARM64[\\/]'
            }
        } |
        Select-Object -First 1
    if (-not [string]::IsNullOrWhiteSpace($visualStudioLibClang)) {
        $libClangDirectory = Split-Path -Parent $visualStudioLibClang
    }
}
if ($null -eq $libClangDirectory) {
    $standaloneLibClang = Join-Path $env:ProgramFiles "LLVM\bin\libclang.dll"
    if (Test-Path -LiteralPath $standaloneLibClang -PathType Leaf) {
        $libClangDirectory = Split-Path -Parent $standaloneLibClang
    }
}
if ($null -eq $libClangDirectory -and $Variant -ne "arm64" -and $null -ne $sdkRoot) {
    $ndkRoot = Join-Path $sdkRoot "ndk"
    if (Test-Path -LiteralPath $ndkRoot -PathType Container) {
        $ndkLibClang = Get-ChildItem -LiteralPath $ndkRoot -Directory |
            Sort-Object { [version]$_.Name } -Descending |
            ForEach-Object {
                Join-Path $_.FullName "toolchains\llvm\prebuilt\windows-x86_64\bin\libclang.dll"
            } |
            Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
            Select-Object -First 1
        if (-not [string]::IsNullOrWhiteSpace($ndkLibClang)) {
            $libClangDirectory = Split-Path -Parent $ndkLibClang
        }
    }
}
if ($null -eq $libClangDirectory) {
    throw "A loadable $Variant libclang.dll was not found."
}
$env:LIBCLANG_PATH = $libClangDirectory
$env:PATH = "$libClangDirectory;$env:PATH"

$previousRustFlags = $env:RUSTFLAGS
if ($Variant -eq "x64-v2") {
    $targetCpuFlag = "-C target-cpu=x86-64-v2"
    $env:RUSTFLAGS = if ([string]::IsNullOrWhiteSpace($previousRustFlags)) {
        $targetCpuFlag
    }
    else {
        "$previousRustFlags $targetCpuFlag"
    }
}

Push-Location $repositoryRoot
try {
    $cargoArguments = switch ($CargoAction) {
        "build" {
            @(
                "build",
                "--locked",
                "--release",
                "--target", $platform.RustTarget,
                "--package", "usque-agent",
                "--package", "usque-engine",
                "--package", "usque-update",
                "--package", "usque-uninstall"
            )
        }
        "test" {
            @("test", "--locked", "--workspace", "--all-targets")
        }
        "clippy" {
            @("clippy", "--locked", "--workspace", "--all-targets", "--", "-D", "warnings")
        }
    }
    & cargo @cargoArguments
    if ($LASTEXITCODE -ne 0) {
        throw "Windows Cargo $CargoAction failed with exit code $LASTEXITCODE."
    }
}
finally {
    $env:RUSTFLAGS = $previousRustFlags
    Pop-Location
}

Write-Output "WINDOWS_CARGO_OK=$CargoAction/$Variant"
