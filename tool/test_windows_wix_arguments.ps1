# Compile-only argument transport regression. All EXE payloads are inert copies
# of the pinned Wintun DLL; no package, helper, service, or VPN is executed.
[CmdletBinding()]
param(
    [ValidateSet("x64-v2", "arm64")]
    [string]$Variant = "x64-v2"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$temporaryBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
# Include spaces to exercise path argument quoting as well as the encoded value.
$temporaryRoot = Join-Path $temporaryBase "Usque Wix Arguments $([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $temporaryRoot | Out-Null
$previousArgumentPassing = Get-Variable PSNativeCommandArgumentPassing -ValueOnly -ErrorAction SilentlyContinue
$modes = if ($null -eq $previousArgumentPassing) { @("Legacy") } else { @("Legacy", "Standard", "Windows") }

Push-Location $repositoryRoot
try {
    $architecture = if ($Variant -eq "arm64") { "arm64" } else { "x64" }
    $wintunArchitecture = if ($Variant -eq "arm64") { "arm64" } else { "amd64" }
    $payload = Join-Path $temporaryRoot "payload"
    New-Item -ItemType Directory -Path (Join-Path $payload "data") -Force | Out-Null
    $fixture = Join-Path $repositoryRoot "third_party/wintun-0.14.1/wintun/bin/$wintunArchitecture/wintun.dll"
    foreach ($name in @("usque.exe", "usque-engine.exe", "usque-agent.exe", "usque-uninstall.exe", "usque-update.exe", "wintun.dll")) {
        Copy-Item -LiteralPath $fixture -Destination (Join-Path $payload $name)
    }
    Copy-Item -LiteralPath (Join-Path $repositoryRoot "LICENSE.md") -Destination (Join-Path $payload "data/LICENSE.md")
    $include = Join-Path $temporaryRoot "en-US.wxi"
    & (Join-Path $PSScriptRoot "render_windows_wix_localization.ps1") `
        -LocalizationPath (Join-Path $repositoryRoot "packaging/windows/loc/en-US.wxl") `
        -OutputPath $include `
        -ExpectedCulture "en-US" | Out-Null
    $encodedScript = & (Join-Path $PSScriptRoot "get_windows_quiet_uninstall_command.ps1") -EncodedScriptOnly
    $productCode = "11111111-1111-1111-1111-111111111111"
    foreach ($mode in $modes) {
        if ($null -ne $previousArgumentPassing) { $PSNativeCommandArgumentPassing = $mode }
        $modeRoot = Join-Path $temporaryRoot $mode
        New-Item -ItemType Directory -Path $modeRoot | Out-Null
        $output = Join-Path $modeRoot "usque-v0.2.5-windows-$Variant.msi"
        $arguments = @(
            "build",
            "-arch", $architecture,
            "-ext", "WixToolset.UI.wixext/5.0.2",
            "-bindpath", "app=$payload",
            "-define", "DisplayVersion=0.2.5",
            "-define", "MsiVersion=0.2.599",
            "-define", "MsiLanguage=1033",
            "-define", "MsiCodepage=65001",
            "-define", "ProductCode=$productCode",
            "-define", "Variant=$Variant",
            "-define", "SignerSha256=$("0" * 64)",
            "-define", "IconPath=$(Join-Path $repositoryRoot 'assets/branding/usque-app-icon.ico')",
            "-define", "LicensePath=$(Join-Path $repositoryRoot 'packaging/windows/LICENSE.rtf')",
            "-define", "UsqueLocalizationPath=$include",
            "-define", "UsqueQuietUninstallScript=$encodedScript",
            "-culture", "en-US",
            "-intermediateFolder", (Join-Path $modeRoot "wix"),
            "-pdbtype", "none",
            "-out", $output,
            "packaging/windows/Usque.wxs",
            "packaging/windows/UsqueUI.wxs"
        )
        & dotnet tool run wix -- @arguments
        if ($LASTEXITCODE -ne 0) { throw "WiX argument transport failed for $Variant/$mode." }
        # Verify the actual MSI Registry table, including the complete quoted
        # launcher, not just an in-memory string or mock process invocation.
        & (Join-Path $PSScriptRoot "verify_windows_msi.ps1") `
            -MsiPath $output `
            -Variant $Variant `
            -ExpectedMsiVersion "0.2.599" `
            -ExpectedDisplayVersion "0.2.5" `
            -ExpectedAgentFileVersion "0.14.1.0" `
            -ExpectedMsiLanguage 1033 `
            -ExpectedProductCode $productCode `
            -SignerSha256 ("0" * 64) | Out-Null
        Write-Output "WIX_ARGUMENT_PASSING_OK=$Variant/$mode"
    }
}
finally {
    if ($null -ne $previousArgumentPassing) { $PSNativeCommandArgumentPassing = $previousArgumentPassing }
    Pop-Location
    if (Test-Path -LiteralPath $temporaryRoot) {
        $resolvedTemporaryRoot = (Resolve-Path -LiteralPath $temporaryRoot).Path
        if (-not $resolvedTemporaryRoot.StartsWith($temporaryBase, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove a WiX argument fixture outside the temporary directory."
        }
        Remove-Item -LiteralPath $resolvedTemporaryRoot -Recurse -Force
    }
}
