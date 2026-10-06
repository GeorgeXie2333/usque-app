# Shared certificate fingerprint calculation; no store or private-key access.
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

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
