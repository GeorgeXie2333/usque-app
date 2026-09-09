# Embedded in the MSI QuietUninstallString, not executed from an installed file.
# A system-owned host can wait without keeping an MSI-owned image mapped.
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-UsqueInstalledHelperPath {
    $machine = [Microsoft.Win32.RegistryKey]::OpenBaseKey(
        [Microsoft.Win32.RegistryHive]::LocalMachine,
        [Microsoft.Win32.RegistryView]::Registry64
    )
    $product = $null
    try {
        $product = $machine.OpenSubKey('Software\Usque')
        if ($null -eq $product) { throw "Usque is not registered." }
        $directory = $product.GetValue(
            "InstallLocation",
            $null,
            [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames
        )
        if ($directory -isnot [string] -or -not [IO.Path]::IsPathRooted($directory)) {
            throw "Usque has no absolute installation directory."
        }
        return [IO.Path]::GetFullPath([IO.Path]::Combine($directory, "usque-uninstall.exe"))
    }
    finally {
        if ($null -ne $product) { $product.Dispose() }
        $machine.Dispose()
    }
}

function Get-UsqueQuietCopyPath {
    return [IO.Path]::Combine([IO.Path]::GetTempPath(), "UsqueUninstall-$PID", "usque-uninstall.exe")
}

function Invoke-UsqueQuietChild {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string]$Arguments
    )
    $process = [Diagnostics.Process]::new()
    try {
        # FileName is a path, never PowerShell or cmd source. Arguments consist
        # only of fixed switches and the numeric host PID, not registry data.
        $process.StartInfo.FileName = $FilePath
        $process.StartInfo.Arguments = $Arguments
        $process.StartInfo.UseShellExecute = $false
        $process.StartInfo.CreateNoWindow = $true
        $process.StartInfo.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
        if (-not $process.Start()) { throw "Could not start the uninstall helper." }
        $process.WaitForExit()
        return $process.ExitCode
    }
    finally {
        $process.Dispose()
    }
}

function Invoke-UsqueQuietUninstall {
    $installed = Get-UsqueInstalledHelperPath
    $copy = Get-UsqueQuietCopyPath
    $stageCode = Invoke-UsqueQuietChild -FilePath $installed -Arguments "--stage-quiet=$PID"
    if ($stageCode -ne 0) { return $stageCode }

    $copyLock = $null
    try {
        # Reverify after acquiring this lock, and retain it through execution.
        # The staged signed file cannot be changed or deleted between the
        # installed verifier exiting and the temporary worker being loaded.
        $copyLock = [IO.File]::Open($copy, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
        $verifyCode = Invoke-UsqueQuietChild -FilePath $installed -Arguments "--verify-quiet=$PID"
        if ($verifyCode -ne 0) { return $verifyCode }
        # Both installed-image processes have exited before MSI can remove
        # their files. No --wait-for-pid cycle or asynchronous success exists.
        return Invoke-UsqueQuietChild -FilePath $copy -Arguments "--quiet"
    }
    finally {
        if ($null -ne $copyLock) { $copyLock.Dispose() }
        try {
            # Remove only the staged file and its empty directory, never a
            # recursive tree. Cleanup failure must not hide the uninstall code.
            [IO.File]::Delete($copy)
            [IO.Directory]::Delete([IO.Path]::GetDirectoryName($copy), $false)
        }
        catch {
            Write-Warning "The temporary uninstall helper could not be removed."
        }
    }
}

# Dot-sourcing is for deterministic tests with process/registry doubles only.
if ($MyInvocation.InvocationName -ne '.') {
    try {
        exit (Invoke-UsqueQuietUninstall)
    }
    catch {
        Write-Error "Quiet uninstall failed: $_" -ErrorAction Continue
        exit 1
    }
}
