# Deterministic quiet-uninstall orchestration tests; never runs an installer,
# touches a product registry key, or invokes a real uninstall helper.
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "windows_quiet_uninstall.ps1")
$waitForChild = ${function:Invoke-UsqueQuietChild}
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) "UsqueQuietTest-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
$script:FixtureCopy = Join-Path $fixtureRoot "usque-uninstall.exe"
$script:Calls = [Collections.Generic.List[string]]::new()
$script:StageCode = 0
$script:VerifyCode = 0
$script:WorkerCode = 0
$script:WorkerFinished = $false

function Assert-QuietTest {
    param([bool]$Condition, [string]$Description)
    if (-not $Condition) { throw "Quiet uninstall regression: $Description" }
}

function Get-UsqueInstalledHelperPath {
    # Metacharacters must remain a filename, not be interpreted as shell code.
    return 'C:\inert & % fixture\usque-uninstall.exe'
}

function Get-UsqueQuietCopyPath {
    return $script:FixtureCopy
}

function Invoke-UsqueQuietChild {
    param([string]$FilePath, [string]$Arguments)
    $script:Calls.Add($Arguments)
    if ($Arguments -like "--stage-quiet=*") {
        Assert-QuietTest ($FilePath -eq (Get-UsqueInstalledHelperPath)) "stage path changed"
        if ($script:StageCode -eq 0) {
            New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($script:FixtureCopy)) -Force | Out-Null
            [IO.File]::WriteAllText($script:FixtureCopy, "inert fixture, not executable")
        }
        return $script:StageCode
    }
    if ($Arguments -like "--verify-quiet=*") {
        Assert-QuietTest ($FilePath -eq (Get-UsqueInstalledHelperPath)) "verifier must use installed image"
        Assert-QuietTest ($script:Calls.Count -eq 2) "verification ran before staging finished"
    }
    else {
        Assert-QuietTest ($Arguments -eq "--quiet") "worker must not wait for a live parent"
        Assert-QuietTest ($FilePath -eq $script:FixtureCopy) "worker must run outside the install directory"
        Assert-QuietTest ($script:Calls.Count -eq 3) "worker ran before verifier finished"
    }
    $writer = $null
    $writeBlocked = $false
    try {
        $writer = [IO.File]::Open($script:FixtureCopy, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::ReadWrite)
    }
    catch [IO.IOException] {
        $writeBlocked = $true
    }
    finally {
        if ($null -ne $writer) { $writer.Dispose() }
    }
    Assert-QuietTest $writeBlocked "copy was mutable during verification/worker execution"
    if ($Arguments -like "--verify-quiet=*") { return $script:VerifyCode }
    $script:WorkerFinished = $true
    return $script:WorkerCode
}

try {
    foreach ($code in @(0, 1, 1602, 1603, 1641, 3010)) {
        $script:Calls.Clear()
        $script:WorkerFinished = $false
        $script:WorkerCode = $code
        $actual = Invoke-UsqueQuietUninstall
        Assert-QuietTest ($actual -eq $code) "worker exit code $code was lost"
        Assert-QuietTest $script:WorkerFinished "launcher returned before worker finished"
        Assert-QuietTest (-not (Test-Path -LiteralPath $script:FixtureCopy)) "staged copy was left behind"
    }

    $script:StageCode = 5
    $script:Calls.Clear()
    Assert-QuietTest ((Invoke-UsqueQuietUninstall) -eq 5) "staging failure became success"
    Assert-QuietTest ($script:Calls.Count -eq 1) "failed staging started another process"

    $script:StageCode = 0
    $script:VerifyCode = 7
    $script:Calls.Clear()
    Assert-QuietTest ((Invoke-UsqueQuietUninstall) -eq 7) "signature failure became success"
    Assert-QuietTest ($script:Calls.Count -eq 2) "unverified worker was started"

    # Exercise the actual wait/exit-code wrapper with an inert system process.
    # Windows PowerShell 5.1 is the installed quiet launcher on every target.
    $systemPowerShell = Join-Path ([Environment]::SystemDirectory) "WindowsPowerShell\v1.0\powershell.exe"
    $actual = & $waitForChild -FilePath $systemPowerShell -Arguments '-NoProfile -NonInteractive -Command "Start-Sleep -Milliseconds 100; exit 3010"'
    Assert-QuietTest ($actual -eq 3010) "real child process exit code was lost"

    $command = & (Join-Path $PSScriptRoot "get_windows_quiet_uninstall_command.ps1")
    Assert-QuietTest ($command.StartsWith('"[System64Folder]WindowsPowerShell\v1.0\powershell.exe" ')) "64-bit MSI must use the native system host"
    $encoded = ($command -split ' -EncodedCommand ', 2)[1]
    $decoded = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($encoded))
    $source = Get-Content -LiteralPath (Join-Path $PSScriptRoot "windows_quiet_uninstall.ps1") -Raw
    Assert-QuietTest ($decoded -ceq $source) "registered launcher differs from tested source"
    Assert-QuietTest ($command -notmatch 'ExecutionPolicy|\[INSTALLFOLDER\]') "launcher bypasses policy or embeds installation paths"

    # The release builder must preserve a caller-supplied ProductCode across
    # the English MSI and all language transforms, independently of cab cache.
    $tokens = $null
    $parseErrors = $null
    $buildAst = [Management.Automation.Language.Parser]::ParseFile(
        (Join-Path $PSScriptRoot "build_windows_msi.ps1"), [ref]$tokens, [ref]$parseErrors
    )
    $assignment = $buildAst.Find({
            param($node)
            $node -is [Management.Automation.Language.AssignmentStatementAst] -and
            $node.Left.Extent.Text -eq '$normalizedProductCode'
        }, $true)
    $selector = [scriptblock]::Create($assignment.Extent.Text)
    $selected = & {
        param($ProductCode, $SelectCode)
        if ([string]::IsNullOrWhiteSpace($ProductCode)) { throw "The explicit-code fixture is empty." }
        . $SelectCode
        $normalizedProductCode
    } "11111111-1111-1111-1111-111111111111" $selector
    Assert-QuietTest ($selected -eq "11111111-1111-1111-1111-111111111111") "explicit MSI ProductCode was discarded"
}
finally {
    # Only a known inert file and an empty test directory can be removed.
    if (Test-Path -LiteralPath $script:FixtureCopy) { [IO.File]::Delete($script:FixtureCopy) }
    if (Test-Path -LiteralPath $fixtureRoot) { [IO.Directory]::Delete($fixtureRoot, $false) }
}

Write-Output "WINDOWS_QUIET_UNINSTALL_TESTS_OK"
