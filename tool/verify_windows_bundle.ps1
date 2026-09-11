[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$BundlePath,

    [Parameter(Mandatory = $true)]
    [ValidateSet("x64-v2", "arm64")]
    [string]$Variant,

    [Parameter(Mandatory = $true)]
    [ValidatePattern("^v?[0-9]+\.[0-9]+\.[0-9]+(?:-beta\.[0-9]+)?$")]
    [string]$Version,

    [Parameter(Mandatory = $true)]
    [ValidatePattern("^[0-9A-Fa-f]{64}$")]
    [string]$SignerSha256,

    [ValidatePattern("^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$")]
    [string]$ExpectedAgentFileVersion,

    [switch]$VerifyAuthenticode,

    [switch]$AllowPinnedUntrustedRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Assert-Equal {
    param(
        [AllowNull()][object]$Actual,
        [AllowNull()][object]$Expected,
        [Parameter(Mandatory = $true)][string]$Description
    )
    if (-not [object]::Equals([string]$Actual, [string]$Expected)) {
        throw "$Description mismatch. Expected '$Expected', got '$Actual'."
    }
}

function Invoke-MsiScalarQuery {
    param(
        [Parameter(Mandatory = $true)][object]$Database,
        [Parameter(Mandatory = $true)][string]$Query
    )

    $view = $null
    $record = $null
    try {
        $view = $Database.GetType().InvokeMember(
            "OpenView",
            [Reflection.BindingFlags]::InvokeMethod,
            $null,
            $Database,
            @($Query)
        )
        $view.GetType().InvokeMember(
            "Execute",
            [Reflection.BindingFlags]::InvokeMethod,
            $null,
            $view,
            $null
        ) | Out-Null
        $record = $view.GetType().InvokeMember(
            "Fetch",
            [Reflection.BindingFlags]::InvokeMethod,
            $null,
            $view,
            $null
        )
        if ($null -eq $record) {
            return $null
        }
        return $record.GetType().InvokeMember(
            "StringData",
            [Reflection.BindingFlags]::GetProperty,
            $null,
            $record,
            1
        )
    }
    finally {
        if ($null -ne $record) {
            [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($record)
        }
        if ($null -ne $view) {
            [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($view)
        }
    }
}

function Get-PeMachine {
    param([Parameter(Mandatory = $true)][string]$Path)

    $stream = [IO.File]::Open(
        $Path,
        [IO.FileMode]::Open,
        [IO.FileAccess]::Read,
        [IO.FileShare]::Read
    )
    $reader = [IO.BinaryReader]::new($stream)
    try {
        if ($reader.ReadUInt16() -ne 0x5A4D) {
            throw "Bundle is not a DOS/PE executable: $Path"
        }
        $stream.Position = 0x3C
        $peOffset = $reader.ReadUInt32()
        if ($peOffset -gt $stream.Length - 6) {
            throw "Bundle has an invalid PE header offset: $Path"
        }
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550) {
            throw "Bundle has an invalid PE signature: $Path"
        }
        return $reader.ReadUInt16()
    }
    finally {
        $reader.Dispose()
        $stream.Dispose()
    }
}

$localizedCultures = @(
    "ar-SA",
    "de-DE",
    "es-ES",
    "fa-IR",
    "fr-FR",
    "id-ID",
    "it-IT",
    "ja-JP",
    "ko-KR",
    "nl-NL",
    "pl-PL",
    "pt-BR",
    "ru-RU",
    "th-TH",
    "tr-TR",
    "uk-UA",
    "vi-VN",
    "zh-CN",
    "zh-HK",
    "zh-TW"
)
$bundleUpgradeCodes = @{
    "x64-v2" = "{33C3EFF5-32C9-4BD6-B923-FA8FF6506CE3}"
    "arm64"  = "{EDE480D1-DE1B-4DC5-BE18-084CCABAE9D0}"
}

$resolvedBundle = (Resolve-Path -LiteralPath $BundlePath -ErrorAction Stop).Path
if (-not (Test-Path -LiteralPath $resolvedBundle -PathType Leaf)) {
    throw "Windows bundle is not a file: $resolvedBundle"
}
$expectedMachine = if ($Variant -eq "arm64") { [uint16]0xAA64 } else { [uint16]0x8664 }
$actualMachine = Get-PeMachine -Path $resolvedBundle
if ($actualMachine -ne $expectedMachine) {
    throw ("Bundle PE architecture mismatch: expected 0x{0:X4}, got 0x{1:X4}." -f `
            $expectedMachine, $actualMachine)
}

if ($VerifyAuthenticode) {
    & (Join-Path $PSScriptRoot "verify_windows_authenticode.ps1") `
        -Path $resolvedBundle `
        -SignerSha256 $SignerSha256 `
        -AllowPinnedUntrustedRoot:$AllowPinnedUntrustedRoot | Out-Null
}

$temporaryRoot = Join-Path ([IO.Path]::GetTempPath()) "usque-bundle-verification-$([guid]::NewGuid().ToString('N'))"
$temporaryRoot = [IO.Path]::GetFullPath($temporaryRoot)
$safeTempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
if (-not $temporaryRoot.StartsWith($safeTempRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to use a bundle verification directory outside the system temporary directory."
}
$payloadRoot = Join-Path $temporaryRoot "payload"
$baRoot = Join-Path $temporaryRoot "ba"
New-Item -ItemType Directory -Path $payloadRoot, $baRoot -Force | Out-Null

try {
    if ($VerifyAuthenticode) {
        $detachedEngine = Join-Path $temporaryRoot "detached-engine.exe"
        & (Join-Path $PSScriptRoot "extract_windows_burn_engine.ps1") `
            -BundlePath $resolvedBundle `
            -OutputPath $detachedEngine | Out-Null
        & (Join-Path $PSScriptRoot "verify_windows_authenticode.ps1") `
            -Path $detachedEngine `
            -SignerSha256 $SignerSha256 `
            -AllowPinnedUntrustedRoot:$AllowPinnedUntrustedRoot | Out-Null
    }

    & dotnet tool run wix -- burn extract `
        $resolvedBundle `
        -out $payloadRoot `
        -outba $baRoot | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "WiX bundle extraction failed with exit code $LASTEXITCODE."
    }

    $manifestPath = Join-Path $baRoot "manifest.xml"
    $baDataPath = Join-Path $baRoot "BootstrapperApplicationData.xml"
    [xml]$manifest = Get-Content -LiteralPath $manifestPath -Raw
    [xml]$baData = Get-Content -LiteralPath $baDataPath -Raw
    $manifestNs = [Xml.XmlNamespaceManager]::new($manifest.NameTable)
    $manifestNs.AddNamespace("burn", $manifest.DocumentElement.NamespaceURI)
    $baNs = [Xml.XmlNamespaceManager]::new($baData.NameTable)
    $baNs.AddNamespace("ba", $baData.DocumentElement.NamespaceURI)

    Assert-Equal $manifest.BurnManifest.EngineVersion "5.0.2.0" "Burn engine version"
    Assert-Equal $manifest.BurnManifest.Win64 "yes" "64-bit Burn engine"

    $relatedBundle = $manifest.SelectSingleNode(
        "/burn:BurnManifest/burn:RelatedBundle",
        $manifestNs
    )
    Assert-Equal $relatedBundle.Id $bundleUpgradeCodes[$Variant] "bundle upgrade code"
    Assert-Equal $relatedBundle.Action "Upgrade" "related-bundle action"

    $registration = $manifest.SelectSingleNode(
        "/burn:BurnManifest/burn:Registration",
        $manifestNs
    )
    Assert-Equal $registration.PerMachine "yes" "bundle registration scope"
    Assert-Equal $registration.ProviderKey "Usque.Windows.$Variant" "bundle provider key"
    Assert-Equal $registration.Version ($Version.TrimStart("v")) "bundle version"
    $arp = $registration.SelectSingleNode("burn:Arp", $manifestNs)
    Assert-Equal $arp.DisableModify "yes" "hidden bundle modify entry"
    Assert-Equal $arp.DisableRemove "yes" "hidden bundle remove entry"

    $variable = $manifest.SelectSingleNode(
        "/burn:BurnManifest/burn:Variable[@Id='UsqueMsiTransform']",
        $manifestNs
    )
    Assert-Equal $variable.Type "string" "transform selector variable type"
    Assert-Equal $variable.Persisted "yes" "transform selector persistence"

    $selectors = @($manifest.SelectNodes("/burn:BurnManifest/burn:SetVariable", $manifestNs))
    if ($selectors.Count -ne $localizedCultures.Count) {
        throw "Bundle must contain exactly $($localizedCultures.Count) language selectors."
    }
    $selectedTransformPaths = @(
        $selectors | ForEach-Object {
            Assert-Equal $_.Variable "UsqueMsiTransform" "language selector variable"
            if (
                [string]::IsNullOrWhiteSpace([string]$_.Condition) -or
                -not ([string]$_.Condition).Contains("UserUILanguageID") -or
                -not ([string]$_.Condition).Contains("NOT WixBundleInstalled")
            ) {
                throw "Language selector $($_.Id) does not use the required UI-language/initial-install condition."
            }
            [string]$_.Value
        }
    )
    $expectedTransformPaths = @(
        $localizedCultures | ForEach-Object { "transforms\$_.mst" } | Sort-Object
    )
    if (@(Compare-Object $expectedTransformPaths ($selectedTransformPaths | Sort-Object)).Count -ne 0) {
        throw "Bundle language selectors do not reference the exact transform set."
    }

    $packages = @($manifest.SelectNodes("/burn:BurnManifest/burn:Chain/burn:MsiPackage", $manifestNs))
    if ($packages.Count -ne 1) {
        throw "Bundle must contain exactly one MSI package."
    }
    $package = $packages[0]
    Assert-Equal $package.Id "UsqueMsi" "bundle MSI package id"
    Assert-Equal $package.Language "1033" "bundle base MSI language"
    Assert-Equal $package.UpgradeCode "{076CF387-E447-4666-9153-2DA16049A390}" "bundle MSI upgrade code"
    $msiVersion = & (Join-Path $PSScriptRoot "convert_to_msi_version.ps1") -SemVer $Version
    $agentFileVersion = if ([string]::IsNullOrWhiteSpace($ExpectedAgentFileVersion)) {
        "$msiVersion.0"
    }
    else {
        $ExpectedAgentFileVersion
    }
    Assert-Equal $package.Version $msiVersion "bundle MSI version"

    $msiProperties = @($package.SelectNodes("burn:MsiProperty", $manifestNs))
    $transformProperty = @($msiProperties | Where-Object Id -eq "TRANSFORMS")
    if ($transformProperty.Count -ne 1) {
        throw "Bundle must pass exactly one conditional TRANSFORMS property."
    }
    Assert-Equal `
        $transformProperty[0].Value `
        "[WixBundleExecutePackageCacheFolder][UsqueMsiTransform]" `
        "bundle TRANSFORMS value"
    Assert-Equal $transformProperty[0].Condition "UsqueMsiTransform" "bundle TRANSFORMS condition"
    $secureTransformProperty = @($msiProperties | Where-Object Id -eq "TRANSFORMSSECURE")
    if ($secureTransformProperty.Count -ne 1) {
        throw "Bundle must secure-cache its selected MSI transform."
    }
    Assert-Equal $secureTransformProperty[0].Value "1" "bundle TRANSFORMSSECURE value"
    Assert-Equal `
        $secureTransformProperty[0].Condition `
        "UsqueMsiTransform" `
        "bundle TRANSFORMSSECURE condition"

    $primary = $baData.SelectSingleNode(
        "//ba:WixBalPackageInfo[@PackageId='UsqueMsi']",
        $baNs
    )
    Assert-Equal $primary.PrimaryPackageType "default" "WixIUIBA primary MSI"

    $displayVersion = $Version.TrimStart("v")
    $expectedMsiName = "usque-v$displayVersion-windows-$Variant.msi"
    $containerEntries = @(Get-ChildItem -LiteralPath $payloadRoot -Force)
    if (
        $containerEntries.Count -ne 1 -or
        -not $containerEntries[0].PSIsContainer -or
        $containerEntries[0].Name -ne "WixAttachedContainer"
    ) {
        throw "Bundle must contain one attached payload container."
    }
    $containerRoot = $containerEntries[0].FullName
    $extractedFiles = @(
        Get-ChildItem -LiteralPath $containerRoot -File -Recurse |
            ForEach-Object {
                [IO.Path]::GetRelativePath($containerRoot, $_.FullName)
            } |
            Sort-Object
    )
    $expectedFiles = @($expectedMsiName) + $expectedTransformPaths
    if (@(Compare-Object ($expectedFiles | Sort-Object) $extractedFiles).Count -ne 0) {
        throw "Extracted bundle payload does not match one base MSI plus the exact language transforms."
    }

    $extractedMsi = (Resolve-Path -LiteralPath (Join-Path $containerRoot $expectedMsiName)).Path
    & (Join-Path $PSScriptRoot "verify_windows_msi.ps1") `
        -MsiPath $extractedMsi `
        -Variant $Variant `
        -ExpectedMsiVersion $msiVersion `
        -ExpectedDisplayVersion $displayVersion `
        -ExpectedAgentFileVersion $agentFileVersion `
        -ExpectedMsiLanguage 1033 `
        -SignerSha256 $SignerSha256 | Out-Null
    if ($VerifyAuthenticode) {
        & (Join-Path $PSScriptRoot "verify_windows_authenticode.ps1") `
            -Path $extractedMsi `
            -SignerSha256 $SignerSha256 `
            -AllowPinnedUntrustedRoot:$AllowPinnedUntrustedRoot | Out-Null
    }

    $installer = $null
    try {
        $installer = New-Object -ComObject WindowsInstaller.Installer
        foreach ($culture in $localizedCultures) {
            $database = $null
            try {
                $database = $installer.GetType().InvokeMember(
                    "OpenDatabase",
                    [Reflection.BindingFlags]::InvokeMethod,
                    $null,
                    $installer,
                    @([string]$extractedMsi, [int]0)
                )
                $transform = Join-Path $containerRoot "transforms\$culture.mst"
                $database.GetType().InvokeMember(
                    "ApplyTransform",
                    [Reflection.BindingFlags]::InvokeMethod,
                    $null,
                    $database,
                    @([string]$transform, [int]0)
                ) | Out-Null
                $language = Invoke-MsiScalarQuery `
                    -Database $database `
                    -Query "SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductLanguage'"
                $expectedLanguage = [Globalization.CultureInfo]::GetCultureInfo($culture).LCID
                Assert-Equal $language $expectedLanguage "$culture transform ProductLanguage"

                $title = Invoke-MsiScalarQuery `
                    -Database $database `
                    -Query "SELECT ``Text`` FROM ``Control`` WHERE ``Dialog_``='UsqueRemoveDataDlg' AND ``Control``='Title'"
                [xml]$loc = Get-Content `
                    -LiteralPath (Join-Path $PSScriptRoot "..\packaging\windows\loc\$culture.wxl") `
                    -Raw
                $expectedTitle = [string](@(
                        $loc.WixLocalization.String |
                            Where-Object Id -eq "UsqueRemoveDataTitle"
                    )[0].Value)
                if (-not ([string]$title).Contains($expectedTitle)) {
                    throw "$culture transform does not contain its localized Usque uninstall title."
                }
            }
            finally {
                if ($null -ne $database) {
                    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($database)
                }
            }
        }
    }
    finally {
        if ($null -ne $installer) {
            [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
        }
    }
}
finally {
    if (Test-Path -LiteralPath $temporaryRoot) {
        $resolvedTemporaryRoot = (Resolve-Path -LiteralPath $temporaryRoot).Path
        if (-not $resolvedTemporaryRoot.StartsWith(
                $safeTempRoot,
                [StringComparison]::OrdinalIgnoreCase
            )) {
            throw "Refusing to remove a bundle verification directory outside the system temporary directory."
        }
        Remove-Item -LiteralPath $resolvedTemporaryRoot -Recurse -Force
    }
}

Write-Output "WINDOWS_BUNDLE_OK=$Variant/$($Version.TrimStart('v'))/$($localizedCultures.Count)"
