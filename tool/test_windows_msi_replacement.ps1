# Negative table-contract tests. Never installs an MSI or executes its actions.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$MsiPath,
    [Parameter(Mandatory = $true)]
    [ValidateSet("x64-v1", "x64-v2", "arm64")]
    [string]$Variant,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedMsiVersion,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedDisplayVersion,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedAgentFileVersion,
    [Parameter(Mandatory = $true)]
    [ValidatePattern("^[0-9A-Fa-f]{64}$")]
    [string]$SignerSha256
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$sourceMsi = (Resolve-Path -LiteralPath $MsiPath -ErrorAction Stop).Path
$verifier = Join-Path $PSScriptRoot "verify_windows_msi.ps1"
$verification = @{
    MsiPath = $sourceMsi
    Variant = $Variant
    ExpectedMsiVersion = $ExpectedMsiVersion
    ExpectedDisplayVersion = $ExpectedDisplayVersion
    ExpectedAgentFileVersion = $ExpectedAgentFileVersion
    SignerSha256 = $SignerSha256
}
& $verifier @verification

$cases = @(
    @{
        Name = "unscoped overwrite mode"
        Sql = 'INSERT INTO `Property` (`Property`,`Value`) VALUES (''REINSTALLMODE'',''amus'')'
        Error = "Payload overwrite mode must be scoped to new-install sequences"
    },
    @{
        Name = "missing interactive overwrite policy"
        Sql = 'DELETE FROM `InstallUISequence` WHERE `Action`=''SetPayloadReinstallMode'''
        Error = "payload overwrite InstallUISequence must have exactly one"
    },
    @{
        Name = "missing execute overwrite policy"
        Sql = 'DELETE FROM `InstallExecuteSequence` WHERE `Action`=''SetPayloadReinstallMode'''
        Error = "payload overwrite InstallExecuteSequence must have exactly one"
    },
    @{
        Name = "weakened enforced overwrite mode"
        Sql = 'UPDATE `CustomAction` SET `Target`=''emus'' WHERE `Action`=''SetPayloadReinstallMode'''
        Error = "payload overwrite enforced mode mismatch"
    },
    @{
        Name = "missing overwrite action"
        Sql = 'DELETE FROM `CustomAction` WHERE `Action`=''SetPayloadReinstallMode'''
        Error = "payload overwrite CustomAction must have exactly one"
    },
    @{
        Name = "overwrite action after file costing"
        Sql = 'UPDATE `InstallExecuteSequence` SET `Sequence`=1001 WHERE `Action`=''SetPayloadReinstallMode'''
        Error = "Payload overwrite policy must be enforced before CostInitialize"
    },
    @{
        Name = "overwrite action also runs on uninstall"
        Sql = 'UPDATE `InstallExecuteSequence` SET `Condition`=''1'' WHERE `Action`=''SetPayloadReinstallMode'''
        Error = "payload overwrite new-install condition mismatch"
    },
    @{
        Name = "repair opt-in"
        Sql = 'INSERT INTO `Property` (`Property`,`Value`) VALUES (''REINSTALL'',''ALL'')'
        Error = "MSI must not enable unsupported repair"
    },
    @{
        Name = "payload preserved by NeverOverwrite"
        Sql = 'UPDATE `Component` SET `Attributes`=384 WHERE `Component`=''UsqueEngineComponent'''
        Error = "must not use NeverOverwrite"
    },
    @{
        Name = "payload outside the private installation directory"
        Sql = 'UPDATE `Component` SET `Directory_`=''ProgramFiles64Folder'' WHERE `Component`=''UsqueEngineComponent'''
        Error = "must remain below INSTALLFOLDER"
    }
)

$temporaryParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
$testRoot = Join-Path $temporaryParent ("usque-msi-replacement-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $testRoot -ErrorAction Stop | Out-Null
$installer = $null
try {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    for ($index = 0; $index -lt $cases.Count; $index++) {
        $case = $cases[$index]
        $copy = Join-Path $testRoot "case-$index.msi"
        Copy-Item -LiteralPath $sourceMsi -Destination $copy
        $database = $null
        $view = $null
        try {
            # Transact only against our fresh copy, never the input package or
            # the Windows Installer product database. No custom action runs.
            $database = $installer.GetType().InvokeMember(
                "OpenDatabase", [Reflection.BindingFlags]::InvokeMethod,
                $null, $installer, @([string]$copy, 1)
            )
            $view = $database.GetType().InvokeMember(
                "OpenView", [Reflection.BindingFlags]::InvokeMethod,
                $null, $database, @([string]$case.Sql)
            )
            $view.GetType().InvokeMember(
                "Execute", [Reflection.BindingFlags]::InvokeMethod,
                $null, $view, $null
            ) | Out-Null
            $database.GetType().InvokeMember(
                "Commit", [Reflection.BindingFlags]::InvokeMethod,
                $null, $database, $null
            ) | Out-Null
        }
        finally {
            if ($null -ne $view) {
                [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($view)
            }
            if ($null -ne $database) {
                [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($database)
            }
        }

        $verification.MsiPath = $copy
        $rejected = $false
        try {
            & $verifier @verification
        }
        catch {
            if (-not $_.Exception.Message.Contains($case.Error)) {
                throw "Unexpected failure for '$($case.Name)': $($_.Exception.Message)"
            }
            $rejected = $true
        }
        if (-not $rejected) {
            throw "MSI verifier accepted '$($case.Name)'."
        }
        Write-Output "MSI_REPLACEMENT_REJECTED=$($case.Name)"
    }
}
finally {
    if ($null -ne $installer) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
    }
    $resolvedTestRoot = (Resolve-Path -LiteralPath $testRoot -ErrorAction Stop).Path
    if (
        (Split-Path -Parent $resolvedTestRoot) -ne $temporaryParent -or
        (Split-Path -Leaf $resolvedTestRoot) -notmatch '^usque-msi-replacement-[0-9a-f]{32}$' -or
        ((Get-Item -LiteralPath $resolvedTestRoot).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0
    ) {
        throw "Unexpected MSI test cleanup target."
    }
    Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
}

Write-Output "MSI_REPLACEMENT_CONTRACT_OK=$Variant/$($cases.Count)"
