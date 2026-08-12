[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern("^v?[0-9]+\.[0-9]+\.[0-9]+(?:-beta\.[0-9]+)?$")]
    [string]$SemVer
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($SemVer -notmatch '^v?(?<major>[0-9]+)\.(?<minor>[0-9]+)\.(?<patch>[0-9]+)(?:-beta\.(?<beta>[0-9]+))?$') {
    throw "Unsupported release version: $SemVer"
}

$major = [int]$Matches.major
$minor = [int]$Matches.minor
$patch = [int]$Matches.patch
$ordinal = if ($Matches.ContainsKey("beta")) {
    [int]$Matches.beta
} else {
    99
}

if ($major -gt 255 -or $minor -gt 255) {
    throw "MSI major and minor versions must be at most 255."
}
if ($ordinal -lt 1 -or $ordinal -gt 99) {
    throw "Beta ordinal must be between 1 and 99."
}

$build = ([long]$patch * 100) + $ordinal
if ($build -gt 65535) {
    throw "Mapped MSI build version exceeds 65535."
}

Write-Output "$major.$minor.$build"
