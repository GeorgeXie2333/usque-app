# Exercise the actual workflow scripts with inert certificate/provider doubles.
# Run in a separate PowerShell process; no certificate store or real key is used.
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$workflowPath = Join-Path $PSScriptRoot "../.github/workflows/release.yml"
$workflow = [IO.File]::ReadAllText($workflowPath)
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) "UsqueSigningTest-$([guid]::NewGuid().ToString('N'))"
[IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
$script:FixturePfx = Join-Path $fixtureRoot "usque-signing.pfx"
$env:RUNNER_TEMP = $fixtureRoot
$env:GITHUB_ENV = Join-Path $fixtureRoot "github-env.txt"
$env:WINDOWS_SIGNING_PFX_BASE64 = [Convert]::ToBase64String([byte[]](1, 2, 3))
$env:WINDOWS_SIGNING_PFX_PASSWORD = "inert-test-password"
$script:Thumbprint = "A" * 40
$script:CertificatePath = "Cert:\CurrentUser\My\$script:Thumbprint"
$script:Certificate = [pscustomobject]@{ HasPrivateKey = $true; Thumbprint = $script:Thumbprint }
$script:Certificate | Add-Member -MemberType ScriptMethod -Name GetRawCertData -Value { return [byte[]](1, 2, 3) }
$digest = [Security.Cryptography.SHA256]::Create()
try {
    $fingerprint = [BitConverter]::ToString($digest.ComputeHash([byte[]](1, 2, 3))).Replace("-", "")
}
finally { $digest.Dispose() }

function Assert-SigningTest {
    param([bool]$Condition, [string]$Description)
    if (-not $Condition) { throw "Signing cleanup regression: $Description" }
}

function Get-WorkflowScript {
    param([string]$Name)
    $pattern = '(?ms)^      - name: ' + [regex]::Escape($Name)
    $pattern += '\r?\n(?:(?!^      - name: ).)*?^        run: \|\r?\n(?<body>(?:^          [^\r\n]*(?:\r?\n|$)|^\r?\n)+)'
    $match = [regex]::Match($workflow, $pattern)
    Assert-SigningTest $match.Success "workflow step missing: $Name"
    $source = [regex]::Replace($match.Groups['body'].Value, '(?m)^ {10}', '')
    $block = [scriptblock]::Create($source)
    # Fail closed if a future edit adds a command not covered by these doubles.
    $allowed = @(
        "Join-Path", "ConvertTo-SecureString", "Import-PfxCertificate", "Where-Object",
        "Select-Object", "Sort-Object", "ForEach-Object", "Get-ChildItem", "Out-File", "Test-Path", "Remove-Item"
    )
    $commands = $block.Ast.FindAll({ param($node) $node -is [Management.Automation.Language.CommandAst] }, $true)
    foreach ($command in $commands) {
        Assert-SigningTest ($command.GetCommandName() -in $allowed) "unmocked workflow command: $($command.Extent.Text)"
    }
    return $block
}

function Import-PfxCertificate {
    param([string]$FilePath, [string]$CertStoreLocation, [Security.SecureString]$Password, [switch]$Exportable)
    Assert-SigningTest ($FilePath -eq $script:FixturePfx) "unexpected PFX path"
    Assert-SigningTest ($CertStoreLocation -eq 'Cert:\CurrentUser\My') "unexpected certificate store"
    Assert-SigningTest ($null -ne $Password -and -not $Exportable) "import policy changed"
    if ($script:Scenario -eq "import-failure") { throw "inert import failure" }
    $script:CertificatePresent = $true
    $script:PrivateKeyPresent = $true
    return $script:Certificate
}

function Get-ChildItem {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSAvoidOverwritingBuiltInCmdlets", "", Justification = "The workflow must enumerate only an inert SignTool double.")]
    [CmdletBinding()]
    param([string]$Path)
    Assert-SigningTest ($Path -like '*\Windows Kits\10\bin\*\signtool.exe') "unexpected file enumeration"
    if ($script:Scenario -ne "missing-signtool") {
        return [pscustomobject]@{ FullName = "inert-signtool.exe" }
    }
}

function Test-Path {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSAvoidOverwritingBuiltInCmdlets", "", Justification = "The workflow must query only the in-memory certificate and inert PFX.")]
    [CmdletBinding()]
    param([string]$LiteralPath)
    if ($LiteralPath -eq $script:CertificatePath) { return $script:CertificatePresent }
    Assert-SigningTest ($LiteralPath -eq $script:FixturePfx) "unexpected path lookup"
    return [IO.File]::Exists($LiteralPath)
}

function Remove-Item {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSAvoidOverwritingBuiltInCmdlets", "", Justification = "Intercept workflow deletion so it never reaches the real certificate provider.")]
    [CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = "Low")]
    param([string]$LiteralPath, [switch]$DeleteKey, [switch]$Force)
    Assert-SigningTest $Force.IsPresent "cleanup must remove its owned fixture"
    if (-not $PSCmdlet.ShouldProcess($LiteralPath, "Remove inert fixture")) { return }
    if ($LiteralPath -eq $script:CertificatePath) {
        if ($script:Scenario -eq "cleanup-failure") { throw "inert cleanup failure" }
        $script:CertificatePresent = $false
        if ($DeleteKey) { $script:PrivateKeyPresent = $false }
        return
    }
    Assert-SigningTest ($LiteralPath -eq $script:FixturePfx) "unexpected removal path"
    [IO.File]::Delete($LiteralPath)
}

try {
    $import = Get-WorkflowScript "Import protected signing identity"
    $cleanup = Get-WorkflowScript "Remove signing identity from runner"
    foreach ($scenario in @("success", "wrong-fingerprint", "missing-signtool", "import-failure", "missing-secret", "cleanup-failure")) {
        $script:Scenario = $scenario
        $script:CertificatePresent = $false
        $script:PrivateKeyPresent = $false
        $env:USQUE_CERT_THUMBPRINT = $null
        $env:USQUE_PFX_PATH = $null
        $env:USQUE_SIGNTOOL = $null
        $env:WINDOWS_SIGNER_SHA256 = if ($scenario -eq "wrong-fingerprint") { "0" * 64 } else { $fingerprint }
        $env:WINDOWS_SIGNING_PFX_PASSWORD = if ($scenario -eq "missing-secret") { "" } else { "inert-test-password" }
        [IO.File]::WriteAllText($env:GITHUB_ENV, "")
        $importError = $null
        try { & $import } catch { $importError = $_.Exception.Message }
        $expectedImportError = switch ($scenario) {
            "wrong-fingerprint" { "PFX signer does not match protected WINDOWS_SIGNER_SHA256." }
            "missing-signtool" { "SignTool was not found." }
            "import-failure" { "inert import failure" }
            "missing-secret" { "Protected Windows signing secrets are missing." }
            default { $null }
        }
        Assert-SigningTest ($importError -eq $expectedImportError) "$scenario import error changed: $importError"
        # Model the runner's GITHUB_ENV transfer even when the import step fails.
        foreach ($line in [IO.File]::ReadAllLines($env:GITHUB_ENV)) {
            if ([string]::IsNullOrWhiteSpace($line)) { continue }
            $name, $value = $line.Split('=', 2)
            Assert-SigningTest ($name -in @("USQUE_CERT_THUMBPRINT", "USQUE_PFX_PATH", "USQUE_SIGNTOOL")) "unexpected environment output"
            [Environment]::SetEnvironmentVariable($name, $value, "Process")
        }
        $cleanupError = $null
        try { & $cleanup } catch { $cleanupError = $_.Exception.Message }
        Assert-SigningTest (-not [IO.File]::Exists($script:FixturePfx)) "$scenario retained the PFX"
        if ($scenario -eq "cleanup-failure") {
            Assert-SigningTest ($cleanupError -eq "inert cleanup failure") "cleanup error became success"
        }
        else {
            Assert-SigningTest ($null -eq $cleanupError) "$scenario cleanup failed: $cleanupError"
            Assert-SigningTest (-not $script:CertificatePresent) "$scenario retained the certificate"
            Assert-SigningTest (-not $script:PrivateKeyPresent) "$scenario retained the private key"
            & $cleanup # A repeated always-step cleanup must be harmless.
        }
        Write-Output "WINDOWS_SIGNING_CLEANUP_OK=$scenario"
    }
}
finally {
    # Delete only the two known inert files and an empty fixture directory.
    [IO.File]::Delete($script:FixturePfx)
    [IO.File]::Delete($env:GITHUB_ENV)
    [IO.Directory]::Delete($fixtureRoot, $false)
}
