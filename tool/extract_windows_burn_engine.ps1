[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$BundlePath,
    [Parameter(Mandatory = $true)][string]$OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# WiX 5.0.2 burn detach copies the engine prefix without restoring its PE
# signature directory. Reproduce Burn's cache.cpp signed-engine header
# restoration instead; the caller must still verify Authenticode and the pin.
# Format: WiX v5.0.2 Bundles/BurnCommon.cs and src/burn/engine/cache.cpp.
function Read-BurnUInt32 {
    param([IO.BinaryReader]$Reader, [long]$Offset)
    if ($Offset -lt 0 -or $Offset -gt $Reader.BaseStream.Length - 4) {
        throw "Truncated Burn/PE field."
    }
    $Reader.BaseStream.Position = $Offset
    return $Reader.ReadUInt32()
}

$sourcePath = (Resolve-Path -LiteralPath $BundlePath -ErrorAction Stop).Path
$destinationPath = [IO.Path]::GetFullPath($OutputPath)
if (Test-Path -LiteralPath $destinationPath) {
    throw "Burn engine output must not already exist."
}
$source = [IO.File]::Open($sourcePath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
$reader = [IO.BinaryReader]::new($source)
$destination = $null
$writer = $null
$createdOutput = $false
$completed = $false
try {
    if ($source.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5A4D) {
        throw "Burn bundle is not a DOS/PE executable."
    }
    [long]$peOffset = Read-BurnUInt32 $reader 0x3C
    if ($peOffset -lt 64 -or (Read-BurnUInt32 $reader $peOffset) -ne 0x00004550) {
        throw "Invalid Burn PE header."
    }
    $machineAndCount = Read-BurnUInt32 $reader ($peOffset + 4)
    $machine = $machineAndCount -band 0xFFFF
    [long]$sectionCount = $machineAndCount -shr 16
    if ($machine -notin @(0x8664, 0xAA64) -or $sectionCount -lt 1 -or $sectionCount -gt 96) {
        throw "Unsupported Burn PE architecture or section count."
    }
    [long]$optionalSize = (Read-BurnUInt32 $reader ($peOffset + 20)) -band 0xFFFF
    [long]$optionalOffset = $peOffset + 24
    if (
        $optionalSize -lt 152 -or
        ((Read-BurnUInt32 $reader $optionalOffset) -band 0xFFFF) -ne 0x20B -or
        (Read-BurnUInt32 $reader ($optionalOffset + 108)) -lt 5
    ) {
        throw "Burn requires a PE32+ certificate directory."
    }
    [long]$sectionTable = $optionalOffset + $optionalSize
    [long]$sectionTableEnd = $sectionTable + 40 * $sectionCount
    if ($sectionTableEnd -gt $source.Length) { throw "Truncated Burn section table." }

    $burnSections = @()
    for ($index = 0; $index -lt $sectionCount; $index++) {
        $entry = $sectionTable + 40 * $index
        $source.Position = $entry
        $name = [Text.Encoding]::ASCII.GetString($reader.ReadBytes(8))
        if ($name -ceq '.wixburn') {
            $burnSections += @{
                Offset = [long](Read-BurnUInt32 $reader ($entry + 20))
                Size = [long](Read-BurnUInt32 $reader ($entry + 16))
            }
        }
    }
    if ($burnSections.Count -ne 1) { throw "Expected exactly one .wixburn section." }
    [long]$burnOffset = $burnSections[0].Offset
    [long]$burnSize = $burnSections[0].Size
    if ($burnOffset -lt $sectionTableEnd -or $burnSize -lt 56 -or $burnOffset + $burnSize -gt $source.Length) {
        throw "Invalid .wixburn section bounds."
    }
    if (
        (Read-BurnUInt32 $reader $burnOffset) -ne 0x00F14300 -or
        (Read-BurnUInt32 $reader ($burnOffset + 4)) -ne 2 -or
        (Read-BurnUInt32 $reader ($burnOffset + 40)) -ne 1
    ) {
        throw "Unsupported .wixburn magic, version, or container format."
    }
    [long]$stubSize = Read-BurnUInt32 $reader ($burnOffset + 24)
    $originalChecksum = Read-BurnUInt32 $reader ($burnOffset + 28)
    [long]$signatureOffset = Read-BurnUInt32 $reader ($burnOffset + 32)
    [long]$signatureSize = Read-BurnUInt32 $reader ($burnOffset + 36)
    [long]$containerCount = Read-BurnUInt32 $reader ($burnOffset + 44)
    [long]$uxSize = Read-BurnUInt32 $reader ($burnOffset + 48)
    if ($containerCount -lt 2 -or 48 + 4 * $containerCount -gt $burnSize) {
        throw "Invalid Burn attached-container count."
    }
    [long]$engineSize = $signatureOffset + $signatureSize
    if (
        $stubSize -lt $burnOffset + $burnSize -or $uxSize -eq 0 -or
        $signatureOffset -lt $stubSize + $uxSize -or $signatureSize -lt 8 -or
        $signatureOffset % 8 -ne 0 -or $signatureSize % 8 -ne 0 -or
        $engineSize -gt $source.Length
    ) {
        throw "Missing or invalid embedded Burn engine signature bounds."
    }
    [long]$certificateLength = Read-BurnUInt32 $reader $signatureOffset
    if (
        $certificateLength -lt 8 -or $certificateLength -gt $signatureSize -or
        (Read-BurnUInt32 $reader ($signatureOffset + 4)) -ne 0x00020200
    ) {
        throw "Invalid embedded Burn WIN_CERTIFICATE header."
    }
    [long]$payloadEnd = $engineSize
    for ($index = 1; $index -lt $containerCount; $index++) {
        $payloadEnd += Read-BurnUInt32 $reader ($burnOffset + 48 + 4 * $index)
    }
    if ($payloadEnd -gt $source.Length) { throw "Truncated Burn attached payload." }

    # Never overwrite the bundle or an existing file. Open the source read-only
    # throughout parsing/copying so another writer cannot race the validated data.
    $destination = [IO.File]::Open($destinationPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    $createdOutput = $true
    $source.Position = 0
    $buffer = [byte[]]::new(65536)
    [long]$remaining = $engineSize
    while ($remaining -gt 0) {
        $count = $source.Read($buffer, 0, [int][Math]::Min($buffer.Length, $remaining))
        if ($count -eq 0) { throw "Truncated Burn engine while copying." }
        $destination.Write($buffer, 0, $count)
        $remaining -= $count
    }
    $writer = [IO.BinaryWriter]::new($destination)
    $destination.Position = $optionalOffset + 64
    $writer.Write([uint32]$originalChecksum)
    $destination.Position = $optionalOffset + 144
    $writer.Write([uint32]$signatureOffset)
    $writer.Write([uint32]$signatureSize)
    $destination.Position = $burnOffset + 28
    $writer.Write([byte[]]::new(12))
    $writer.Flush()
    $completed = $true
}
finally {
    if ($null -ne $writer) { $writer.Dispose() }
    if ($null -ne $destination) { $destination.Dispose() }
    $reader.Dispose()
    $source.Dispose()
    if ($createdOutput -and -not $completed) {
        Remove-Item -LiteralPath $destinationPath -Force
    }
}
Write-Output $destinationPath
