[CmdletBinding()]
param(
    [ValidateSet("x64-v1", "x64-v2")]
    [string]$Variant = "x64-v2",
    [ValidateSet("build", "test", "clippy")]
    [string]$CargoAction = "build"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
    throw "vswhere.exe was not found: $vswhere"
}
$visualStudio = (& $vswhere `
        -latest `
        -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath).Trim()
if ([string]::IsNullOrWhiteSpace($visualStudio)) {
    throw "Visual Studio C++ Build Tools were not found."
}
$vcvars = Join-Path $visualStudio "VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path -LiteralPath $vcvars -PathType Leaf)) {
    throw "vcvars64.bat was not found: $vcvars"
}

# Import the complete MSVC environment into this PowerShell process. CMake
# 3.22 cannot name the Visual Studio 18 generator, so BoringSSL must use Ninja
# while cl.exe/link.exe come from this initialized developer environment.
$environmentLines = & $env:ComSpec /d /s /c "call `"$vcvars`" >nul && set"
if ($LASTEXITCODE -ne 0) {
    throw "vcvars64.bat failed with exit code $LASTEXITCODE."
}
foreach ($line in $environmentLines) {
    $separator = $line.IndexOf('=')
    if ($separator -le 0) {
        continue
    }
    $name = $line.Substring(0, $separator)
    $value = $line.Substring($separator + 1)
    Set-Item -LiteralPath "Env:$name" -Value $value
}

$localProperties = Join-Path $repositoryRoot "apps\usque_gui\android\local.properties"
$sdkLine = Get-Content -LiteralPath $localProperties |
    Where-Object { $_.StartsWith("sdk.dir=") } |
    Select-Object -First 1
if ([string]::IsNullOrWhiteSpace($sdkLine)) {
    throw "Android SDK path is missing from $localProperties."
}
$sdkRoot = $sdkLine.Substring("sdk.dir=".Length).Replace('\:', ':').Replace('\\', '\')
$cmakeDirectory = Get-ChildItem -LiteralPath (Join-Path $sdkRoot "cmake") -Directory |
    Where-Object {
        (Test-Path -LiteralPath (Join-Path $_.FullName "bin\cmake.exe")) -and
        (Test-Path -LiteralPath (Join-Path $_.FullName "bin\ninja.exe"))
    } |
    Sort-Object { [version]$_.Name } -Descending |
    Select-Object -First 1
if ($null -eq $cmakeDirectory) {
    throw "Android SDK CMake/Ninja were not found below $sdkRoot\cmake."
}
$cmakeBin = Join-Path $cmakeDirectory.FullName "bin"
$env:CMAKE = Join-Path $cmakeBin "cmake.exe"
$env:CMAKE_GENERATOR = "Ninja"
$env:PATH = "$cmakeBin;$env:PATH"

# boring-sys runs bindgen even for a native Windows release build. Prefer an
# explicitly supplied libclang, then Visual Studio's LLVM component, and finally
# the pinned Android NDK already required by this workspace. This keeps a clean
# shell from succeeding only because a previous build happened to populate the
# bindgen output cache.
$libClangDirectory = $null
if (-not [string]::IsNullOrWhiteSpace($env:LIBCLANG_PATH) -and
    (Test-Path -LiteralPath (Join-Path $env:LIBCLANG_PATH "libclang.dll") -PathType Leaf)) {
    $libClangDirectory = $env:LIBCLANG_PATH
}
if ($null -eq $libClangDirectory) {
    $visualStudioLibClang = & $vswhere -latest -products * -find "**\libclang.dll" |
        # VS 18 can install the ARM64 LLVM tools beside x64. bindgen will find
        # an ARM64 DLL by name but cannot load it in the x64 Cargo process.
        Where-Object { $_ -notmatch '[\\/]ARM64[\\/]' } |
        Select-Object -First 1
    if (-not [string]::IsNullOrWhiteSpace($visualStudioLibClang)) {
        $libClangDirectory = Split-Path -Parent $visualStudioLibClang
    }
}
if ($null -eq $libClangDirectory) {
    $ndkLibClang = Get-ChildItem -LiteralPath (Join-Path $sdkRoot "ndk") -Directory |
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
if ($null -eq $libClangDirectory) {
    throw "libclang.dll was not found in Visual Studio or the installed Android NDKs."
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
                "--target", "x86_64-pc-windows-msvc",
                "--package", "usque-agent",
                "--package", "usque-engine"
            )
        }
        "test" {
            @("test", "--locked", "--workspace")
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
