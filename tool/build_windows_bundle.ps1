[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("x64-v2", "arm64")]
    [string]$Variant,

    [Parameter(Mandatory = $true)]
    [string]$MsiPath,

    [Parameter(Mandatory = $true)]
    [string]$TransformDirectory,

    [Parameter(Mandatory = $true)]
    [string]$OutputPath,

    [Parameter(Mandatory = $true)]
    [ValidatePattern("^[0-9A-Fa-f]{64}$")]
    [string]$SignerSha256,

    [Parameter(Mandatory = $true)]
    [ValidatePattern("^v?[0-9]+\.[0-9]+\.[0-9]+(?:-beta\.[0-9]+)?$")]
    [string]$Version,

    [switch]$AllowPinnedUntrustedRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$localizedCultures = @(
    "ar-SA",
    "de-DE",
    "es-ES",
    "fa-IR",
    "fr-FR",
    "id-ID",
    "it-IT",
    "ja-JP",
    "ko-KR",
    "nl-NL",
    "pl-PL",
    "pt-BR",
    "ru-RU",
    "th-TH",
    "tr-TR",
    "uk-UA",
    "vi-VN",
    "zh-CN",
    "zh-HK",
    "zh-TW"
)

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$sourcePath = Join-Path $repositoryRoot "packaging\windows\UsqueBundle.wxs"
$iconPath = Join-Path $repositoryRoot "assets\branding\usque-app-icon.ico"
$logoPath = Join-Path $repositoryRoot "assets\branding\usque-app-icon.png"
$bootstrapperExtension = "WixToolset.BootstrapperApplications.wixext/5.0.2"

$resolvedMsi = (Resolve-Path -LiteralPath $MsiPath -ErrorAction Stop).Path
$resolvedTransforms = (Resolve-Path -LiteralPath $TransformDirectory -ErrorAction Stop).Path
if (-not (Test-Path -LiteralPath $resolvedMsi -PathType Leaf)) {
    throw "Bundle MSI payload is not a file: $resolvedMsi"
}
if (-not (Test-Path -LiteralPath $resolvedTransforms -PathType Container)) {
    throw "Bundle transform payload is not a directory: $resolvedTransforms"
}

$displayVersion = $Version.TrimStart("v")
$expectedMsiName = "usque-v$displayVersion-windows-$Variant.msi"
if (-not [StringComparer]::OrdinalIgnoreCase.Equals(
        [IO.Path]::GetFileName($resolvedMsi),
        $expectedMsiName
    )) {
    throw "Bundle MSI filename mismatch. Expected $expectedMsiName, got $([IO.Path]::GetFileName($resolvedMsi))."
}

$actualTransforms = @(
    Get-ChildItem -LiteralPath $resolvedTransforms -File -Filter "*.mst" |
        ForEach-Object BaseName |
        Sort-Object
)
if (@(Compare-Object ($localizedCultures | Sort-Object) $actualTransforms).Count -ne 0) {
    throw "Bundle transforms do not match the supported culture contract."
}

& (Join-Path $PSScriptRoot "verify_windows_authenticode.ps1") `
    -Path $resolvedMsi `
    -SignerSha256 $SignerSha256 `
    -AllowPinnedUntrustedRoot:$AllowPinnedUntrustedRoot | Out-Null

$msiVersion = & (Join-Path $PSScriptRoot "convert_to_msi_version.ps1") -SemVer $Version
$expectedAgentVersion = "$msiVersion.0"
& (Join-Path $PSScriptRoot "verify_windows_msi.ps1") `
    -MsiPath $resolvedMsi `
    -Variant $Variant `
    -ExpectedMsiVersion $msiVersion `
    -ExpectedDisplayVersion $displayVersion `
    -ExpectedAgentFileVersion $expectedAgentVersion `
    -ExpectedMsiLanguage 1033 `
    -SignerSha256 $SignerSha256 | Out-Null

$bundleUpgradeCodes = @{
    "x64-v2" = "33C3EFF5-32C9-4BD6-B923-FA8FF6506CE3"
    "arm64"  = "EDE480D1-DE1B-4DC5-BE18-084CCABAE9D0"
}
$architecture = if ($Variant -eq "arm64") { "arm64" } else { "x64" }
$resolvedOutput = [IO.Path]::GetFullPath($OutputPath)
$outputParent = Split-Path -Parent $resolvedOutput
New-Item -ItemType Directory -Path $outputParent -Force | Out-Null
$intermediate = Join-Path $outputParent "wix-bundle-$Variant"
New-Item -ItemType Directory -Path $intermediate -Force | Out-Null

Push-Location $repositoryRoot
try {
    & dotnet tool restore
    if ($LASTEXITCODE -ne 0) {
        throw "dotnet tool restore failed with exit code $LASTEXITCODE."
    }
    & dotnet tool run wix -- extension add $bootstrapperExtension
    if ($LASTEXITCODE -ne 0) {
        throw "WiX bootstrapper extension restore failed with exit code $LASTEXITCODE."
    }
    & dotnet tool run wix -- build `
        -arch $architecture `
        -ext $bootstrapperExtension `
        -define "DisplayVersion=$displayVersion" `
        -define "BundleVersion=$displayVersion" `
        -define "BundleUpgradeCode=$($bundleUpgradeCodes[$Variant])" `
        -define "Variant=$Variant" `
        -define "IconPath=$iconPath" `
        -define "LogoPath=$logoPath" `
        -define "MsiPath=$resolvedMsi" `
        -define "TransformDirectory=$resolvedTransforms" `
        -defaultcompressionlevel high `
        -intermediateFolder $intermediate `
        -pdbtype none `
        -out $resolvedOutput `
        $sourcePath
    if ($LASTEXITCODE -ne 0) {
        throw "WiX bundle build failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}

if (-not (Test-Path -LiteralPath $resolvedOutput -PathType Leaf)) {
    throw "WiX did not produce the expected bundle: $resolvedOutput"
}

& (Join-Path $PSScriptRoot "verify_windows_bundle.ps1") `
    -BundlePath $resolvedOutput `
    -Variant $Variant `
    -Version $Version `
    -SignerSha256 $SignerSha256 `
    -ExpectedAgentFileVersion $expectedAgentVersion | Out-Null

Write-Output $resolvedOutput
