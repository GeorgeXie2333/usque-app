[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$LocalizationPath,

    [Parameter(Mandatory = $true)]
    [string]$OutputPath,

    [Parameter(Mandatory = $true)]
    [string]$ExpectedCulture
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$requiredIds = @(
    "UsqueRemoveDataTitle",
    "UsqueRemoveDataDescription",
    "UsqueRemoveDataCheckbox",
    "UsqueRemoveDataWarning",
    "UsqueRepairTitle",
    "UsqueRepairDescription",
    "UsqueRepairOk"
)

$source = (Resolve-Path -LiteralPath $LocalizationPath -ErrorAction Stop).Path
if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
    throw "WiX localization source is not a file: $source"
}

[xml]$document = Get-Content -LiteralPath $source -Raw
$actualCulture = [string]$document.WixLocalization.Culture
if (-not [StringComparer]::OrdinalIgnoreCase.Equals($actualCulture, $ExpectedCulture)) {
    throw "WiX localization culture mismatch in $source. Expected $ExpectedCulture, got $actualCulture."
}
$strings = @($document.WixLocalization.String)
$byId = @{}
foreach ($entry in $strings) {
    $id = [string]$entry.Id
    if ($id -notin $requiredIds) {
        throw "Unexpected Usque WiX localization string '$id' in $source."
    }
    if ($byId.ContainsKey($id)) {
        throw "Duplicate Usque WiX localization string '$id' in $source."
    }
    $value = [string]$entry.Value
    if ([string]::IsNullOrWhiteSpace($value)) {
        throw "Usque WiX localization string '$id' is empty in $source."
    }
    if ($value.Contains("?>")) {
        throw "Usque WiX localization string '$id' contains an unsupported processing-instruction terminator."
    }
    $byId[$id] = $value
}

$missing = @($requiredIds | Where-Object { -not $byId.ContainsKey($_) })
if ($missing.Count -ne 0) {
    throw "Missing Usque WiX localization strings in $source`: $($missing -join ', ')"
}

$destination = [IO.Path]::GetFullPath($OutputPath)
$parent = Split-Path -Parent $destination
if ([string]::IsNullOrWhiteSpace($parent)) {
    throw "Generated WiX localization include needs a parent directory."
}
New-Item -ItemType Directory -Path $parent -Force | Out-Null

$lines = [System.Collections.Generic.List[string]]::new()
$lines.Add('<?xml version="1.0" encoding="utf-8"?>')
$lines.Add('<Include xmlns="http://wixtoolset.org/schemas/v4/wxs">')
foreach ($id in $requiredIds) {
    $escaped = [Security.SecurityElement]::Escape([string]$byId[$id])
    $lines.Add("  <?define $id = `"$escaped`" ?>")
}
$lines.Add('</Include>')
$lines.Add('')

[IO.File]::WriteAllText(
    $destination,
    ($lines -join "`r`n"),
    [Text.UTF8Encoding]::new($false)
)
Write-Output $destination
