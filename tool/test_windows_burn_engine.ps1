[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$BundlePath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Assert-Rejected {
    param([scriptblock]$Action, [string]$MessagePattern)
    $rejected = $false
    try { & $Action | Out-Null }
    catch {
        if ($_.Exception.Message -notlike $MessagePattern) { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw "Expected rejection: $MessagePattern" }
}

function Set-TestSignature {
    [CmdletBinding(SupportsShouldProcess)]
    param([string]$File, [Security.Cryptography.X509Certificates.X509Certificate2]$Certificate)
    if ($PSCmdlet.ShouldProcess($File, "Apply and verify temporary test signature")) {
        Set-AuthenticodeSignature -FilePath $File -Certificate $Certificate -HashAlgorithm SHA256 | Out-Null
        & (Join-Path $PSScriptRoot "verify_windows_authenticode.ps1") `
            -Path $File `
            -SignerSha256 $Certificate.GetCertHashString([Security.Cryptography.HashAlgorithmName]::SHA256) `
            -AllowPinnedUntrustedRoot | Out-Null
    }
}

$sourceBundle = (Resolve-Path -LiteralPath $BundlePath).Path
$sourceHash = (Get-FileHash -LiteralPath $sourceBundle -Algorithm SHA256).Hash
$temporaryRoot = Join-Path ([IO.Path]::GetTempPath()) "usque-burn-test-$([guid]::NewGuid().ToString('N'))"
$safeTempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
$temporaryRoot = [IO.Path]::GetFullPath($temporaryRoot)
if (-not $temporaryRoot.StartsWith($safeTempRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Invalid Burn test temporary directory."
}
New-Item -ItemType Directory -Path $temporaryRoot | Out-Null
$certificate = $null
$extract = Join-Path $PSScriptRoot "extract_windows_burn_engine.ps1"
$verify = Join-Path $PSScriptRoot "verify_windows_authenticode.ps1"
try {
    # A fresh, untrusted test identity only. Never export its key, import it
    # into a trust store, sign Wintun, execute the bundle, or install its MSI.
    $certificate = New-SelfSignedCertificate `
        -Type CodeSigningCert `
        -Subject "CN=Usque inert Burn test $([guid]::NewGuid().ToString('N'))" `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -KeyAlgorithm RSA `
        -KeyLength 2048 `
        -HashAlgorithm SHA256 `
        -KeyExportPolicy NonExportable `
        -NotAfter ([DateTime]::Now.AddDays(1))
    $signer = $certificate.GetCertHashString([Security.Cryptography.HashAlgorithmName]::SHA256)
    $engine = Join-Path $temporaryRoot "engine.exe"
    $signedBundle = Join-Path $temporaryRoot "signed-bundle.exe"
    $rawDetached = Join-Path $temporaryRoot "raw-detached.exe"
    $restored = Join-Path $temporaryRoot "restored.exe"
    & dotnet tool run wix -- burn detach $sourceBundle -engine $engine -intermediateFolder $temporaryRoot
    if ($LASTEXITCODE -ne 0) { throw "Test engine detach failed." }
    Set-TestSignature $engine $certificate
    $engineHash = (Get-FileHash -LiteralPath $engine -Algorithm SHA256).Hash
    & dotnet tool run wix -- burn reattach $sourceBundle -engine $engine -out $signedBundle -intermediateFolder $temporaryRoot
    if ($LASTEXITCODE -ne 0) { throw "Test engine reattach failed." }
    Set-TestSignature $signedBundle $certificate
    $bundleHash = (Get-FileHash -LiteralPath $signedBundle -Algorithm SHA256).Hash

    # Reproduce the former release failure before testing the repaired path.
    & dotnet tool run wix -- burn detach $signedBundle -engine $rawDetached -intermediateFolder $temporaryRoot
    if ($LASTEXITCODE -ne 0) { throw "Signed test bundle detach failed." }
    Assert-Rejected {
        & $verify -Path $rawDetached -SignerSha256 $signer -AllowPinnedUntrustedRoot
    } "No signer certificate was returned*"
    & $extract -BundlePath $signedBundle -OutputPath $restored | Out-Null
    & $verify -Path $restored -SignerSha256 $signer -AllowPinnedUntrustedRoot | Out-Null
    if ((Get-FileHash -LiteralPath $restored -Algorithm SHA256).Hash -cne $engineHash) {
        throw "Restored engine differs from the originally signed engine."
    }
    Write-Output "BURN_SIGNED_ROUND_TRIP_OK"

    Assert-Rejected {
        & $verify -Path $restored -SignerSha256 ("0" * 64) -AllowPinnedUntrustedRoot
    } "Unexpected signer*"
    $tampered = Join-Path $temporaryRoot "tampered-engine.exe"
    Copy-Item -LiteralPath $restored -Destination $tampered
    $file = [IO.File]::Open($tampered, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite)
    try {
        $file.Position = 0x40
        $value = $file.ReadByte()
        $file.Position = 0x40
        $file.WriteByte([byte]($value -bxor 1))
    }
    finally { $file.Dispose() }
    Assert-Rejected {
        & $verify -Path $tampered -SignerSha256 $signer -AllowPinnedUntrustedRoot
    } "Authenticode verification failed*"
    Assert-Rejected {
        & $extract -BundlePath $signedBundle -OutputPath $restored
    } "Burn engine output must not already exist*"
    if ((Get-FileHash -LiteralPath $restored -Algorithm SHA256).Hash -cne $engineHash) {
        throw "Existing extraction output was changed."
    }
    $rejectedOutput = Join-Path $temporaryRoot "must-not-exist.exe"
    Assert-Rejected {
        & $extract -BundlePath $sourceBundle -OutputPath $rejectedOutput
    } "Missing or invalid embedded Burn engine signature bounds*"

    # Mutations affect only fresh fixture copies; each must fail before writing
    # an output. Locate the real section so both x64 and ARM64 layouts are tested.
    $bytes = [IO.File]::ReadAllBytes($signedBundle)
    $peOffset = [BitConverter]::ToUInt32($bytes, 0x3C)
    $sectionTable = $peOffset + 24 + [BitConverter]::ToUInt16($bytes, $peOffset + 20)
    $sectionCount = [BitConverter]::ToUInt16($bytes, $peOffset + 6)
    $burnOffset = $null
    for ($index = 0; $index -lt $sectionCount; $index++) {
        $entry = $sectionTable + 40 * $index
        if ([Text.Encoding]::ASCII.GetString($bytes, $entry, 8) -ceq '.wixburn') {
            $burnOffset = [BitConverter]::ToUInt32($bytes, $entry + 20)
        }
    }
    if ($null -eq $burnOffset) { throw "Fixture has no Burn section." }
    $mutations = @(
        @{ Name = "bad-magic"; Offset = $burnOffset; Value = 0; Error = "Unsupported .wixburn*" },
        @{ Name = "bad-version"; Offset = $burnOffset + 4; Value = 99; Error = "Unsupported .wixburn*" },
        @{ Name = "bad-format"; Offset = $burnOffset + 40; Value = 0; Error = "Unsupported .wixburn*" },
        @{ Name = "missing-signature"; Offset = $burnOffset + 32; Value = 0; Error = "Missing or invalid embedded*" },
        @{ Name = "signature-overflow"; Offset = $burnOffset + 36; Value = [uint32]::MaxValue; Error = "Missing or invalid embedded*" },
        @{ Name = "signature-overlap"; Offset = $burnOffset + 32; Value = 64; Error = "Missing or invalid embedded*" },
        @{ Name = "container-overflow"; Offset = $burnOffset + 44; Value = [uint32]::MaxValue; Error = "Invalid Burn attached-container count*" },
        @{ Name = "payload-truncated"; Offset = $burnOffset + 52; Value = [uint32]::MaxValue; Error = "Truncated Burn attached payload*" },
        @{ Name = "header-truncated"; Offset = 0x3C; Value = [uint32]::MaxValue; Error = "Truncated Burn/PE field*" }
    )
    foreach ($mutation in $mutations) {
        $copy = [byte[]]$bytes.Clone()
        [BitConverter]::GetBytes([uint32]$mutation.Value).CopyTo($copy, [int]$mutation.Offset)
        $malformed = Join-Path $temporaryRoot "$($mutation.Name).exe"
        [IO.File]::WriteAllBytes($malformed, $copy)
        Assert-Rejected {
            & $extract -BundlePath $malformed -OutputPath $rejectedOutput
        } $mutation.Error
        if (Test-Path -LiteralPath $rejectedOutput) { throw "Malformed bundle left an output." }
        Write-Output "BURN_MALFORMED_REJECTED=$($mutation.Name)"
    }
    if (
        (Get-FileHash -LiteralPath $sourceBundle -Algorithm SHA256).Hash -cne $sourceHash -or
        (Get-FileHash -LiteralPath $signedBundle -Algorithm SHA256).Hash -cne $bundleHash
    ) {
        throw "Burn extraction changed an input bundle."
    }
    Write-Output "BURN_ENGINE_SIGNATURE_TESTS_OK"
}
finally {
    try {
        if ($null -ne $certificate) {
            $certificatePath = "Cert:\CurrentUser\My\$($certificate.Thumbprint)"
            Remove-Item -LiteralPath $certificatePath -DeleteKey -Force
            $certificate.Dispose()
        }
    }
    finally {
        $resolvedTemporaryRoot = (Resolve-Path -LiteralPath $temporaryRoot).Path
        if (-not $resolvedTemporaryRoot.StartsWith($safeTempRoot, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove a Burn test directory outside the temporary directory."
        }
        Remove-Item -LiteralPath $resolvedTemporaryRoot -Recurse -Force
    }
}
