[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("x64-v1", "x64-v2", "arm64")]
    [string]$Variant,

    [Parameter(Mandatory = $true)]
    [string]$AppDirectory,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,

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

function Resolve-ExistingDirectory {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$Description
    )

    $resolved = Resolve-Path -LiteralPath $Path -ErrorAction Stop
    if (-not (Test-Path -LiteralPath $resolved.Path -PathType Container)) {
        throw "$Description is not a directory: $($resolved.Path)"
    }
    return $resolved.Path
}

function Get-CertificateSha256 {
    param(
        [Parameter(Mandatory = $true)]
        [System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate
    )

    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        $digest = $sha256.ComputeHash($Certificate.GetRawCertData())
        return (($digest | ForEach-Object { $_.ToString("X2") }) -join "")
    }
    finally {
        $sha256.Dispose()
    }
}

function Assert-NoReparsePoint {
    param([Parameter(Mandatory = $true)][string]$Root)

    $rootItem = Get-Item -LiteralPath $Root -Force
    if (($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Application directory must not be a reparse point: $Root"
    }

    $reparsePoint = Get-ChildItem -LiteralPath $Root -Force -Recurse |
        Where-Object {
            ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0
        } |
        Select-Object -First 1
    if ($null -ne $reparsePoint) {
        throw "Application payload contains a reparse point: $($reparsePoint.FullName)"
    }
}

function Assert-ReleaseSignature {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$ExpectedSigner,
        [switch]$AllowUntrustedRoot
    )

    $binaries = @(
        Get-ChildItem -LiteralPath $Root -File -Recurse |
            Where-Object { $_.Extension -in ".exe", ".dll" }
    )
    if ($binaries.Count -eq 0) {
        throw "Application payload has no executable binaries."
    }

    $wintunPath = [IO.Path]::GetFullPath((Join-Path $Root "wintun.dll"))
    foreach ($binary in $binaries) {
        $binaryPath = [IO.Path]::GetFullPath($binary.FullName)
        if ([StringComparer]::OrdinalIgnoreCase.Equals($binaryPath, $wintunPath)) {
            continue
        }

        $signature = Get-AuthenticodeSignature -LiteralPath $binaryPath
        $valid = $signature.Status -eq [System.Management.Automation.SignatureStatus]::Valid
        $pinnedSelfSigned =
        $AllowUntrustedRoot -and
        $signature.Status -eq [System.Management.Automation.SignatureStatus]::UnknownError -and
        $null -ne $signature.SignerCertificate -and
        $signature.SignerCertificate.Subject -eq $signature.SignerCertificate.Issuer
        if (-not $valid -and -not $pinnedSelfSigned) {
            throw "Authenticode verification failed for $binaryPath ($($signature.Status))."
        }
        if ($null -eq $signature.SignerCertificate) {
            throw "No signer certificate was returned for $binaryPath."
        }

        $actualSigner = Get-CertificateSha256 -Certificate $signature.SignerCertificate
        if (-not [StringComparer]::OrdinalIgnoreCase.Equals($actualSigner, $ExpectedSigner)) {
            throw "Unexpected signer for $binaryPath. Expected $ExpectedSigner, got $actualSigner."
        }
    }
}

function Assert-OfficialWintun {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$BuildVariant
    )

    $expectedHashes = @{
        "x64-v1" = "E5DA8447DC2C320EDC0FC52FA01885C103DE8C118481F683643CACC3220DAFCE"
        "x64-v2" = "E5DA8447DC2C320EDC0FC52FA01885C103DE8C118481F683643CACC3220DAFCE"
        "arm64"  = "F7BA89005544BE9D85231A9E0D5F23B2D15B3311667E2DAD0DEBD344918A3F80"
    }

    $wintunPath = Join-Path $Root "wintun.dll"
    if (-not (Test-Path -LiteralPath $wintunPath -PathType Leaf)) {
        throw "Official Wintun DLL is missing: $wintunPath"
    }

    $actual = (Get-FileHash -LiteralPath $wintunPath -Algorithm SHA256).Hash
    $expected = $expectedHashes[$BuildVariant]
    if (-not [StringComparer]::OrdinalIgnoreCase.Equals($actual, $expected)) {
        throw "Wintun 0.14.1 hash mismatch for $BuildVariant. Expected $expected, got $actual."
    }

    $nestedWintun = Get-ChildItem -LiteralPath $Root -File -Recurse -Filter "wintun.dll" |
        Where-Object {
            -not [StringComparer]::OrdinalIgnoreCase.Equals(
                [IO.Path]::GetFullPath($_.FullName),
                [IO.Path]::GetFullPath($wintunPath)
            )
        } |
        Select-Object -First 1
    if ($null -ne $nestedWintun) {
        throw "Unexpected additional Wintun DLL: $($nestedWintun.FullName)"
    }
}

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$sourcePath = Join-Path $repositoryRoot "packaging\windows\Usque.wxs"
$uiSourcePath = Join-Path $repositoryRoot "packaging\windows\UsqueUI.wxs"
$licensePath = Join-Path $repositoryRoot "packaging\windows\LICENSE.rtf"
$iconPath = Join-Path $repositoryRoot "assets\branding\usque-app-icon.ico"
$uiExtension = "WixToolset.UI.wixext/5.0.2"
$appRoot = Resolve-ExistingDirectory -Path $AppDirectory -Description "Application payload"

$requiredFiles = @("usque.exe", "usque-engine.exe", "usque-agent.exe", "usque-uninstall.exe", "wintun.dll")
foreach ($requiredFile in $requiredFiles) {
    $candidate = Join-Path $appRoot $requiredFile
    if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
        throw "Required application file is missing: $candidate"
    }
}

$pdb = Get-ChildItem -LiteralPath $appRoot -File -Recurse -Filter "*.pdb" |
    Select-Object -First 1
if ($null -ne $pdb) {
    throw "Release payload must not contain PDB files: $($pdb.FullName)"
}

$testExecutable = Get-ChildItem `
    -LiteralPath $appRoot `
    -File `
    -Recurse `
    -Filter "usque_zero_trust_test.exe" |
    Select-Object -First 1
if ($null -ne $testExecutable) {
    throw "Release payload must not contain the native test executable: $($testExecutable.FullName)"
}

Assert-NoReparsePoint -Root $appRoot
Assert-OfficialWintun -Root $appRoot -BuildVariant $Variant
$normalizedSigner = $SignerSha256.ToUpperInvariant()
Assert-ReleaseSignature `
    -Root $appRoot `
    -ExpectedSigner $normalizedSigner `
    -AllowUntrustedRoot:$AllowPinnedUntrustedRoot

if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
    throw "WiX authoring is missing: $sourcePath"
}
if (-not (Test-Path -LiteralPath $uiSourcePath -PathType Leaf)) {
    throw "WiX UI authoring is missing: $uiSourcePath"
}
if (-not (Test-Path -LiteralPath $licensePath -PathType Leaf)) {
    throw "Installer license is missing: $licensePath"
}
if (-not (Test-Path -LiteralPath $iconPath -PathType Leaf)) {
    throw "Installer icon is missing: $iconPath"
}

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$outputRoot = (Resolve-Path -LiteralPath $OutputDirectory).Path
$displayVersion = $Version.TrimStart("v")
$msiVersion = & (Join-Path $PSScriptRoot "convert_to_msi_version.ps1") `
    -SemVer $Version
$outputPath = Join-Path $outputRoot "usque-v$displayVersion-windows-$Variant.msi"
$intermediatePath = Join-Path $outputRoot "wix-$Variant"
New-Item -ItemType Directory -Path $intermediatePath -Force | Out-Null

Push-Location $repositoryRoot
try {
    & dotnet tool restore
    if ($LASTEXITCODE -ne 0) {
        throw "dotnet tool restore failed with exit code $LASTEXITCODE."
    }

    & dotnet tool run wix -- extension add $uiExtension
    if ($LASTEXITCODE -ne 0) {
        throw "WiX UI extension restore failed with exit code $LASTEXITCODE."
    }

    $architecture = if ($Variant -eq "arm64") { "arm64" } else { "x64" }
    & dotnet tool run wix -- build `
        -arch $architecture `
        -ext $uiExtension `
        -bindpath "app=$appRoot" `
        -define "DisplayVersion=$displayVersion" `
        -define "MsiVersion=$msiVersion" `
        -define "Variant=$Variant" `
        -define "SignerSha256=$normalizedSigner" `
        -define "IconPath=$iconPath" `
        -define "LicensePath=$licensePath" `
        -defaultcompressionlevel high `
        -intermediateFolder $intermediatePath `
        -pdbtype none `
        -out $outputPath `
        $sourcePath `
        $uiSourcePath
    if ($LASTEXITCODE -ne 0) {
        throw "WiX build failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}

if (-not (Test-Path -LiteralPath $outputPath -PathType Leaf)) {
    throw "WiX did not produce the expected MSI: $outputPath"
}

& (Join-Path $PSScriptRoot "verify_windows_msi.ps1") `
    -MsiPath $outputPath `
    -Variant $Variant `
    -ExpectedMsiVersion $msiVersion `
    -ExpectedDisplayVersion $displayVersion `
    -SignerSha256 $normalizedSigner
if ($LASTEXITCODE -ne 0) {
    throw "MSI table contract verification failed with exit code $LASTEXITCODE."
}

# ICE61 rejects the deliberately enabled equal-version major upgrade used to
# replace the same validation product. Every other standard ICE remains enabled.
& dotnet tool run wix -- msi validate -sice ICE61 $outputPath
if ($LASTEXITCODE -ne 0) {
    throw "MSI ICE validation failed with exit code $LASTEXITCODE."
}

Write-Output $outputPath
