[CmdletBinding()]
param(
    [string]$FlutterProject = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($FlutterProject)) {
    $FlutterProject = Join-Path $PSScriptRoot "../apps/usque_gui"
}
$project = (Resolve-Path -LiteralPath $FlutterProject).Path
$metadataPath = Join-Path $project ".flutter-plugins-dependencies"
if (-not (Test-Path -LiteralPath $metadataPath)) {
    throw "Run 'flutter pub get' first; $metadataPath does not exist."
}

$metadata = Get-Content -LiteralPath $metadataPath -Raw | ConvertFrom-Json
$ephemeral = Join-Path $project "windows/flutter/ephemeral"
$junctionRoot = Join-Path $ephemeral ".plugin_symlinks"
New-Item -ItemType Directory -Path $junctionRoot -Force | Out-Null
$junctionRoot = (Resolve-Path -LiteralPath $junctionRoot).Path

foreach ($plugin in $metadata.plugins.windows) {
    $source = (Resolve-Path -LiteralPath $plugin.path).Path
    $destination = Join-Path $junctionRoot $plugin.name
    if (Test-Path -LiteralPath $destination) {
        continue
    }
    New-Item -ItemType Junction -Path $destination -Target $source | Out-Null
}

Write-Output "WINDOWS_PLUGIN_JUNCTIONS_READY=$junctionRoot"
