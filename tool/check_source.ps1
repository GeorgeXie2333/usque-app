# Check-only multi-language quality gates for Usque.
# Never rewrites files. Fails clearly when tools are missing or wrong version.
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$ExpectedRuff = "0.16.0"
$ExpectedPssa = "1.25.0"
$ExpectedBuf = "1.72.0"

function Write-Step {
    param([Parameter(Mandatory = $true)][string]$Name)
    Write-Output ""
    Write-Output "==> $Name"
}

function Assert-Command {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Hint
    )
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Required tool '$Name' was not found on PATH. $Hint"
    }
}

function Get-RuffCommand {
    if (Get-Command ruff -ErrorAction SilentlyContinue) {
        return @("ruff")
    }
    if (Get-Command python -ErrorAction SilentlyContinue) {
        $probe = & python -m ruff --version 2>$null
        if ($LASTEXITCODE -eq 0 -and $probe) {
            return @("python", "-m", "ruff")
        }
    }
    if (Get-Command py -ErrorAction SilentlyContinue) {
        $probe = & py -3 -m ruff --version 2>$null
        if ($LASTEXITCODE -eq 0 -and $probe) {
            return @("py", "-3", "-m", "ruff")
        }
    }
    throw "Required tool 'ruff' was not found. Install with: pip install ruff==$ExpectedRuff"
}

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][scriptblock]$Action
    )
    Write-Step $Label
    & $Action
    if ($null -ne $LASTEXITCODE -and $LASTEXITCODE -ne 0) {
        throw "Check failed: $Label (exit $LASTEXITCODE)"
    }
}

Push-Location $RepoRoot
try {
    # --- Rust (host) ---
    Invoke-Checked "Rust format (cargo fmt --check)" {
        Assert-Command cargo "Install the pinned Rust toolchain from rust-toolchain.toml."
        & cargo fmt --all --check
    }
    Invoke-Checked "Rust clippy (-D warnings)" {
        & cargo clippy --workspace --all-targets --locked -- -D warnings
    }

    # --- Dart / Flutter GUI ---
    $flutterRoot = Join-Path $RepoRoot "apps/usque_gui"
    Invoke-Checked "Dart format (check-only)" {
        $dart = $null
        if (Get-Command dart -ErrorAction SilentlyContinue) {
            $dart = (Get-Command dart).Source
        }
        elseif (Get-Command flutter -ErrorAction SilentlyContinue) {
            # Resolve dart from the Flutter SDK layout only (bin/dart[.bat] beside flutter).
            # Do not use `flutter dart format`; it is not a reliable first-class subcommand.
            $flutterBin = (Get-Command flutter).Source
            $flutterDir = Split-Path -Parent $flutterBin
            $candidates = @(
                (Join-Path $flutterDir "dart"),
                (Join-Path $flutterDir "dart.bat"),
                (Join-Path $flutterDir "dart.exe")
            )
            foreach ($candidate in $candidates) {
                if (Test-Path -LiteralPath $candidate) {
                    $dart = $candidate
                    break
                }
            }
            if (-not $dart) {
                throw "Found 'flutter' at $flutterBin but no sibling dart executable. Install a complete Flutter SDK so dart is available on PATH (or next to flutter)."
            }
        }
        if (-not $dart) {
            throw "Required tool 'dart' was not found on PATH. Install the Flutter SDK (which provides dart) or add dart to PATH."
        }
        Push-Location $flutterRoot
        try {
            & $dart format --output=none --set-exit-if-changed lib test
        }
        finally {
            Pop-Location
        }
    }
    Invoke-Checked "Flutter analyze" {
        Assert-Command flutter "Install Flutter and ensure it is on PATH."
        Push-Location $flutterRoot
        try {
            & flutter --suppress-analytics analyze --no-pub
        }
        finally {
            Pop-Location
        }
    }

    # --- Kotlin (ktlint check-only) ---
    Invoke-Checked "Kotlin ktlintCheck" {
        $androidRoot = Join-Path $RepoRoot "apps/usque_gui/android"
        $gradlew = if ($env:OS -eq "Windows_NT") {
            Join-Path $androidRoot "gradlew.bat"
        }
        else {
            Join-Path $androidRoot "gradlew"
        }
        if (-not (Test-Path -LiteralPath $gradlew)) {
            throw "Gradle wrapper not found at $gradlew"
        }
        Push-Location $androidRoot
        try {
            if ($env:OS -eq "Windows_NT") {
                & .\gradlew.bat --no-daemon :app:ktlintCheck
            }
            else {
                & ./gradlew --no-daemon :app:ktlintCheck
            }
        }
        finally {
            Pop-Location
        }
    }

    # --- Python (Ruff) ---
    Invoke-Checked "Python Ruff ($ExpectedRuff)" {
        $ruffCmd = @(Get-RuffCommand)
        $ruffArgs = @()
        if ($ruffCmd.Count -gt 1) {
            $ruffArgs = $ruffCmd[1..($ruffCmd.Count - 1)]
        }
        $versionLine = & $ruffCmd[0] @ruffArgs --version
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to query ruff version."
        }
        $versionText = if ($versionLine -is [array]) { $versionLine -join " " } else { [string]$versionLine }
        if ($versionText -notmatch [regex]::Escape($ExpectedRuff)) {
            throw "Ruff version mismatch: expected $ExpectedRuff, got '$versionText'. Install with: pip install ruff==$ExpectedRuff"
        }
        & $ruffCmd[0] @ruffArgs check tool
        if ($LASTEXITCODE -ne 0) {
            throw "ruff check failed (exit $LASTEXITCODE)"
        }
    }

    # --- PowerShell (PSScriptAnalyzer) ---
    Invoke-Checked "PowerShell PSScriptAnalyzer ($ExpectedPssa)" {
        $module = Get-Module -ListAvailable -Name PSScriptAnalyzer |
            Where-Object { $_.Version.ToString() -eq $ExpectedPssa } |
            Select-Object -First 1
        if (-not $module) {
            throw "PSScriptAnalyzer $ExpectedPssa is required. Install with: Install-Module PSScriptAnalyzer -RequiredVersion $ExpectedPssa -Scope CurrentUser -Force"
        }
        Import-Module $module.Path -Force
        $settings = Join-Path $RepoRoot "tool/PSScriptAnalyzerSettings.psd1"
        $scripts = Get-ChildItem -Path (Join-Path $RepoRoot "tool") -Filter "*.ps1" -File
        $findings = @()
        foreach ($script in $scripts) {
            $findings += Invoke-ScriptAnalyzer -Path $script.FullName -Settings $settings
        }
        if ($findings.Count -gt 0) {
            $findings |
                Format-Table -AutoSize Severity, RuleName, ScriptName, Line, Message |
                Out-String |
                Write-Output
            throw "PSScriptAnalyzer reported $($findings.Count) Error/Warning finding(s)."
        }
        Write-Output "PSScriptAnalyzer: no Error/Warning findings in tool/*.ps1"
    }

    # --- Proto (Buf) ---
    Invoke-Checked "Proto Buf lint/format ($ExpectedBuf)" {
        Assert-Command buf "Install buf $ExpectedBuf (https://buf.build/docs/cli/installation)."
        # buf format invokes the external `diff` utility for --exit-code/--diff.
        # On Windows, prefer Git for Windows' diff when the shell alias shadows it.
        if ($env:OS -eq "Windows_NT") {
            $gitDiffDirs = @(
                "C:\Program Files\Git\usr\bin",
                "C:\Program Files\Git\bin"
            )
            foreach ($dir in $gitDiffDirs) {
                if (Test-Path -LiteralPath (Join-Path $dir "diff.exe")) {
                    $env:Path = "$dir;$env:Path"
                    break
                }
            }
        }
        $bufVersion = (& buf --version 2>&1 | Out-String).Trim()
        if ($bufVersion -notmatch [regex]::Escape($ExpectedBuf)) {
            throw "Buf version mismatch: expected $ExpectedBuf, got '$bufVersion'."
        }
        & buf lint
        if ($LASTEXITCODE -ne 0) {
            throw "buf lint failed (exit $LASTEXITCODE)"
        }
        # Check-only format: exit non-zero if files would change.
        & buf format --exit-code --diff
        if ($LASTEXITCODE -ne 0) {
            throw "buf format check failed (exit $LASTEXITCODE). Run 'buf format -w' to apply pure formatting only."
        }
    }

    Write-Output ""
    Write-Output "All source quality checks passed."
}
finally {
    Pop-Location
}
