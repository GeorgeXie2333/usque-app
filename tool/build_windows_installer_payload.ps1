[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("x64-v2", "arm64")]
    [string]$Variant,

    [Parameter(Mandatory = $true)]
    [string]$AppDirectory,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,

    [Parameter(Mandatory = $true)]
    [string]$WorkingDirectory,

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
$locDirectory = Join-Path $repositoryRoot "packaging\windows\loc"
$expectedSources = @("en-US") + $localizedCultures
$actualSources = @(
    Get-ChildItem -LiteralPath $locDirectory -File -Filter "*.wxl" |
        ForEach-Object BaseName |
        Sort-Object
)
if (@(Compare-Object ($expectedSources | Sort-Object) $actualSources).Count -ne 0) {
    throw "Windows installer localization sources do not match the supported culture contract."
}

$workingRoot = [IO.Path]::GetFullPath($WorkingDirectory)
if (Test-Path -LiteralPath $workingRoot) {
    $existing = Get-ChildItem -LiteralPath $workingRoot -Force | Select-Object -First 1
    if ($null -ne $existing) {
        throw "Windows installer working directory must be empty: $workingRoot"
    }
}
else {
    New-Item -ItemType Directory -Path $workingRoot -Force | Out-Null
}
$workingRoot = (Resolve-Path -LiteralPath $workingRoot).Path

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$outputRoot = (Resolve-Path -LiteralPath $OutputDirectory).Path
$localizedOutput = Join-Path $workingRoot "localized-msi"
$transformDirectory = Join-Path $workingRoot "transforms"
$cabCacheDirectory = Join-Path $workingRoot "cab-cache"
New-Item `
    -ItemType Directory `
    -Path $localizedOutput, $transformDirectory, $cabCacheDirectory `
    -Force | Out-Null

$displayVersion = $Version.TrimStart("v")
$baseMsiPath = Join-Path $outputRoot "usque-v$displayVersion-windows-$Variant.msi"
$productCode = ([guid]::NewGuid()).ToString("D").ToUpperInvariant()
$buildMsi = Join-Path $PSScriptRoot "build_windows_msi.ps1"
$commonArguments = @{
    Variant                  = $Variant
    AppDirectory             = $AppDirectory
    SignerSha256             = $SignerSha256
    Version                  = $Version
    ProductCode              = $productCode
    CabCacheDirectory        = $cabCacheDirectory
    AllowPinnedUntrustedRoot = $AllowPinnedUntrustedRoot
}

& $buildMsi @commonArguments -Culture "en-US" -OutputDirectory $outputRoot | Out-Null
if (-not (Test-Path -LiteralPath $baseMsiPath -PathType Leaf)) {
    throw "English base MSI was not produced: $baseMsiPath"
}

foreach ($culture in $localizedCultures) {
    & $buildMsi `
        @commonArguments `
        -Culture $culture `
        -OutputDirectory $localizedOutput | Out-Null

    $localizedMsiPath = Join-Path `
        $localizedOutput `
        "usque-v$displayVersion-windows-$Variant-$culture.msi"
    if (-not (Test-Path -LiteralPath $localizedMsiPath -PathType Leaf)) {
        throw "Localized MSI was not produced for $culture`: $localizedMsiPath"
    }

    $transformPath = Join-Path $transformDirectory "$culture.mst"
    & dotnet tool run wix -- msi transform `
        $baseMsiPath `
        $localizedMsiPath `
        -out $transformPath `
        -t language
    if ($LASTEXITCODE -ne 0) {
        throw "WiX language transform generation failed for $culture with exit code $LASTEXITCODE."
    }
    if (
        -not (Test-Path -LiteralPath $transformPath -PathType Leaf) -or
        (Get-Item -LiteralPath $transformPath).Length -le 0
    ) {
        throw "WiX produced an empty or missing language transform for $culture."
    }
}

$actualTransforms = @(
    Get-ChildItem -LiteralPath $transformDirectory -File -Filter "*.mst" |
        ForEach-Object BaseName |
        Sort-Object
)
if (@(Compare-Object ($localizedCultures | Sort-Object) $actualTransforms).Count -ne 0) {
    throw "Generated Windows language transforms do not match the supported culture contract."
}

Write-Output "BASE_MSI=$baseMsiPath"
Write-Output "TRANSFORM_DIRECTORY=$transformDirectory"
Write-Output "PRODUCT_CODE=$productCode"
