[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Root,

    [Parameter(Mandatory = $true)]
    [ValidateSet("x64", "arm64")]
    [string]$Architecture
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$resolvedRoot = (Resolve-Path -LiteralPath $Root -ErrorAction Stop).Path
if (-not (Test-Path -LiteralPath $resolvedRoot -PathType Container)) {
    throw "PE verification root is not a directory: $resolvedRoot"
}

$expectedMachine = if ($Architecture -eq "arm64") {
    [uint16]0xAA64
}
else {
    [uint16]0x8664
}

$binaries = @(
    Get-ChildItem -LiteralPath $resolvedRoot -Recurse -File |
        Where-Object { $_.Extension -in ".exe", ".dll" }
)
if ($binaries.Count -eq 0) {
    throw "No PE binaries were found below $resolvedRoot."
}

foreach ($binary in $binaries) {
    $stream = [System.IO.File]::Open(
        $binary.FullName,
        [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read,
        [System.IO.FileShare]::Read
    )
    $reader = [System.IO.BinaryReader]::new($stream)
    try {
        if ($reader.ReadUInt16() -ne 0x5A4D) {
            throw "Not a DOS/PE binary: $($binary.FullName)"
        }
        $stream.Position = 0x3C
        $peOffset = $reader.ReadUInt32()
        if ($peOffset -gt $stream.Length - 6) {
            throw "Invalid PE header offset in $($binary.FullName)."
        }
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550) {
            throw "Invalid PE signature in $($binary.FullName)."
        }
        $actualMachine = $reader.ReadUInt16()
        if ($actualMachine -ne $expectedMachine) {
            throw ("PE architecture mismatch for {0}: expected 0x{1:X4}, got 0x{2:X4}." -f `
                    $binary.FullName, $expectedMachine, $actualMachine)
        }
    }
    finally {
        $reader.Dispose()
        $stream.Dispose()
    }
}

Write-Output "PE_ARCHITECTURE_OK=$Architecture/$($binaries.Count)"
