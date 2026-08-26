[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$MsiPath,

    [Parameter(Mandatory = $true)]
    [ValidateSet("x64-v1", "x64-v2", "arm64")]
    [string]$Variant,

    [Parameter(Mandatory = $true)]
    [ValidatePattern("^[0-9]+\.[0-9]+\.[0-9]+$")]
    [string]$ExpectedMsiVersion,

    [Parameter(Mandatory = $true)]
    [ValidatePattern("^[0-9]+\.[0-9]+\.[0-9]+(?:-beta\.[0-9]+)?$")]
    [string]$ExpectedDisplayVersion,

    [Parameter(Mandatory = $true)]
    [ValidatePattern("^[0-9A-Fa-f]{64}$")]
    [string]$SignerSha256
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Invoke-MsiQuery {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Database,
        [Parameter(Mandatory = $true)]
        [string]$Query,
        [Parameter(Mandatory = $true)]
        [string[]]$Columns
    )

    $view = $null
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

        $rows = @()
        while ($true) {
            $record = $view.GetType().InvokeMember(
                "Fetch",
                [Reflection.BindingFlags]::InvokeMethod,
                $null,
                $view,
                $null
            )
            if ($null -eq $record) {
                break
            }
            try {
                $row = [ordered]@{}
                for ($index = 0; $index -lt $Columns.Count; $index++) {
                    $row[$Columns[$index]] = $record.GetType().InvokeMember(
                        "StringData",
                        [Reflection.BindingFlags]::GetProperty,
                        $null,
                        $record,
                        ($index + 1)
                    )
                }
                $rows += [pscustomobject]$row
            }
            finally {
                [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($record)
            }
        }
        return @($rows)
    }
    finally {
        if ($null -ne $view) {
            [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($view)
        }
    }
}

function Assert-OneRow {
    param(
        [Parameter(Mandatory = $true)]
        [object[]]$Rows,
        [Parameter(Mandatory = $true)]
        [string]$Description
    )
    if ($Rows.Count -ne 1) {
        throw "$Description must have exactly one MSI table row; found $($Rows.Count)."
    }
    return $Rows[0]
}

function Assert-Equal {
    param(
        [AllowNull()]
        [object]$Actual,
        [AllowNull()]
        [object]$Expected,
        [Parameter(Mandatory = $true)]
        [string]$Description
    )
    if (-not [object]::Equals([string]$Actual, [string]$Expected)) {
        throw "$Description mismatch. Expected '$Expected', got '$Actual'."
    }
}

function Get-MsiStreamSize {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Database,
        [Parameter(Mandatory = $true)]
        [string]$Query
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
            return 0
        }
        return [int]$record.GetType().InvokeMember(
            "DataSize",
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

$resolvedMsi = (Resolve-Path -LiteralPath $MsiPath -ErrorAction Stop).Path
if (-not (Test-Path -LiteralPath $resolvedMsi -PathType Leaf)) {
    throw "MSI does not exist: $resolvedMsi"
}

$installer = $null
$database = $null
try {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.GetType().InvokeMember(
        "OpenDatabase",
        [Reflection.BindingFlags]::InvokeMethod,
        $null,
        $installer,
        @($resolvedMsi, 0)
    )

    $tableRows = Invoke-MsiQuery `
        -Database $database `
        -Query "SELECT ``Name`` FROM ``_Tables``" `
        -Columns @("Name")
    $tableNames = @($tableRows | ForEach-Object { $_.Name })
    if ($tableNames -notcontains "MsiLockPermissionsEx") {
        throw "MSI is missing the MsiLockPermissionsEx table."
    }
    if ($tableNames -contains "LockPermissions") {
        throw "MSI must not combine LockPermissions with MsiLockPermissionsEx."
    }

    $properties = Invoke-MsiQuery `
        -Database $database `
        -Query "SELECT ``Property``,``Value`` FROM ``Property``" `
        -Columns @("Property", "Value")
    $propertyMap = @{}
    foreach ($property in $properties) {
        $propertyMap[$property.Property] = $property.Value
    }
    Assert-Equal $propertyMap.ProductVersion $ExpectedMsiVersion "ProductVersion"
    Assert-Equal $propertyMap.ProductName "Usque $ExpectedDisplayVersion" "ProductName"
    Assert-Equal `
        $propertyMap.UpgradeCode `
        "{076CF387-E447-4666-9153-2DA16049A390}" `
        "UpgradeCode"
    Assert-Equal `
        $propertyMap.ARPCOMMENTS `
        "Unofficial Consumer WARP client. Installed variant: $Variant." `
        "ARPCOMMENTS"
    Assert-Equal $propertyMap.WIXUI_INSTALLDIR "INSTALLFOLDER" "WixUI install directory"
    Assert-Equal $propertyMap.ARPSYSTEMCOMPONENT "1" "hidden MSI ARP entry"
    if (
        $propertyMap.ContainsKey("USQUE_REMOVE_USER_DATA") -and
        -not [string]::IsNullOrEmpty([string]$propertyMap.USQUE_REMOVE_USER_DATA)
    ) {
        throw "USQUE_REMOVE_USER_DATA must not have a default Property table value."
    }

    $secureProperties = @($propertyMap.SecureCustomProperties -split ";")
    foreach ($secureProperty in @("INSTALLFOLDER", "USQUE_REMOVE_USER_DATA")) {
        if ($secureProperties -notcontains $secureProperty) {
            throw "$secureProperty must be listed in SecureCustomProperties."
        }
    }

    $installDirectory = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Directory``,``Directory_Parent``,``DefaultDir`` FROM ``Directory`` WHERE ``Directory``='INSTALLFOLDER'" `
            -Columns @("Directory", "Parent", "DefaultDir")) `
        "INSTALLFOLDER directory"
    Assert-Equal $installDirectory.Parent "ProgramFiles64Folder" "INSTALLFOLDER parent"
    Assert-Equal $installDirectory.DefaultDir "Usque" "INSTALLFOLDER default name"

    $installLocationRegistry = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Root``,``Key``,``Name``,``Value``,``Component_`` FROM ``Registry`` WHERE ``Key``='Software\Usque' AND ``Name``='InstallLocation'" `
            -Columns @("Root", "Key", "Name", "Value", "Component")) `
        "persisted install location"
    Assert-Equal $installLocationRegistry.Root "2" "InstallLocation registry root"
    Assert-Equal $installLocationRegistry.Key "Software\Usque" "InstallLocation registry key"
    Assert-Equal $installLocationRegistry.Value "[INSTALLFOLDER]" "InstallLocation registry value"
    Assert-Equal `
        $installLocationRegistry.Component `
        "UsqueGuiComponent" `
        "InstallLocation registry component"

    $productCodeRegistry = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Root``,``Key``,``Name``,``Value``,``Component_`` FROM ``Registry`` WHERE ``Key``='Software\Usque' AND ``Name``='ProductCode'" `
            -Columns @("Root", "Key", "Name", "Value", "Component")) `
        "persisted product code"
    Assert-Equal $productCodeRegistry.Root "2" "ProductCode registry root"
    Assert-Equal $productCodeRegistry.Value "[ProductCode]" "ProductCode registry value"
    Assert-Equal `
        $productCodeRegistry.Component `
        "UsqueGuiComponent" `
        "ProductCode registry component"

    $arpUninstallString = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Root``,``Key``,``Name``,``Value``,``Component_`` FROM ``Registry`` WHERE ``Key``='Software\Microsoft\Windows\CurrentVersion\Uninstall\Usque' AND ``Name``='UninstallString'" `
            -Columns @("Root", "Key", "Name", "Value", "Component")) `
        "custom ARP uninstall string"
    Assert-Equal $arpUninstallString.Root "2" "custom ARP uninstall root"
    Assert-Equal `
        $arpUninstallString.Value `
        '"[INSTALLFOLDER]usque-uninstall.exe"' `
        "custom ARP uninstall string"
    Assert-Equal `
        $arpUninstallString.Component `
        "UsqueUninstallComponent" `
        "custom ARP uninstall component"

    $arpQuietUninstall = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Value`` FROM ``Registry`` WHERE ``Key``='Software\Microsoft\Windows\CurrentVersion\Uninstall\Usque' AND ``Name``='QuietUninstallString'" `
            -Columns @("Value")) `
        "custom ARP quiet uninstall string"
    Assert-Equal `
        $arpQuietUninstall.Value `
        "msiexec /x [ProductCode] /qn" `
        "custom ARP quiet uninstall string"
    if ($arpQuietUninstall.Value -like "*USQUE_REMOVE_USER_DATA=1*") {
        throw "QuietUninstallString must not request user-data deletion."
    }

    $arpNoModify = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Value`` FROM ``Registry`` WHERE ``Key``='Software\Microsoft\Windows\CurrentVersion\Uninstall\Usque' AND ``Name``='NoModify'" `
            -Columns @("Value")) `
        "custom ARP NoModify"
    Assert-Equal $arpNoModify.Value "#1" "custom ARP NoModify"
    $arpNoRepair = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Value`` FROM ``Registry`` WHERE ``Key``='Software\Microsoft\Windows\CurrentVersion\Uninstall\Usque' AND ``Name``='NoRepair'" `
            -Columns @("Value")) `
        "custom ARP NoRepair"
    Assert-Equal $arpNoRepair.Value "#1" "custom ARP NoRepair"

    $installLocationSearch = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Property``,``Signature_`` FROM ``AppSearch`` WHERE ``Property``='INSTALLFOLDER'" `
            -Columns @("Property", "Signature")) `
        "install location AppSearch"
    Assert-Equal `
        $installLocationSearch.Signature `
        "UsqueInstallLocationSearch" `
        "InstallLocation search signature"

    $installLocationLocator = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Root``,``Key``,``Name``,``Type`` FROM ``RegLocator`` WHERE ``Signature_``='UsqueInstallLocationSearch'" `
            -Columns @("Root", "Key", "Name", "Type")) `
        "install location registry locator"
    Assert-Equal $installLocationLocator.Root "2" "InstallLocation locator root"
    Assert-Equal $installLocationLocator.Key "Software\Usque" "InstallLocation locator key"
    Assert-Equal $installLocationLocator.Name "InstallLocation" "InstallLocation locator name"
    if (([int]$installLocationLocator.Type -band 16) -eq 0) {
        throw "InstallLocation registry search must read the 64-bit registry view."
    }

    $installDirectoryDialog = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Dialog`` FROM ``Dialog`` WHERE ``Dialog``='InstallDirDlg'" `
            -Columns @("Dialog")) `
        "install-directory dialog"
    Assert-Equal $installDirectoryDialog.Dialog "InstallDirDlg" "install-directory dialog id"

    $removeDataDialog = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Dialog`` FROM ``Dialog`` WHERE ``Dialog``='UsqueRemoveDataDlg'" `
            -Columns @("Dialog")) `
        "uninstall data dialog"
    Assert-Equal $removeDataDialog.Dialog "UsqueRemoveDataDlg" "uninstall data dialog id"

    $removeDataCheckbox = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Type``,``Property``,``Text`` FROM ``Control`` WHERE ``Dialog_``='UsqueRemoveDataDlg' AND ``Control``='DeleteUserData'" `
            -Columns @("Type", "Property", "Text")) `
        "uninstall data checkbox"
    Assert-Equal $removeDataCheckbox.Type "CheckBox" "uninstall data control type"
    Assert-Equal `
        $removeDataCheckbox.Property `
        "USQUE_REMOVE_USER_DATA" `
        "uninstall data checkbox property"
    if ([string]::IsNullOrWhiteSpace($removeDataCheckbox.Text)) {
        throw "Uninstall data checkbox must have a user-visible label."
    }

    $removeDataCheckboxValue = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Property``,``Value`` FROM ``CheckBox`` WHERE ``Property``='USQUE_REMOVE_USER_DATA'" `
            -Columns @("Property", "Value")) `
        "uninstall data checkbox value"
    Assert-Equal $removeDataCheckboxValue.Value "1" "uninstall data checked value"

    foreach ($route in @(
            @("MaintenanceTypeDlg", "RemoveButton", "UsqueRemoveDataDlg"),
            @("UsqueRemoveDataDlg", "Back", "MaintenanceTypeDlg"),
            @("UsqueRemoveDataDlg", "Next", "VerifyReadyDlg")
        )) {
        $routeRow = Assert-OneRow `
        (Invoke-MsiQuery `
                -Database $database `
                -Query "SELECT ``Event``,``Argument`` FROM ``ControlEvent`` WHERE ``Dialog_``='$($route[0])' AND ``Control_``='$($route[1])' AND ``Event``='NewDialog' AND ``Argument``='$($route[2])'" `
                -Columns @("Event", "Argument")) `
            "$($route[0]).$($route[1]) dialog route"
        Assert-Equal $routeRow.Event "NewDialog" "$($route[0]).$($route[1]) route event"
    }

    $removeMode = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Argument`` FROM ``ControlEvent`` WHERE ``Dialog_``='MaintenanceTypeDlg' AND ``Control_``='RemoveButton' AND ``Event``='[WixUI_InstallMode]'" `
            -Columns @("Argument")) `
        "maintenance uninstall-mode selection"
    Assert-Equal $removeMode.Argument "Remove" "maintenance uninstall mode"

    $repairRoute = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Event``,``Argument`` FROM ``ControlEvent`` WHERE ``Dialog_``='MaintenanceTypeDlg' AND ``Control_``='RepairButton' AND ``Event``='SpawnDialog'" `
            -Columns @("Event", "Argument")) `
        "unsupported Repair dialog route"
    Assert-Equal $repairRoute.Event "SpawnDialog" "Repair control event"
    Assert-Equal `
        $repairRoute.Argument `
        "UsqueRepairUnsupportedDlg" `
        "Repair explanation dialog"
    $repairExecutionRoutes = @(Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Argument`` FROM ``ControlEvent`` WHERE ``Dialog_``='MaintenanceTypeDlg' AND ``Control_``='RepairButton' AND ``Event``='NewDialog'" `
            -Columns @("Argument"))
    if ($repairExecutionRoutes.Count -ne 0) {
        throw "RepairButton must not navigate to an executable maintenance path."
    }

    $removeBackRoute = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Condition`` FROM ``ControlEvent`` WHERE ``Dialog_``='VerifyReadyDlg' AND ``Control_``='Back' AND ``Event``='NewDialog' AND ``Argument``='UsqueRemoveDataDlg'" `
            -Columns @("Condition")) `
        "uninstall confirmation back route"
    Assert-Equal `
        $removeBackRoute.Condition `
        'Installed AND NOT PATCH AND WixUI_InstallMode="Remove"' `
        "uninstall confirmation back condition"

    $shortcut = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Shortcut``,``Directory_``,``Name``,``Component_``,``Target``,``Icon_``,``IconIndex``,``WkDir`` FROM ``Shortcut`` WHERE ``Shortcut``='UsqueStartMenuShortcut'" `
            -Columns @(
            "Shortcut",
            "Directory",
            "Name",
            "Component",
            "Target",
            "Icon",
            "IconIndex",
            "WorkingDirectory"
        )) `
        "Usque Start Menu shortcut"
    Assert-Equal $shortcut.Directory "UsqueProgramMenuFolder" "shortcut directory"
    Assert-Equal $shortcut.Name "Usque" "shortcut name"
    Assert-Equal $shortcut.Component "UsqueShortcutComponent" "shortcut component"
    Assert-Equal $shortcut.Target "[INSTALLFOLDER]usque.exe" "non-advertised shortcut target"
    Assert-Equal $shortcut.Icon "UsqueProductIcon.ico" "shortcut icon"
    Assert-Equal $shortcut.IconIndex "0" "shortcut icon index"
    Assert-Equal $shortcut.WorkingDirectory "INSTALLFOLDER" "shortcut working directory"

    $shortcutComponent = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Component``,``KeyPath`` FROM ``Component`` WHERE ``Component``='UsqueShortcutComponent'" `
            -Columns @("Component", "KeyPath")) `
        "non-advertised shortcut component"
    Assert-Equal `
        $shortcutComponent.KeyPath `
        "UsqueShortcutKeyPath" `
        "shortcut HKCU registry KeyPath"
    $shortcutKeyPath = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Root``,``Key``,``Name``,``Component_`` FROM ``Registry`` WHERE ``Registry``='UsqueShortcutKeyPath'" `
            -Columns @("Root", "Key", "Name", "Component")) `
        "shortcut registry KeyPath"
    Assert-Equal $shortcutKeyPath.Root "1" "shortcut registry root"
    Assert-Equal $shortcutKeyPath.Key "Software\Usque" "shortcut registry key"
    Assert-Equal $shortcutKeyPath.Component "UsqueShortcutComponent" "shortcut registry component"

    $icon = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Name`` FROM ``Icon`` WHERE ``Name``='UsqueProductIcon.ico'" `
            -Columns @("Name")) `
        "Usque product icon"
    Assert-Equal $icon.Name "UsqueProductIcon.ico" "product icon identifier"
    $iconStreamSize = Get-MsiStreamSize `
        -Database $database `
        -Query "SELECT ``Data`` FROM ``Icon`` WHERE ``Name``='UsqueProductIcon.ico'"
    if ($iconStreamSize -le 0) {
        throw "UsqueProductIcon.ico has no embedded MSI icon stream."
    }

    $serviceInstall = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``ServiceInstall``,``Name``,``ServiceType``,``StartType``,``ErrorControl``,``Arguments``,``Component_`` FROM ``ServiceInstall``" `
            -Columns @(
            "ServiceInstall",
            "Name",
            "ServiceType",
            "StartType",
            "ErrorControl",
            "Arguments",
            "Component"
        )) `
        "Agent ServiceInstall"
    Assert-Equal $serviceInstall.Name "UsqueAgent" "Agent service name"
    Assert-Equal $serviceInstall.ServiceType "16" "Agent service type"
    Assert-Equal $serviceInstall.StartType "3" "Agent service start type"
    Assert-Equal $serviceInstall.ErrorControl "32769" "Agent service vital error control"
    Assert-Equal $serviceInstall.Component "UsqueAgentComponent" "Agent service component"
    Assert-Equal `
        $serviceInstall.Arguments `
        "--service --signer-sha256 $($SignerSha256.ToUpperInvariant())" `
        "Agent service arguments"

    $serviceControl = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``ServiceControl``,``Name``,``Event``,``Wait``,``Component_`` FROM ``ServiceControl``" `
            -Columns @("ServiceControl", "Name", "Event", "Wait", "Component")) `
        "Agent ServiceControl"
    Assert-Equal $serviceControl.Name "UsqueAgent" "controlled service"
    Assert-Equal $serviceControl.Event "162" "service stop/remove events"
    Assert-Equal $serviceControl.Wait "1" "service wait policy"
    Assert-Equal $serviceControl.Component "UsqueAgentComponent" "service control component"

    $servicePermission = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``LockObject``,``Table``,``SDDLText``,``Condition`` FROM ``MsiLockPermissionsEx``" `
            -Columns @("LockObject", "Table", "Sddl", "Condition")) `
        "Agent MsiLockPermissionsEx"
    Assert-Equal `
        $servicePermission.LockObject `
        "UsqueAgentServiceInstall" `
        "Agent permission lock object"
    Assert-Equal $servicePermission.Table "ServiceInstall" "Agent permission table"
    Assert-Equal `
        $servicePermission.Sddl `
        "D:P(A;;0xF01FF;;;SY)(A;;0xF01FF;;;BA)(A;;0x14;;;IU)" `
        "Agent service SDDL"
    Assert-Equal $servicePermission.Condition "" "Agent service SDDL condition"

    $preflightAction = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Action``,``Type``,``Source``,``Target`` FROM ``CustomAction`` WHERE ``Action``='ValidateAgentStartup'" `
            -Columns @("Action", "Type", "Source", "Target")) `
        "ValidateAgentStartup CustomAction"
    Assert-Equal $preflightAction.Type "3090" "Agent startup preflight custom action type"
    Assert-Equal $preflightAction.Source "UsqueAgentExecutable" "Agent startup preflight executable"
    Assert-Equal `
        $preflightAction.Target `
        "--validate-only --signer-sha256 $($SignerSha256.ToUpperInvariant())" `
        "Agent startup preflight command"

    $customAction = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Action``,``Type``,``Source``,``Target`` FROM ``CustomAction`` WHERE ``Action``='RecoverAgentState'" `
            -Columns @("Action", "Type", "Source", "Target")) `
        "RecoverAgentState CustomAction"
    Assert-Equal $customAction.Type "3090" "recovery custom action type"
    Assert-Equal $customAction.Source "UsqueAgentExecutable" "recovery executable"
    Assert-Equal $customAction.Target "--recover-state" "recovery command"

    $purgeAction = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Action``,``Type``,``Source``,``Target`` FROM ``CustomAction`` WHERE ``Action``='PurgeUserData'" `
            -Columns @("Action", "Type", "Source", "Target")) `
        "PurgeUserData CustomAction"
    Assert-Equal $purgeAction.Type "1042" "current-user purge custom action type"
    Assert-Equal $purgeAction.Source "UsqueEngineExecutable" "current-user purge executable"
    Assert-Equal `
        $purgeAction.Target `
        '--config "[LocalAppDataFolder]Usque\config.json" --purge-user-data --preferences-directory "[AppDataFolder]io.github.georgexie2333\Usque"' `
        "current-user purge command"

    $startupAction = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Action``,``Type``,``Source``,``Target`` FROM ``CustomAction`` WHERE ``Action``='RemoveUserStartupRegistration'" `
            -Columns @("Action", "Type", "Source", "Target")) `
        "RemoveUserStartupRegistration CustomAction"
    Assert-Equal $startupAction.Type "1042" "current-user startup cleanup custom action type"
    Assert-Equal $startupAction.Source "UsqueGuiExecutable" "current-user startup cleanup executable"
    Assert-Equal $startupAction.Target "--remove-startup" "current-user startup cleanup command"

    $emergencyAction = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Action``,``Type``,``Source``,``Target`` FROM ``CustomAction`` WHERE ``Action``='EmergencyRemoveKillSwitch'" `
            -Columns @("Action", "Type", "Source", "Target")) `
        "EmergencyRemoveKillSwitch CustomAction"
    Assert-Equal $emergencyAction.Type "3090" "emergency cleanup custom action type"
    Assert-Equal $emergencyAction.Source "UsqueAgentExecutable" "emergency cleanup executable"
    Assert-Equal `
        $emergencyAction.Target `
        "--emergency-remove-kill-switch" `
        "emergency cleanup command"

    $finalizeAction = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Action``,``Type``,``Source``,``Target`` FROM ``CustomAction`` WHERE ``Action``='FinalizeAgentUninstall'" `
            -Columns @("Action", "Type", "Source", "Target")) `
        "FinalizeAgentUninstall CustomAction"
    Assert-Equal $finalizeAction.Type "3090" "uninstall finalization custom action type"
    Assert-Equal $finalizeAction.Source "UsqueAgentExecutable" "uninstall finalization executable"
    Assert-Equal `
        $finalizeAction.Target `
        "--finalize-uninstall" `
        "uninstall finalization command"

    $maintenanceGuard = Assert-OneRow `
    (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Action``,``Type``,``Target`` FROM ``CustomAction`` WHERE ``Action``='RejectUnsupportedMaintenance'" `
            -Columns @("Action", "Type", "Target")) `
        "RejectUnsupportedMaintenance CustomAction"
    Assert-Equal $maintenanceGuard.Type "19" "maintenance rejection custom action type"
    if ([string]::IsNullOrWhiteSpace($maintenanceGuard.Target)) {
        throw "Maintenance rejection must include a user-facing explanation."
    }

    $sequenceRows = Invoke-MsiQuery `
        -Database $database `
        -Query "SELECT ``Action``,``Condition``,``Sequence`` FROM ``InstallExecuteSequence`` WHERE ``Action``='RemoveExistingProducts' OR ``Action``='RejectUnsupportedMaintenance' OR ``Action``='StopServices' OR ``Action``='EmergencyRemoveKillSwitch' OR ``Action``='RecoverAgentState' OR ``Action``='PurgeUserData' OR ``Action``='RemoveUserStartupRegistration' OR ``Action``='FinalizeAgentUninstall' OR ``Action``='DeleteServices' OR ``Action``='RemoveFiles' OR ``Action``='InstallServices' OR ``Action``='ValidateAgentStartup' OR ``Action``='StartServices'" `
        -Columns @("Action", "Condition", "Sequence")
    $sequences = @{}
    foreach ($row in $sequenceRows) {
        $sequences[$row.Action] = $row
    }
    foreach ($requiredAction in @(
            "RemoveExistingProducts",
            "RejectUnsupportedMaintenance",
            "StopServices",
            "EmergencyRemoveKillSwitch",
            "RecoverAgentState",
            "PurgeUserData",
            "RemoveUserStartupRegistration",
            "FinalizeAgentUninstall",
            "DeleteServices",
            "RemoveFiles",
            "InstallServices",
            "ValidateAgentStartup",
            "StartServices"
        )) {
        if (-not $sequences.ContainsKey($requiredAction)) {
            throw "MSI is missing the $requiredAction execute-sequence row."
        }
    }
    Assert-Equal $sequences.RemoveExistingProducts.Sequence "1501" "major-upgrade removal sequence"
    Assert-Equal `
        $sequences.RejectUnsupportedMaintenance.Condition `
        'Installed AND NOT REMOVE~="ALL" AND NOT UPGRADINGPRODUCTCODE' `
        "unsupported maintenance condition"
    if (
        [int]$sequences.RejectUnsupportedMaintenance.Sequence -ge
        [int]$sequences.StopServices.Sequence
    ) {
        throw "Unsupported maintenance must be rejected before StopServices."
    }
    Assert-Equal `
        $sequences.ValidateAgentStartup.Condition `
        'NOT Installed AND NOT REMOVE~="ALL"' `
        "Agent startup preflight condition"
    if (
        [int]$sequences.ValidateAgentStartup.Sequence -le
        [int]$sequences.InstallServices.Sequence -or
        [int]$sequences.ValidateAgentStartup.Sequence -ge
        [int]$sequences.StartServices.Sequence
    ) {
        throw "Agent startup preflight must run after InstallServices and before StartServices."
    }
    Assert-Equal `
        $sequences.EmergencyRemoveKillSwitch.Condition `
        'REMOVE~="ALL"' `
        "emergency cleanup condition"
    Assert-Equal $sequences.RecoverAgentState.Condition 'REMOVE~="ALL"' "recovery condition"
    Assert-Equal `
        $sequences.PurgeUserData.Condition `
        'REMOVE~="ALL" AND NOT UPGRADINGPRODUCTCODE AND USQUE_REMOVE_USER_DATA="1"' `
        "current-user purge condition"
    Assert-Equal `
        $sequences.RemoveUserStartupRegistration.Condition `
        'REMOVE~="ALL" AND NOT UPGRADINGPRODUCTCODE' `
        "current-user startup cleanup condition"
    Assert-Equal `
        $sequences.FinalizeAgentUninstall.Condition `
        'REMOVE~="ALL" AND NOT UPGRADINGPRODUCTCODE' `
        "true-uninstall finalization condition"
    if (
        [int]$sequences.EmergencyRemoveKillSwitch.Sequence -ne
        ([int]$sequences.StopServices.Sequence + 1) -or
        [int]$sequences.RecoverAgentState.Sequence -ne
        ([int]$sequences.EmergencyRemoveKillSwitch.Sequence + 1) -or
        [int]$sequences.PurgeUserData.Sequence -ne
        ([int]$sequences.RecoverAgentState.Sequence + 1) -or
        [int]$sequences.RemoveUserStartupRegistration.Sequence -ne
        ([int]$sequences.PurgeUserData.Sequence + 1) -or
        [int]$sequences.FinalizeAgentUninstall.Sequence -ne
        ([int]$sequences.RemoveUserStartupRegistration.Sequence + 1) -or
        [int]$sequences.FinalizeAgentUninstall.Sequence -ge
        [int]$sequences.DeleteServices.Sequence -or
        [int]$sequences.FinalizeAgentUninstall.Sequence -ge
        [int]$sequences.RemoveFiles.Sequence
    ) {
        throw "Emergency WFP cleanup, detailed recovery, optional user-data purge, startup cleanup, and true-uninstall finalization must run immediately after StopServices and before service/file removal."
    }

    $launchRows = Invoke-MsiQuery `
        -Database $database `
        -Query "SELECT ``Condition``,``Description`` FROM ``LaunchCondition``" `
        -Columns @("Condition", "Description")
    $launchConditions = @($launchRows | ForEach-Object { $_.Condition })
    if (
        $launchConditions -notcontains
        "Installed OR (VersionNT64 AND WINDOWSBUILDNUMBER >= 19045)"
    ) {
        throw "MSI does not enforce Windows 10 22H2 build 19045+."
    }
    if ($launchConditions -notcontains "NOT WIX_DOWNGRADE_DETECTED") {
        throw "MSI does not block downgrades."
    }

    $upgradeRows = Invoke-MsiQuery `
        -Database $database `
        -Query "SELECT ``UpgradeCode``,``VersionMin``,``VersionMax``,``Attributes``,``ActionProperty`` FROM ``Upgrade``" `
        -Columns @("UpgradeCode", "VersionMin", "VersionMax", "Attributes", "ActionProperty")
    $detected = Assert-OneRow `
    @($upgradeRows | Where-Object { $_.ActionProperty -eq "WIX_UPGRADE_DETECTED" }) `
        "WIX_UPGRADE_DETECTED"
    Assert-Equal $detected.UpgradeCode "{076CF387-E447-4666-9153-2DA16049A390}" "detected upgrade code"
    Assert-Equal $detected.VersionMax $ExpectedMsiVersion "upgrade maximum version"
    if (([int]$detected.Attributes -band 512) -eq 0) {
        throw "Equal-version architecture replacement is not enabled."
    }

    $files = Invoke-MsiQuery `
        -Database $database `
        -Query "SELECT ``File``,``FileName``,``Component_`` FROM ``File``" `
        -Columns @("File", "FileName", "Component")
    $longNames = @(
        $files | ForEach-Object {
            if ($_.FileName.Contains("|")) {
                $_.FileName.Split("|", 2)[1]
            }
            else {
                $_.FileName
            }
        }
    )
    foreach ($requiredFile in @(
            "usque.exe",
            "usque-engine.exe",
            "usque-agent.exe",
            "usque-uninstall.exe",
            "wintun.dll"
        )) {
        if (@($longNames | Where-Object { $_ -ieq $requiredFile }).Count -ne 1) {
            throw "MSI must contain exactly one $requiredFile."
        }
    }
    if (@($longNames | Where-Object { $_ -like "*.pdb" }).Count -ne 0) {
        throw "MSI contains a forbidden PDB file."
    }
    if (@($longNames | Where-Object { $_ -ieq "usque_zero_trust_test.exe" }).Count -ne 0) {
        throw "MSI contains the native test executable."
    }

    $components = Invoke-MsiQuery `
        -Database $database `
        -Query "SELECT ``Component``,``Attributes`` FROM ``Component``" `
        -Columns @("Component", "Attributes")
    foreach ($component in $components) {
        if (([int]$component.Attributes -band 256) -eq 0) {
            throw "Component $($component.Component) is not marked 64-bit."
        }
    }
}
finally {
    if ($null -ne $database) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($database)
    }
    if ($null -ne $installer) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
    }
}

Write-Output "MSI table contract verified: $resolvedMsi"
