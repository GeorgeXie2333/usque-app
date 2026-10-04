#Requires -Version 7.0
# Compile-only fixtures for the complete Windows installer authoring gate.
# Never executes a bundle, installs an MSI, or uses release signing material.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("x64-v2", "arm64")]
    [string]$Variant,

    # Embedded as data only. May be a compiled BA or an explicitly inert,
    # matching-architecture PE fixture; this gate does not prove BA execution.
    [Parameter(Mandatory = $true)]
    [string]$BootstrapperPath,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$bootstrapper = (Resolve-Path -LiteralPath $BootstrapperPath -ErrorAction Stop).Path
if (-not (Test-Path -LiteralPath $bootstrapper -PathType Leaf)) {
    throw "BootstrapperPath must be an existing PE file; it will only be embedded."
}
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $outputRoot) {
    $item = Get-Item -LiteralPath $outputRoot -Force
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "OutputDirectory must be an empty, ordinary directory."
    }
    if (@(Get-ChildItem -LiteralPath $outputRoot -Force).Count -ne 0) {
        throw "OutputDirectory must be empty; existing artifacts are never overwritten."
    }
}
else {
    New-Item -ItemType Directory -Path $outputRoot -ErrorAction Stop | Out-Null
}

$architecture = if ($Variant -eq "arm64") { "arm64" } else { "x64" }
$wintunArchitecture = if ($Variant -eq "arm64") { "arm64" } else { "amd64" }
$version = "0.2.9"
$msiVersion = "0.2.999"
$productCode = "11111111-1111-1111-1111-111111111111"
$signer = "0" * 64
$expectedCultures = @(
    "ar-SA", "de-DE", "en-US", "es-ES", "fa-IR", "fr-FR", "id-ID",
    "it-IT", "ja-JP", "ko-KR", "nl-NL", "pl-PL", "pt-BR", "ru-RU",
    "th-TH", "tr-TR", "uk-UA", "vi-VN", "zh-CN", "zh-HK", "zh-TW"
)
$payload = Join-Path $outputRoot "payload"
$msiDirectory = Join-Path $outputRoot "msi"
$transformDirectory = Join-Path $outputRoot "transforms"
$temporaryDirectory = Join-Path $outputRoot "temporary"
$transcriptPath = Join-Path $outputRoot "authoring.log"
$resultPath = Join-Path $outputRoot "authoring-results.json"
$oldTemporary = $env:TEMP
$oldTmp = $env:TMP
$summary = [ordered]@{
    status = "running"
    variant = $Variant
    bootstrapper_path = $bootstrapper
    bootstrapper_execution = "not_run"
    installer_execution = "not_run"
    msi_ice_cultures = @()
    transforms = @()
    completed_checks = @()
    current_check = "initialization"
}

Start-Transcript -LiteralPath $transcriptPath -ErrorAction Stop | Out-Null
Push-Location $repositoryRoot
try {
    New-Item -ItemType Directory -Path $msiDirectory, $transformDirectory, $temporaryDirectory |
        Out-Null
    # Existing regression helpers own and validate their temporary subtrees.
    # Keep those fixtures under this run too, restoring both variables below.
    $env:TEMP = $temporaryDirectory
    $env:TMP = $temporaryDirectory

    $summary.current_check = "pinned_wix_restore"
    & dotnet tool restore
    if ($LASTEXITCODE -ne 0) { throw "Pinned WiX tool restore failed." }
    foreach ($extension in @("WixToolset.UI.wixext/5.0.2", "WixToolset.BootstrapperApplications.wixext/5.0.2")) {
        & dotnet tool run wix -- extension add $extension
        if ($LASTEXITCODE -ne 0) { throw "Pinned WiX extension restore failed: $extension" }
    }
    $summary.completed_checks += "pinned_wix_restore"

    $summary.current_check = "version_mapping"
    $stable = & (Join-Path $PSScriptRoot "convert_to_msi_version.ps1") -SemVer "v0.2.9"
    $beta = & (Join-Path $PSScriptRoot "convert_to_msi_version.ps1") -SemVer "0.2.9-beta.3"
    if ($stable -ne "0.2.999" -or $beta -ne "0.2.903") { throw "MSI version mapping is invalid." }
    $rejected = $false
    try { & (Join-Path $PSScriptRoot "convert_to_msi_version.ps1") -SemVer "0.2.9-beta.0" }
    catch { $rejected = $_.Exception.Message -like "Beta ordinal must be*" }
    if (-not $rejected) { throw "Invalid beta ordinal was not rejected." }
    $summary.completed_checks += "version_mapping"

    $summary.current_check = "inert_payload"
    New-Item -ItemType Directory -Path (Join-Path $payload "data") -Force | Out-Null
    $fixture = Join-Path $repositoryRoot "third_party/wintun-0.14.1/wintun/bin/$wintunArchitecture/wintun.dll"
    foreach ($name in @("usque.exe", "usque-engine.exe", "usque-agent.exe", "usque-uninstall.exe", "usque-update.exe", "wintun.dll")) {
        Copy-Item -LiteralPath $fixture -Destination (Join-Path $payload $name)
    }
    Copy-Item -LiteralPath (Join-Path $repositoryRoot "LICENSE.md") -Destination (Join-Path $payload "data/LICENSE.md")
    $summary.completed_checks += "inert_payload"

    $summary.current_check = "wix_argument_transport"
    & (Join-Path $PSScriptRoot "test_windows_wix_arguments.ps1") -Variant $Variant
    $summary.completed_checks += "wix_argument_transport"

    $summary.current_check = "signing_cleanup_doubles"
    & (Join-Path $PSScriptRoot "test_windows_release_signing.ps1")
    $summary.completed_checks += "signing_cleanup_doubles"

    $summary.current_check = "quiet_uninstall_powershell7"
    & (Join-Path $PSScriptRoot "test_windows_quiet_uninstall.ps1")
    $summary.completed_checks += "quiet_uninstall_powershell7"
    $summary.current_check = "quiet_uninstall_powershell51"
    & "$env:SystemRoot/System32/WindowsPowerShell/v1.0/powershell.exe" `
        -NoProfile -NonInteractive -File (Join-Path $PSScriptRoot "test_windows_quiet_uninstall.ps1")
    if ($LASTEXITCODE -ne 0) { throw "Windows PowerShell quiet-uninstall tests failed." }
    $summary.completed_checks += "quiet_uninstall_powershell51"

    $cultures = @(
        Get-ChildItem -LiteralPath "packaging/windows/loc" -Filter "*.wxl" |
            ForEach-Object BaseName | Sort-Object
    )
    if (@(Compare-Object ($expectedCultures | Sort-Object) $cultures).Count -ne 0) {
        throw "The authoring gate requires the complete 21-language localization set."
    }
    $quietUninstallScript = & (Join-Path $PSScriptRoot "get_windows_quiet_uninstall_command.ps1") -EncodedScriptOnly
    foreach ($culture in $cultures) {
        $summary.current_check = "msi_build_and_ice/$culture"
        $language = [Globalization.CultureInfo]::GetCultureInfo($culture).LCID
        $include = Join-Path $outputRoot "$culture.wxi"
        & (Join-Path $PSScriptRoot "render_windows_wix_localization.ps1") `
            -LocalizationPath "packaging/windows/loc/$culture.wxl" `
            -OutputPath $include -ExpectedCulture $culture | Out-Null
        $name = if ($culture -eq "en-US") {
            "usque-v$version-windows-$Variant.msi"
        }
        else { "usque-v$version-windows-$Variant-$culture.msi" }
        $output = Join-Path $msiDirectory $name
        $arguments = @(
            "build", "-arch", $architecture,
            "-ext", "WixToolset.UI.wixext/5.0.2",
            "-bindpath", "app=$payload",
            "-define", "DisplayVersion=$version",
            "-define", "MsiVersion=$msiVersion",
            "-define", "MsiLanguage=$language",
            "-define", "MsiCodepage=65001",
            "-define", "ProductCode=$productCode",
            "-define", "Variant=$Variant",
            "-define", "SignerSha256=$signer",
            "-define", "IconPath=$(Join-Path $repositoryRoot 'assets/branding/usque-app-icon.ico')",
            "-define", "LicensePath=$(Join-Path $repositoryRoot 'packaging/windows/LICENSE.rtf')",
            "-define", "UsqueLocalizationPath=$include",
            "-define", "UsqueQuietUninstallScript=$quietUninstallScript",
            "-culture", $culture
        )
        if ($culture -ne "en-US") { $arguments += @("-culture", "en-US") }
        $arguments += @(
            "-defaultcompressionlevel", "high",
            "-intermediateFolder", (Join-Path $outputRoot "wix-$culture"),
            "-pdbtype", "none", "-out", $output,
            "packaging/windows/Usque.wxs", "packaging/windows/UsqueUI.wxs"
        )
        & dotnet tool run wix -- @arguments
        if ($LASTEXITCODE -ne 0) { throw "WiX build failed for $culture." }
        & (Join-Path $PSScriptRoot "verify_windows_msi.ps1") `
            -MsiPath $output -Variant $Variant `
            -ExpectedMsiVersion $msiVersion -ExpectedDisplayVersion $version `
            -ExpectedAgentFileVersion "0.14.1.0" -ExpectedMsiLanguage $language `
            -ExpectedProductCode $productCode -SignerSha256 $signer | Out-Null
        # Only ICE61 is suppressed for the intentional equal-version upgrade.
        # Every language must independently pass the full remaining ICE set.
        & dotnet tool run wix -- msi validate -sice ICE61 $output
        if ($LASTEXITCODE -ne 0) { throw "MSI ICE validation failed for $culture." }
        $summary.msi_ice_cultures += $culture
        Write-Output "AUTHORING_MSI_ICE_PASS=$culture"
    }

    $baseMsi = Join-Path $msiDirectory "usque-v$version-windows-$Variant.msi"
    foreach ($culture in @($cultures | Where-Object { $_ -ne "en-US" })) {
        $summary.current_check = "transform/$culture"
        $localizedMsi = Join-Path $msiDirectory "usque-v$version-windows-$Variant-$culture.msi"
        & dotnet tool run wix -- msi transform $baseMsi $localizedMsi `
            -out (Join-Path $transformDirectory "$culture.mst") -t language
        if ($LASTEXITCODE -ne 0) { throw "WiX transform generation failed for $culture." }
        $summary.transforms += $culture
    }

    $summary.current_check = "bundle_build_and_verify"
    $bundleUpgradeCode = if ($Variant -eq "arm64") {
        "EDE480D1-DE1B-4DC5-BE18-084CCABAE9D0"
    }
    else { "33C3EFF5-32C9-4BD6-B923-FA8FF6506CE3" }
    $bundle = Join-Path $outputRoot "usque-v$version-windows-$Variant.exe"
    & dotnet tool run wix -- build -arch $architecture `
        -ext "WixToolset.BootstrapperApplications.wixext/5.0.2" `
        -define "DisplayVersion=$version" -define "BundleVersion=$version" `
        -define "BundleUpgradeCode=$bundleUpgradeCode" -define "Variant=$Variant" `
        -define "IconPath=$(Join-Path $repositoryRoot 'assets/branding/usque-app-icon.ico')" `
        -define "BootstrapperPath=$bootstrapper" `
        -define "LicensePath=$(Join-Path $repositoryRoot 'packaging/windows/LICENSE.rtf')" `
        -define "MsiPath=$baseMsi" -define "TransformDirectory=$transformDirectory" `
        -intermediateFolder (Join-Path $outputRoot "wix-bundle") `
        -pdbtype none -out $bundle packaging/windows/UsqueBundle.wxs
    if ($LASTEXITCODE -ne 0) { throw "WiX bundle build failed." }
    & (Join-Path $PSScriptRoot "verify_windows_bundle.ps1") `
        -BundlePath $bundle -Variant $Variant -Version $version `
        -SignerSha256 $signer -ExpectedAgentFileVersion "0.14.1.0" | Out-Null
    $summary.completed_checks += "bundle_build_and_verify"

    $summary.current_check = "burn_detach_reattach"
    $engine = Join-Path $outputRoot "burn-engine.exe"
    $reattached = Join-Path $outputRoot "reattached.exe"
    $detachIntermediate = Join-Path $outputRoot "wix-detach"
    $reattachIntermediate = Join-Path $outputRoot "wix-reattach"
    New-Item -ItemType Directory -Path $detachIntermediate, $reattachIntermediate | Out-Null
    & dotnet tool run wix -- burn detach $bundle -engine $engine `
        -intermediateFolder $detachIntermediate
    if ($LASTEXITCODE -ne 0) { throw "Burn engine detach failed." }
    & dotnet tool run wix -- burn reattach $bundle -engine $engine -out $reattached `
        -intermediateFolder $reattachIntermediate
    if ($LASTEXITCODE -ne 0) { throw "Burn engine reattach failed." }
    & (Join-Path $PSScriptRoot "verify_windows_bundle.ps1") `
        -BundlePath $reattached -Variant $Variant -Version $version `
        -SignerSha256 $signer -ExpectedAgentFileVersion "0.14.1.0" | Out-Null
    $summary.completed_checks += "burn_detach_reattach"

    $summary.current_check = "base_msi_contract"
    & (Join-Path $PSScriptRoot "verify_windows_msi.ps1") `
        -MsiPath $baseMsi -Variant $Variant `
        -ExpectedMsiVersion $msiVersion -ExpectedDisplayVersion $version `
        -ExpectedAgentFileVersion "0.14.1.0" -ExpectedProductCode $productCode `
        -SignerSha256 $signer
    $summary.completed_checks += "base_msi_contract"

    $summary.current_check = "msi_replacement_negative_tests"
    & (Join-Path $PSScriptRoot "test_windows_msi_replacement.ps1") `
        -MsiPath $baseMsi -Variant $Variant `
        -ExpectedMsiVersion $msiVersion -ExpectedDisplayVersion $version `
        -ExpectedAgentFileVersion "0.14.1.0" -SignerSha256 $signer
    $summary.completed_checks += "msi_replacement_negative_tests"

    $summary.current_check = "japanese_ice03_negative_test"
    & (Join-Path $PSScriptRoot "test_windows_msi_localization.ps1") `
        -MsiPath (Join-Path $msiDirectory "usque-v$version-windows-$Variant-ja-JP.msi")
    $summary.completed_checks += "japanese_ice03_negative_test"

    $summary.current_check = "signed_burn_engine_inert_tests"
    # Creates and removes a fresh non-exportable identity in CurrentUser/My.
    # The existing test never adds trust, signs payloads, or executes a bundle.
    & (Join-Path $PSScriptRoot "test_windows_burn_engine.ps1") -BundlePath $bundle
    $summary.completed_checks += "signed_burn_engine_inert_tests"
    $summary.current_check = "complete"
    $summary.status = "passed"
    Write-Output "WINDOWS_INSTALLER_AUTHORING=PASS/$Variant"
}
catch {
    $summary.status = "failed"
    $summary.error = $_.Exception.Message
    throw
}
finally {
    $summary | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $resultPath -Encoding utf8
    $env:TEMP = $oldTemporary
    $env:TMP = $oldTmp
    Pop-Location
    Stop-Transcript | Out-Null
}
