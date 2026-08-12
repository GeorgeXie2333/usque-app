[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$Path,

    [Parameter(Mandatory = $true)]
    [ValidatePattern("^[0-9A-Fa-f]{64}$")]
    [string]$SignerSha256,

    [switch]$AllowPinnedUntrustedRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-CertificateSha256 {
    param(
        [Parameter(Mandatory = $true)]
        [Security.Cryptography.X509Certificates.X509Certificate2]$Certificate
    )

    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        return (
            $sha.ComputeHash($Certificate.GetRawCertData()) |
                ForEach-Object { $_.ToString("X2") }
        ) -join ""
    } finally {
        $sha.Dispose()
    }
}

$resolvedPath = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path
if (-not (Test-Path -LiteralPath $resolvedPath -PathType Leaf)) {
    throw "Authenticode target is not a file: $resolvedPath"
}

$signature = Get-AuthenticodeSignature -LiteralPath $resolvedPath
if ($null -eq $signature.SignerCertificate) {
    throw "No signer certificate was returned for $resolvedPath."
}

$valid = $signature.Status -eq [System.Management.Automation.SignatureStatus]::Valid
$pinnedSelfSigned =
$AllowPinnedUntrustedRoot -and
$signature.Status -eq [System.Management.Automation.SignatureStatus]::UnknownError -and
$signature.SignerCertificate.Subject -eq $signature.SignerCertificate.Issuer
if (-not $valid -and -not $pinnedSelfSigned) {
    throw "Authenticode verification failed for $resolvedPath ($($signature.Status))."
}

$expectedSigner = $SignerSha256.ToUpperInvariant()
$actualSigner = Get-CertificateSha256 -Certificate $signature.SignerCertificate
if (-not [StringComparer]::OrdinalIgnoreCase.Equals($actualSigner, $expectedSigner)) {
    throw "Unexpected signer for $resolvedPath. Expected $expectedSigner, got $actualSigner."
}

Write-Output $resolvedPath
