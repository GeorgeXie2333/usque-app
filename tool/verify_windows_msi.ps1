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

    $shortcut = Assert-OneRow `
        (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Shortcut``,``Directory_``,``Name``,``Component_``,``Icon_``,``IconIndex``,``WkDir`` FROM ``Shortcut`` WHERE ``Shortcut``='UsqueStartMenuShortcut'" `
            -Columns @(
                "Shortcut",
                "Directory",
                "Name",
                "Component",
                "Icon",
                "IconIndex",
                "WorkingDirectory"
            )) `
        "Usque Start Menu shortcut"
    Assert-Equal $shortcut.Directory "UsqueProgramMenuFolder" "shortcut directory"
    Assert-Equal $shortcut.Name "Usque" "shortcut name"
    Assert-Equal $shortcut.Component "UsqueGuiComponent" "shortcut component"
    Assert-Equal $shortcut.Icon "UsqueProductIcon.ico" "shortcut icon"
    Assert-Equal $shortcut.IconIndex "0" "shortcut icon index"
    Assert-Equal $shortcut.WorkingDirectory "INSTALLFOLDER" "shortcut working directory"

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
    Assert-Equal $serviceInstall.StartType "2" "Agent service start type"
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
    Assert-Equal $serviceControl.Event "163" "service start/stop/remove events"
    Assert-Equal $serviceControl.Wait "1" "service wait policy"
    Assert-Equal $serviceControl.Component "UsqueAgentComponent" "service control component"

    $customAction = Assert-OneRow `
        (Invoke-MsiQuery `
            -Database $database `
            -Query "SELECT ``Action``,``Type``,``Source``,``Target`` FROM ``CustomAction`` WHERE ``Action``='RecoverAgentState'" `
            -Columns @("Action", "Type", "Source", "Target")) `
        "RecoverAgentState CustomAction"
    Assert-Equal $customAction.Type "3090" "recovery custom action type"
    Assert-Equal $customAction.Source "UsqueAgentExecutable" "recovery executable"
    Assert-Equal $customAction.Target "--recover-state" "recovery command"

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

    $sequenceRows = Invoke-MsiQuery `
        -Database $database `
        -Query "SELECT ``Action``,``Condition``,``Sequence`` FROM ``InstallExecuteSequence`` WHERE ``Action``='RemoveExistingProducts' OR ``Action``='StopServices' OR ``Action``='EmergencyRemoveKillSwitch' OR ``Action``='RecoverAgentState' OR ``Action``='DeleteServices' OR ``Action``='RemoveFiles'" `
        -Columns @("Action", "Condition", "Sequence")
    $sequences = @{}
    foreach ($row in $sequenceRows) {
        $sequences[$row.Action] = $row
    }
    foreach ($requiredAction in @(
        "RemoveExistingProducts",
        "StopServices",
        "EmergencyRemoveKillSwitch",
        "RecoverAgentState",
        "DeleteServices",
        "RemoveFiles"
    )) {
        if (-not $sequences.ContainsKey($requiredAction)) {
            throw "MSI is missing the $requiredAction execute-sequence row."
        }
    }
    Assert-Equal $sequences.RemoveExistingProducts.Sequence "1501" "major-upgrade removal sequence"
    Assert-Equal `
        $sequences.EmergencyRemoveKillSwitch.Condition `
        'REMOVE~="ALL"' `
        "emergency cleanup condition"
    Assert-Equal $sequences.RecoverAgentState.Condition 'REMOVE~="ALL"' "recovery condition"
    if (
        [int]$sequences.EmergencyRemoveKillSwitch.Sequence -ne
            ([int]$sequences.StopServices.Sequence + 1) -or
        [int]$sequences.RecoverAgentState.Sequence -ne
            ([int]$sequences.EmergencyRemoveKillSwitch.Sequence + 1) -or
        [int]$sequences.RecoverAgentState.Sequence -ge
            [int]$sequences.DeleteServices.Sequence -or
        [int]$sequences.RecoverAgentState.Sequence -ge
            [int]$sequences.RemoveFiles.Sequence
    ) {
        throw "Emergency WFP cleanup and detailed recovery must run immediately after StopServices and before service/file removal."
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
        "wintun.dll"
    )) {
        if (@($longNames | Where-Object { $_ -ieq $requiredFile }).Count -ne 1) {
            throw "MSI must contain exactly one $requiredFile."
        }
    }
    if (@($longNames | Where-Object { $_ -like "*.pdb" }).Count -ne 0) {
        throw "MSI contains a forbidden PDB file."
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
