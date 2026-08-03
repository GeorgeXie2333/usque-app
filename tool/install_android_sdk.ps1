param(
    [string]$SdkRoot = "$env:USERPROFILE\.local\share\android-sdk",
    [string]$LogPath = "$PSScriptRoot\android-sdk-install.log"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
Start-Transcript -LiteralPath $LogPath -Force

try {
    $archiveUrl = "https://dl.google.com/android/repository/commandlinetools-win-15859902_latest.zip"
    $expectedHash = "90ae805d20434428bffcb699c290860f19bb5f66a67e6b330067e3de801fb04a"
    $archive = Join-Path $env:TEMP "usque-commandlinetools-win-15859902.zip"
    $extractRoot = Join-Path $env:TEMP "usque-commandlinetools-15859902"
    $latestRoot = Join-Path $SdkRoot "cmdline-tools\latest"

    Write-Output "Downloading Android command-line tools."
    Invoke-WebRequest -Uri $archiveUrl -OutFile $archive
    $actualHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $expectedHash) {
        throw "Android command-line tools checksum mismatch: $actualHash"
    }

    if (Test-Path -LiteralPath $extractRoot) {
        $resolvedExtract = (Resolve-Path -LiteralPath $extractRoot).Path
        $expectedParent = (Resolve-Path -LiteralPath $env:TEMP).Path
        if (-not $resolvedExtract.StartsWith($expectedParent, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove unexpected extraction path: $resolvedExtract"
        }
        Remove-Item -LiteralPath $resolvedExtract -Recurse -Force
    }

    New-Item -ItemType Directory -Force -Path $extractRoot, $latestRoot | Out-Null
    Expand-Archive -LiteralPath $archive -DestinationPath $extractRoot -Force
    Copy-Item -Path (Join-Path $extractRoot "cmdline-tools\*") -Destination $latestRoot -Recurse -Force

    $sdkManager = Join-Path $latestRoot "bin\sdkmanager.bat"
    if (-not (Test-Path -LiteralPath $sdkManager)) {
        throw "sdkmanager was not installed at $sdkManager"
    }

    Write-Output "Accepting Android SDK licenses for the requested build dependencies."
    (1..100 | ForEach-Object { "y" }) | & $sdkManager --sdk_root=$SdkRoot --licenses
    if ($LASTEXITCODE -ne 0) {
        throw "sdkmanager --licenses failed with exit code $LASTEXITCODE"
    }

    Write-Output "Installing pinned Android SDK, Build Tools, CMake, Platform Tools, and NDK."
    & $sdkManager --sdk_root=$SdkRoot `
        "platform-tools" `
        "platforms;android-36" `
        "build-tools;36.0.0" `
        "cmake;3.22.1" `
        "ndk;29.0.14206865"
    if ($LASTEXITCODE -ne 0) {
        throw "sdkmanager package install failed with exit code $LASTEXITCODE"
    }

    Write-Output "ANDROID_SDK_READY=$SdkRoot"
} finally {
    Stop-Transcript
}
