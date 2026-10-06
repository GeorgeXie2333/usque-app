# Test the real verifier with public certificates and an inert signature double.
# Never signs a file, accesses a certificate store, or loads a private key.
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "windows_certificate_hash.ps1")

function Assert-AuthenticodeTest {
    param([bool]$Condition, [string]$Description)
    if (-not $Condition) { throw "Authenticode regression: $Description" }
}

function Read-PublicCertificate {
    param([string]$Name)
    $path = Join-Path $PSScriptRoot "../crates/usque-openvpn/tests/fixtures/$Name.crt"
    $pem = [IO.File]::ReadAllText($path)
    $match = [regex]::Match($pem, '(?s)-----BEGIN CERTIFICATE-----(.*?)-----END CERTIFICATE-----')
    $der = [Convert]::FromBase64String($match.Groups[1].Value)
    return [Security.Cryptography.X509Certificates.X509Certificate2]::new($der)
}

function Get-AuthenticodeSignature {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSAvoidOverwritingBuiltInCmdlets", "", Justification = "Use only an inert signature result, never the Windows signing provider.")]
    [CmdletBinding()]
    param([string]$LiteralPath)
    Assert-AuthenticodeTest ($LiteralPath -eq $signatureFixture.Path) "unexpected signature target"
    $signatureFixture.Reads++
    return $signatureFixture.Signature
}

function Assert-Rejected {
    param([scriptblock]$Action, [string]$MessagePattern)
    $rejected = $false
    try { & $Action | Out-Null }
    catch {
        if ($_.Exception.Message -notlike $MessagePattern) { throw }
        $rejected = $true
    }
    Assert-AuthenticodeTest $rejected "expected rejection: $MessagePattern"
}

$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) "UsqueAuthenticodeTest-$([guid]::NewGuid().ToString('N'))"
[IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
$script:FixturePath = Join-Path $fixtureRoot "inert.exe"
[IO.File]::WriteAllText($script:FixturePath, "inert fixture, not executable")
$wintunFixture = Join-Path $fixtureRoot "wintun.dll"
[IO.File]::WriteAllText($wintunFixture, "inert Wintun exclusion fixture, not executable")
$signatureFixture = [pscustomobject]@{ Path = $script:FixturePath; Reads = 0; Signature = $null }
$verify = Join-Path $PSScriptRoot "verify_windows_authenticode.ps1"
$ca = $null
$client = $null
try {
    $ca = Read-PublicCertificate "ca"
    $client = Read-PublicCertificate "client"
    Assert-AuthenticodeTest (-not $ca.HasPrivateKey -and -not $client.HasPrivateKey) "public fixture has a private key"
    Assert-AuthenticodeTest ($ca.Subject -eq $ca.Issuer -and $client.Subject -ne $client.Issuer) "fixture issuer relationships changed"
    $caPin = Get-CertificateSha256 -Certificate $ca
    Assert-AuthenticodeTest ($caPin -ceq "72F7DA6247EE0FEBE45981FF7A643AC82A0777A1491AE3EA4892576CB9F58FEE") "certificate DER fingerprint changed"
    $clientPin = Get-CertificateSha256 -Certificate $client

    $script:Signature = [pscustomobject]@{ Status = [Management.Automation.SignatureStatus]::Valid; SignerCertificate = $client }
    $signatureFixture.Signature = $script:Signature
    $output = @(& $verify -Path $script:FixturePath -SignerSha256 $clientPin.ToLowerInvariant())
    Assert-AuthenticodeTest ($output.Count -eq 1 -and $output[0] -eq $script:FixturePath) "successful verifier output changed"
    Assert-Rejected { & $verify -Path $script:FixturePath -SignerSha256 ("0" * 64) } "Unexpected signer*"

    $script:Signature.SignerCertificate = $null
    Assert-Rejected { & $verify -Path $script:FixturePath -SignerSha256 $caPin } "No signer certificate was returned*"
    $script:Signature.Status = [Management.Automation.SignatureStatus]::UnknownError
    Assert-Rejected { & $verify -Path $script:FixturePath -SignerSha256 $caPin -AllowPinnedUntrustedRoot } "No signer certificate was returned*"

    $script:Signature.SignerCertificate = $ca
    Assert-Rejected { & $verify -Path $script:FixturePath -SignerSha256 $caPin } "Authenticode verification failed*"
    $output = @(& $verify -Path $script:FixturePath -SignerSha256 $caPin -AllowPinnedUntrustedRoot)
    Assert-AuthenticodeTest ($output.Count -eq 1 -and $output[0] -eq $script:FixturePath) "pinned self-signed result changed"
    Assert-Rejected { & $verify -Path $script:FixturePath -SignerSha256 ("0" * 64) -AllowPinnedUntrustedRoot } "Unexpected signer*"

    $script:Signature.SignerCertificate = $client
    Assert-Rejected { & $verify -Path $script:FixturePath -SignerSha256 $clientPin -AllowPinnedUntrustedRoot } "Authenticode verification failed*"
    $script:Signature.SignerCertificate = $ca
    foreach ($status in @("HashMismatch", "NotSigned", "NotTrusted", "NotSupportedFileFormat", "Incompatible")) {
        $script:Signature.Status = [Management.Automation.SignatureStatus]::$status
        Assert-Rejected { & $verify -Path $script:FixturePath -SignerSha256 $caPin -AllowPinnedUntrustedRoot } "Authenticode verification failed*"
    }

    $reads = $signatureFixture.Reads
    Assert-Rejected { & $verify -Path $fixtureRoot -SignerSha256 $caPin } "Authenticode target is not a file*"
    Assert-AuthenticodeTest ($signatureFixture.Reads -eq $reads) "invalid file reached the signature provider"

    # Load only the real payload-enumeration function, not the packaging script.
    $tokens = $null
    $parseErrors = $null
    $buildAst = [Management.Automation.Language.Parser]::ParseFile(
        (Join-Path $PSScriptRoot "build_windows_msi.ps1"), [ref]$tokens, [ref]$parseErrors
    )
    Assert-AuthenticodeTest ($parseErrors.Count -eq 0) "MSI script failed to parse"
    $enumerator = @($buildAst.FindAll({
                param($node)
                $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -eq "Assert-ReleaseSignature"
            }, $true))
    Assert-AuthenticodeTest ($enumerator.Count -eq 1) "payload verifier function is missing"
    ${function:Assert-ReleaseSignature} = $enumerator[0].Body.GetScriptBlock()
    $script:Signature.Status = [Management.Automation.SignatureStatus]::Valid
    $script:Signature.SignerCertificate = $client
    $reads = $signatureFixture.Reads
    $output = @(Assert-ReleaseSignature -Root $fixtureRoot -ExpectedSigner $clientPin)
    Assert-AuthenticodeTest ($output.Count -eq 0) "payload verifier leaked success output"
    Assert-AuthenticodeTest ($signatureFixture.Reads -eq $reads + 1) "official Wintun exclusion changed"
    Assert-Rejected { Assert-ReleaseSignature -Root $fixtureRoot -ExpectedSigner ("0" * 64) } "Unexpected signer*"
    $script:Signature.Status = [Management.Automation.SignatureStatus]::UnknownError
    $script:Signature.SignerCertificate = $ca
    Assert-Rejected { Assert-ReleaseSignature -Root $fixtureRoot -ExpectedSigner $caPin } "Authenticode verification failed*"
    Assert-ReleaseSignature -Root $fixtureRoot -ExpectedSigner $caPin -AllowUntrustedRoot
    Write-Output "WINDOWS_AUTHENTICODE_OK"
}
finally {
    if ($null -ne $ca) { $ca.Dispose() }
    if ($null -ne $client) { $client.Dispose() }
    [IO.File]::Delete($script:FixturePath)
    [IO.File]::Delete($wintunFixture)
    [IO.Directory]::Delete($fixtureRoot, $false)
}
