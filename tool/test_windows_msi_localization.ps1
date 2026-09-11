# Regress malformed localized MSI format strings using data-only validation.
# Only a temporary package copy is modified; no installer action is executed.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$MsiPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$sourceMsi = (Resolve-Path -LiteralPath $MsiPath -ErrorAction Stop).Path
$temporaryBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
$testRoot = Join-Path $temporaryBase ("usque-msi-localization-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $testRoot -ErrorAction Stop | Out-Null
$installer = $null
$database = $null
$view = $null
Push-Location $repositoryRoot
try {
    & dotnet tool run wix -- msi validate -sice ICE61 $sourceMsi
    if ($LASTEXITCODE -ne 0) {
        throw "The localization regression requires a valid input MSI."
    }

    $copy = Join-Path $testRoot "invalid-localization.msi"
    Copy-Item -LiteralPath $sourceMsi -Destination $copy
    $installer = New-Object -ComObject WindowsInstaller.Installer
    try {
        $database = $installer.GetType().InvokeMember(
            "OpenDatabase", [Reflection.BindingFlags]::InvokeMethod,
            $null, $installer, @([string]$copy, 1)
        )
        # Square brackets invoke MSI formatting. This is not a valid property
        # identifier and must be rejected instead of reaching an installer UI.
        # Construct the Japanese Remove label without non-ASCII source bytes,
        # keeping the fixture independent of the PowerShell host's encoding.
        $invalidText = '[' + [char]0x524A + [char]0x9664 + ']'
        $sql = 'UPDATE `Control` SET `Text`=''{0}'' WHERE `Dialog_`=''UsqueRepairUnsupportedDlg'' AND `Control`=''Description''' -f $invalidText
        $view = $database.GetType().InvokeMember(
            "OpenView", [Reflection.BindingFlags]::InvokeMethod,
            $null, $database, @($sql)
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
            $view = $null
        }
        if ($null -ne $database) {
            [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($database)
            $database = $null
        }
    }

    $diagnostics = @(& dotnet tool run wix -- msi validate -sice ICE61 $copy 2>&1)
    $validationExit = $LASTEXITCODE
    $diagnosticText = $diagnostics -join [Environment]::NewLine
    if (
        $validationExit -ne 204 -or
        $diagnosticText -notmatch 'ICE03: Invalid format string' -or
        $diagnosticText -notmatch 'UsqueRepairUnsupportedDlg\.Description'
    ) {
        throw "Expected ICE03 for the malformed localized string, got exit $validationExit`: $diagnosticText"
    }
    # Recheck the untouched source and leave a successful native exit status
    # after the deliberately failing negative test.
    & dotnet tool run wix -- msi validate -sice ICE61 $sourceMsi
    if ($LASTEXITCODE -ne 0) {
        throw "The input MSI no longer passes validation after the negative test."
    }
    Write-Output "MSI_LOCALIZATION_REJECTED=ICE03/UsqueRepairUnsupportedDlg.Description"
}
finally {
    if ($null -ne $installer) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
    }
    Pop-Location
    $resolvedTestRoot = (Resolve-Path -LiteralPath $testRoot -ErrorAction Stop).Path
    if (-not $resolvedTestRoot.StartsWith($temporaryBase, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to remove a localization fixture outside the temporary directory."
    }
    Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
}
