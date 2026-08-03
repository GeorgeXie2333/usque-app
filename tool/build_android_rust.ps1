[CmdletBinding()]
param(
    # Named BuildProfile to avoid PowerShell automatic $Profile; keep -Profile CLI alias.
    [Alias("Profile")]
    [ValidateSet("debug", "release")]
    [string]$BuildProfile = "debug",
    [string]$NdkRoot = "",
    [ValidateSet("all", "arm64-v8a", "armeabi-v7a", "x86_64")]
    [string]$AbiFilter = "all"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$androidRoot = Join-Path $repoRoot "apps/usque_gui/android"
$jniRoot = Join-Path $androidRoot "app/src/main/jniLibs"
$ndkVersion = "29.0.14206865"
$minimumApi = "26"

if ([string]::IsNullOrWhiteSpace($NdkRoot)) {
    if (-not [string]::IsNullOrWhiteSpace($env:ANDROID_NDK_HOME)) {
        $NdkRoot = $env:ANDROID_NDK_HOME
    } elseif (-not [string]::IsNullOrWhiteSpace($env:ANDROID_NDK_ROOT)) {
        $NdkRoot = $env:ANDROID_NDK_ROOT
    } elseif (-not [string]::IsNullOrWhiteSpace($env:ANDROID_SDK_ROOT)) {
        $NdkRoot = Join-Path $env:ANDROID_SDK_ROOT "ndk/$ndkVersion"
    } else {
        $localProperties = Join-Path $androidRoot "local.properties"
        if (Test-Path -LiteralPath $localProperties) {
            $sdkLine = Get-Content -LiteralPath $localProperties |
                Where-Object { $_ -like "sdk.dir=*" } |
                Select-Object -First 1
            if ($sdkLine) {
                # Java properties escape both the drive colon and path
                # separators. Decode those two forms before passing the path
                # back to PowerShell.
                $sdkRoot = $sdkLine.Substring("sdk.dir=".Length).Replace("\:", ":").Replace("\\", "\")
                $NdkRoot = Join-Path $sdkRoot "ndk/$ndkVersion"
            }
        }
    }
}

if ([string]::IsNullOrWhiteSpace($NdkRoot) -or -not (Test-Path -LiteralPath $NdkRoot)) {
    throw "Android NDK $ndkVersion was not found. Set ANDROID_SDK_ROOT or ANDROID_NDK_HOME."
}
$NdkRoot = (Resolve-Path -LiteralPath $NdkRoot).Path
$env:ANDROID_NDK_HOME = $NdkRoot
$env:ANDROID_NDK_ROOT = $NdkRoot
$sdkRootFromNdk = Split-Path -Parent (Split-Path -Parent $NdkRoot)
$androidCmakeBin = Join-Path $sdkRootFromNdk "cmake/3.22.1/bin"
$isWindowsHost = $env:OS -eq "Windows_NT"
$executableSuffix = if ($isWindowsHost) { ".exe" } else { "" }
$commandSuffix = if ($isWindowsHost) { ".cmd" } else { "" }
$pathSeparator = [IO.Path]::PathSeparator
$cmakeExecutable = Join-Path $androidCmakeBin "cmake$executableSuffix"
$ninjaExecutable = Join-Path $androidCmakeBin "ninja$executableSuffix"
if (-not (Test-Path -LiteralPath $cmakeExecutable)) {
    throw "Pinned Android CMake 3.22.1 was not found at $androidCmakeBin"
}
if (-not (Test-Path -LiteralPath $ninjaExecutable)) {
    throw "Pinned Android Ninja was not found at $androidCmakeBin"
}
$env:PATH = "$androidCmakeBin$pathSeparator$env:PATH"
$env:CMAKE_GENERATOR = "Ninja"
$env:CMAKE_MAKE_PROGRAM = $ninjaExecutable
# Cross-target debug artifacts are large and are never reused by the signed
# release build. Disabling incremental compilation keeps a local three-ABI
# verification from recreating tens of gigabytes under target/.
$env:CARGO_INCREMENTAL = "0"

$hostTag = if ($isWindowsHost) { "windows-x86_64" } else { "linux-x86_64" }
$toolchainBin = Join-Path $NdkRoot "toolchains/llvm/prebuilt/$hostTag/bin"
if (-not (Test-Path -LiteralPath $toolchainBin)) {
    throw "NDK LLVM toolchain was not found at $toolchainBin"
}
$libclangDirectory =
    if ($isWindowsHost) {
        $toolchainBin
    } else {
        Join-Path $NdkRoot "toolchains/llvm/prebuilt/$hostTag/lib64"
    }
$libclang =
    if ($isWindowsHost) {
        Join-Path $libclangDirectory "libclang.dll"
    } else {
        Join-Path $libclangDirectory "libclang.so"
    }
if (-not (Test-Path -LiteralPath $libclang)) {
    throw "NDK libclang was not found at $libclang"
}
$env:LIBCLANG_PATH = $libclangDirectory
$env:PATH = "$toolchainBin$pathSeparator$env:PATH"

$cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
$cargoPath = if ($cargoCommand) { $cargoCommand.Source } else { "" }
if ([string]::IsNullOrWhiteSpace($cargoPath) -and $isWindowsHost) {
    $userCargo = Join-Path $env:USERPROFILE ".cargo/bin/cargo.exe"
    if (Test-Path -LiteralPath $userCargo) {
        $cargoPath = $userCargo
    }
}
if ([string]::IsNullOrWhiteSpace($cargoPath)) {
    throw "cargo was not found in PATH or the standard user install directory."
}

$targets = @(
    @{
        RustTarget = "aarch64-linux-android"
        Abi = "arm64-v8a"
        Linker = "aarch64-linux-android$minimumApi-clang$commandSuffix"
        CxxLinker = "aarch64-linux-android$minimumApi-clang++$commandSuffix"
        Environment = "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"
        CcEnvironment = "CC_aarch64_linux_android"
        CxxEnvironment = "CXX_aarch64_linux_android"
        ArEnvironment = "AR_aarch64_linux_android"
    },
    @{
        RustTarget = "armv7-linux-androideabi"
        Abi = "armeabi-v7a"
        Linker = "armv7a-linux-androideabi$minimumApi-clang$commandSuffix"
        CxxLinker = "armv7a-linux-androideabi$minimumApi-clang++$commandSuffix"
        Environment = "CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER"
        CcEnvironment = "CC_armv7_linux_androideabi"
        CxxEnvironment = "CXX_armv7_linux_androideabi"
        ArEnvironment = "AR_armv7_linux_androideabi"
    },
    @{
        RustTarget = "x86_64-linux-android"
        Abi = "x86_64"
        Linker = "x86_64-linux-android$minimumApi-clang$commandSuffix"
        CxxLinker = "x86_64-linux-android$minimumApi-clang++$commandSuffix"
        Environment = "CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER"
        CcEnvironment = "CC_x86_64_linux_android"
        CxxEnvironment = "CXX_x86_64_linux_android"
        ArEnvironment = "AR_x86_64_linux_android"
    }
)
if ($AbiFilter -ne "all") {
    $targets = @($targets | Where-Object { $_.Abi -eq $AbiFilter })
}

Push-Location $repoRoot
try {
    foreach ($target in $targets) {
        $linker = (Join-Path $toolchainBin $target.Linker).Replace("\", "/")
        if (-not (Test-Path -LiteralPath $linker)) {
            throw "Android linker was not found: $linker"
        }
        Set-Item -Path "Env:$($target.Environment)" -Value $linker
        $cxx = (Join-Path $toolchainBin $target.CxxLinker).Replace("\", "/")
        if (-not (Test-Path -LiteralPath $cxx)) {
            throw "Android C++ target compiler was not found: $cxx"
        }

        # Native Rust dependencies such as ring use cc-rs directly and need
        # the NDK's API-qualified wrappers. Never point these variables at the
        # host-facing clang.exe: doing so loses both the Android API level and,
        # for 32-bit ARM, the armv7 architecture selection.
        Set-Item -Path "Env:$($target.CcEnvironment)" -Value $linker
        Set-Item -Path "Env:$($target.CxxEnvironment)" -Value $cxx
        $archiver = (Join-Path $toolchainBin "llvm-ar$executableSuffix").Replace("\", "/")
        Set-Item -Path "Env:$($target.ArEnvironment)" -Value $archiver

        $cargoArguments = @(
            "build",
            "--locked",
            "--package", "usque-android",
            "--target", $target.RustTarget
        )
        if ($BuildProfile -eq "release") {
            $cargoArguments += "--release"
        }

        & $cargoPath @cargoArguments
        if ($LASTEXITCODE -ne 0) {
            throw "Rust Android build failed for $($target.RustTarget)."
        }

        $artifactProfile = if ($BuildProfile -eq "release") { "release" } else { "debug" }
        $source = Join-Path $repoRoot "target/$($target.RustTarget)/$artifactProfile/libusque_android.so"
        if (-not (Test-Path -LiteralPath $source)) {
            throw "Expected Rust library was not produced: $source"
        }
        $destination = Join-Path $jniRoot $target.Abi
        New-Item -ItemType Directory -Path $destination -Force | Out-Null
        Copy-Item -LiteralPath $source -Destination (Join-Path $destination "libusque_android.so") -Force
    }
} finally {
    Pop-Location
}

Write-Output "RUST_ANDROID_LIBS_READY=$jniRoot"
