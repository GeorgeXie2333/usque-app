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

function Install-PluginJunction {
    param(
        [Parameter(Mandatory = $true)][string]$PlatformName,
        $Plugins
    )
    $platformRoot = Join-Path $project $PlatformName
    if (-not (Test-Path -LiteralPath $platformRoot -PathType Container)) {
        return
    }
    $plugins = @($Plugins)
    if ($plugins.Count -eq 0) {
        return
    }
    $junctionRoot = Join-Path $platformRoot "flutter/ephemeral/.plugin_symlinks"
    New-Item -ItemType Directory -Path $junctionRoot -Force | Out-Null
    $junctionRoot = (Resolve-Path -LiteralPath $junctionRoot).Path
    foreach ($plugin in $plugins) {
        $source = (Resolve-Path -LiteralPath $plugin.path).Path
        $destination = Join-Path $junctionRoot $plugin.name
        if (Test-Path -LiteralPath $destination) {
            continue
        }
        New-Item -ItemType Junction -Path $destination -Target $source | Out-Null
    }
    Write-Output ("{0}_PLUGIN_JUNCTIONS_READY={1}" -f $PlatformName.ToUpperInvariant(), $junctionRoot)
}

# flutter build windows also materializes Linux plugin links when that
# embedder exists. Junctions avoid the Developer Mode symlink requirement.
Install-PluginJunction -PlatformName "windows" -Plugins $metadata.plugins.windows
Install-PluginJunction -PlatformName "linux" -Plugins $metadata.plugins.linux
