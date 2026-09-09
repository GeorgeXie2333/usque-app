[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Embed trusted repository code, not an installation path or a mutable script
# file. This also avoids shell interpolation of custom paths containing &, %, etc.
$source = Get-Content -LiteralPath (Join-Path $PSScriptRoot "windows_quiet_uninstall.ps1") -Raw
$encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($source))
$command = '"[System64Folder]WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand ' + $encoded
if ($command.Length -ge 32000) {
    throw "The quiet uninstall command exceeds the supported Windows command-line length."
}
Write-Output $command
