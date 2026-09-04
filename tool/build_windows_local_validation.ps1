[CmdletBinding()]
param(
    [ValidateSet("x64-v1", "x64-v2", "arm64")]
    [string]$Variant = "x64-v2",
    [string]$Version = "0.2.4",
    [string]$BuildLabel = "local-validation",
    [string]$FlutterReleaseDirectory = "",
    [string]$OutputDirectory = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$architecture = if ($Variant -eq "arm64") {
    @{
        Flutter = "arm64"
        RustTarget = "aarch64-pc-windows-msvc"
        SignTool = "arm64"
        Wintun = "arm64"
    }
}
else {
    @{
        Flutter = "x64"
        RustTarget = "x86_64-pc-windows-msvc"
        SignTool = "x64"
        Wintun = "amd64"
    }
}
$displayVersion = $Version.TrimStart("v")
if ([string]::IsNullOrWhiteSpace($FlutterReleaseDirectory)) {
    $FlutterReleaseDirectory = Join-Path $repositoryRoot "apps/usque_gui/build/windows/$($architecture.Flutter)/runner/Release"
}
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $repositoryRoot "dist/windows"
}
$FlutterReleaseDirectory = (Resolve-Path -LiteralPath $FlutterReleaseDirectory).Path
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path

$stagingRoot = Join-Path $repositoryRoot "build/v$displayVersion-windows-$Variant-local-packaging"
if (Test-Path -LiteralPath $stagingRoot) {
    throw "Refusing to reuse the local signing staging directory: $stagingRoot"
}
$payload = Join-Path $stagingRoot "payload"
$msiOutput = Join-Path $stagingRoot "msi"
New-Item -ItemType Directory -Path $stagingRoot | Out-Null

$certificate = $null
$certificateThumbprint = $null
$certificateSha256 = $null
$finalMsi = Join-Path $OutputDirectory "usque-v$displayVersion-windows-$Variant-$BuildLabel.msi"

function Get-CertificateSha256 {
    param(
        [Parameter(Mandatory = $true)]
        [Security.Cryptography.X509Certificates.X509Certificate2]$Certificate
    )

    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        return (($sha256.ComputeHash($Certificate.GetRawCertData()) |
                    ForEach-Object { $_.ToString("X2") }) -join "")
    }
    finally {
        $sha256.Dispose()
    }
}

function Assert-PinnedLocalSignature {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$ExpectedSigner
    )

    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($null -eq $signature.SignerCertificate) {
        throw "No Authenticode signer was returned for $Path."
    }
    $actualSigner = Get-CertificateSha256 -Certificate $signature.SignerCertificate
    if (-not [StringComparer]::OrdinalIgnoreCase.Equals($actualSigner, $ExpectedSigner)) {
        throw "Unexpected Authenticode signer for $Path."
    }
    $valid = $signature.Status -eq [Management.Automation.SignatureStatus]::Valid
    $pinnedSelfSigned =
    $signature.Status -eq [Management.Automation.SignatureStatus]::UnknownError -and
    $signature.SignerCertificate.Subject -eq $signature.SignerCertificate.Issuer
    if (-not $valid -and -not $pinnedSelfSigned) {
        throw "Local Authenticode verification failed for $Path ($($signature.Status))."
    }
}

try {
    Copy-Item -LiteralPath $FlutterReleaseDirectory -Destination $payload -Recurse
    Copy-Item -LiteralPath (
        Join-Path $repositoryRoot "target/$($architecture.RustTarget)/release/usque-engine.exe"
    ) -Destination $payload
    Copy-Item -LiteralPath (
        Join-Path $repositoryRoot "target/$($architecture.RustTarget)/release/usque-agent.exe"
    ) -Destination $payload
    Copy-Item -LiteralPath (
        Join-Path $repositoryRoot "target/$($architecture.RustTarget)/release/usque-uninstall.exe"
    ) -Destination $payload
    Copy-Item -LiteralPath (
        Join-Path $repositoryRoot "target/$($architecture.RustTarget)/release/usque-update.exe"
    ) -Destination $payload
    Copy-Item -LiteralPath (
        Join-Path $repositoryRoot "third_party/wintun-0.14.1/wintun/bin/$($architecture.Wintun)/wintun.dll"
    ) -Destination $payload

    $pdb = Get-ChildItem -LiteralPath $payload -Recurse -File -Filter "*.pdb" |
        Select-Object -First 1
    if ($null -ne $pdb) {
        throw "Release payload contains a PDB: $($pdb.FullName)"
    }

    $certificate = New-SelfSignedCertificate `
        -Type CodeSigningCert `
        -Subject "CN=Usque v$displayVersion Local Validation" `
        -FriendlyName "Usque v$displayVersion Local Validation" `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -KeyAlgorithm RSA `
        -KeyLength 3072 `
        -HashAlgorithm SHA256 `
        -NotAfter (Get-Date).AddYears(2)
    $certificateThumbprint = $certificate.Thumbprint
    $certificateSha256 = Get-CertificateSha256 -Certificate $certificate

    $cerPath = Join-Path $stagingRoot "usque-v$displayVersion-local.cer"
    Export-Certificate -Cert $certificate -FilePath $cerPath | Out-Null
    # Trust the exact leaf for this local validation run. TrustedPeople avoids
    # adding a development identity as a root CA while still allowing
    # Authenticode chain validation for the temporary self-signed signer.
    Import-Certificate -FilePath $cerPath -CertStoreLocation "Cert:\CurrentUser\TrustedPeople" |
        Out-Null
    Import-Certificate `
        -FilePath $cerPath `
        -CertStoreLocation "Cert:\CurrentUser\TrustedPublisher" |
        Out-Null

    $signTool = Get-ChildItem `
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\$($architecture.SignTool)\signtool.exe" |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if ($null -eq $signTool) {
        throw "SignTool was not found."
    }

    $officialWintun = [IO.Path]::GetFullPath((Join-Path $payload "wintun.dll"))
    $binaries = @(Get-ChildItem -LiteralPath $payload -File -Recurse |
            Where-Object { $_.Extension -in ".exe", ".dll" })
    foreach ($binary in $binaries) {
        if ([StringComparer]::OrdinalIgnoreCase.Equals(
                [IO.Path]::GetFullPath($binary.FullName),
                $officialWintun
            )) {
            continue
        }
        & $signTool.FullName sign `
            /sha1 $certificateThumbprint `
            /s My `
            /fd SHA256 `
            $binary.FullName
        if ($LASTEXITCODE -ne 0) {
            throw "Signing failed for $($binary.FullName)."
        }
        Assert-PinnedLocalSignature `
            -Path $binary.FullName `
            -ExpectedSigner $certificateSha256
    }

    & (Join-Path $PSScriptRoot "build_windows_msi.ps1") `
        -Variant $Variant `
        -AppDirectory $payload `
        -OutputDirectory $msiOutput `
        -SignerSha256 $certificateSha256 `
        -Version $Version `
        -AllowPinnedUntrustedRoot
    if ($LASTEXITCODE -ne 0) {
        throw "MSI construction failed."
    }
    $unsignedName = "usque-v$displayVersion-windows-$Variant.msi"
    $builtMsi = Join-Path $msiOutput $unsignedName
    if (-not (Test-Path -LiteralPath $builtMsi -PathType Leaf)) {
        throw "Expected MSI was not produced: $builtMsi"
    }
    & $signTool.FullName sign `
        /sha1 $certificateThumbprint `
        /s My `
        /fd SHA256 `
        $builtMsi
    if ($LASTEXITCODE -ne 0) {
        throw "MSI signing failed."
    }
    Assert-PinnedLocalSignature `
        -Path $builtMsi `
        -ExpectedSigner $certificateSha256

    Copy-Item -LiteralPath $builtMsi -Destination $finalMsi -Force
    [PSCustomObject]@{
        MsiPath = $finalMsi
        SignerSha256 = $certificateSha256
        SignerThumbprint = $certificateThumbprint
    }
}
finally {
    if (-not [string]::IsNullOrWhiteSpace($certificateThumbprint)) {
        foreach ($store in @("My", "TrustedPeople", "TrustedPublisher")) {
            $certificatePath = "Cert:\CurrentUser\$store\$certificateThumbprint"
            if (Test-Path -LiteralPath $certificatePath) {
                Remove-Item -LiteralPath $certificatePath -Force
            }
        }
    }
    if (Test-Path -LiteralPath $stagingRoot) {
        Remove-Item -LiteralPath $stagingRoot -Recurse -Force
    }
}
