[CmdletBinding()]
param(
    [string]$ShortcutPath = (
        Join-Path $env:ProgramData "Microsoft\Windows\Start Menu\Programs\Usque\Usque.lnk"
    )
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$resolvedShortcut = (Resolve-Path -LiteralPath $ShortcutPath -ErrorAction Stop).Path
if (-not (Test-Path -LiteralPath $resolvedShortcut -PathType Leaf)) {
    throw "Installed Start Menu shortcut does not exist: $resolvedShortcut"
}

Add-Type -AssemblyName System.Drawing
if (-not ("UsqueShortcutIcon" -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class UsqueShortcutIcon {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct SHFILEINFO {
        public IntPtr hIcon;
        public int iIcon;
        public uint dwAttributes;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)]
        public string szDisplayName;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 80)]
        public string szTypeName;
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SHGetFileInfo(
        string path,
        uint attributes,
        ref SHFILEINFO info,
        uint infoSize,
        uint flags
    );

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool DestroyIcon(IntPtr icon);
}
'@
}

$info = New-Object UsqueShortcutIcon+SHFILEINFO
$result = [UsqueShortcutIcon]::SHGetFileInfo(
    $resolvedShortcut,
    0,
    [ref]$info,
    [Runtime.InteropServices.Marshal]::SizeOf($info),
    0x00000100
)
if ($result -eq [IntPtr]::Zero -or $info.hIcon -eq [IntPtr]::Zero) {
    throw "Windows Shell could not extract the installed Usque shortcut icon."
}

$icon = $null
$bitmap = $null
try {
    $borrowed = [Drawing.Icon]::FromHandle($info.hIcon)
    $icon = $borrowed.Clone()
    $bitmap = $icon.ToBitmap()
    $orangePixels = 0
    for ($y = 0; $y -lt $bitmap.Height; $y++) {
        for ($x = 0; $x -lt $bitmap.Width; $x++) {
            $pixel = $bitmap.GetPixel($x, $y)
            if (
                $pixel.A -ge 128 -and
                $pixel.R -ge 180 -and
                $pixel.G -ge 60 -and
                $pixel.G -le 190 -and
                $pixel.B -le 100
            ) {
                $orangePixels += 1
            }
        }
    }
    $minimumOrangePixels = [Math]::Max(
        8,
        [int](($bitmap.Width * $bitmap.Height) * 0.01)
    )
    if ($orangePixels -lt $minimumOrangePixels) {
        throw (
            "The installed shortcut rendered a generic or blank icon " +
            "(orange pixels: $orangePixels, required: $minimumOrangePixels)."
        )
    }
}
finally {
    if ($null -ne $bitmap) {
        $bitmap.Dispose()
    }
    if ($null -ne $icon) {
        $icon.Dispose()
    }
    [void][UsqueShortcutIcon]::DestroyIcon($info.hIcon)
}

Write-Output "Installed Start Menu shortcut icon verified: $resolvedShortcut"
