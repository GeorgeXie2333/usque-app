[CmdletBinding()]
param(
    [string]$Python = "python"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Inert standard-library fixture only. This never reads or modifies an SDK,
# launches Usque, or integrates the candidate patch into a shipping DLL.
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$helper = Join-Path $PSScriptRoot "flutter_windows_timer.py"
$pythonCommand = Get-Command $Python -CommandType Application -ErrorAction Stop
if ($pythonCommand.Source -like "*\Microsoft\WindowsApps\*") {
    throw "Pass -Python with an installed Python executable, not the WindowsApps alias."
}
$runDirectory = Join-Path $repositoryRoot ("target\flutter-windows-timer\run-" + [Guid]::NewGuid().ToString("N"))
& $pythonCommand.Source $helper prepare --output $runDirectory
if ($LASTEXITCODE -ne 0) {
    throw "Timer fixture preparation failed with exit code $LASTEXITCODE."
}

$vswhereCommand = Get-Command vswhere.exe -ErrorAction SilentlyContinue
$vswhere = if ($null -ne $vswhereCommand) {
    $vswhereCommand.Source
}
else {
    Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
}
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
    throw "vswhere.exe was not found: $vswhere"
}
$visualStudioMatch = & $vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or $null -eq $visualStudioMatch) {
    throw "Visual Studio x64 C++ Build Tools were not found."
}
$visualStudio = ([string]$visualStudioMatch).Trim()
$vcvars = Join-Path $visualStudio "VC\Auxiliary\Build\vcvars64.bat"
if ([string]::IsNullOrWhiteSpace($visualStudio) -or -not (Test-Path -LiteralPath $vcvars -PathType Leaf)) {
    throw "vcvars64.bat was not found."
}

# Follow the repository's native build helper: no arch arguments, and retain
# the imported MSVC PATH if the caller supplied both PATH and Path entries.
$environmentLines = & $env:ComSpec /d /s /c "call `"$vcvars`" >nul && set"
if ($LASTEXITCODE -ne 0) {
    throw "vcvars64.bat failed with exit code $LASTEXITCODE."
}
$developerPath = $null
foreach ($line in $environmentLines) {
    $separator = $line.IndexOf('=')
    if ($separator -le 0) {
        continue
    }
    $name = $line.Substring(0, $separator)
    $value = $line.Substring($separator + 1)
    if ($name -ieq "Path") {
        if ($value.IndexOf($visualStudio, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
            $developerPath = $value
        }
        continue
    }
    Set-Item -LiteralPath "Env:$name" -Value $value
}
if ([string]::IsNullOrWhiteSpace($developerPath)) {
    throw "vcvars64.bat did not provide a Visual Studio PATH."
}
Remove-Item -LiteralPath "Env:PATH" -ErrorAction SilentlyContinue
Remove-Item -LiteralPath "Env:Path" -ErrorAction SilentlyContinue
Set-Item -LiteralPath "Env:PATH" -Value $developerPath

$compiler = (Get-Command cl.exe -CommandType Application -ErrorAction Stop).Source
if (-not $compiler.StartsWith($visualStudio + "\", [StringComparison]::OrdinalIgnoreCase)) {
    throw "cl.exe is outside the selected Visual Studio installation."
}
$executable = Join-Path $runDirectory "timer_fixture.exe"
$object = Join-Path $runDirectory "timer_fixture.obj"
$source = Join-Path $runDirectory "timer_fixture.cpp"
$compileLog = Join-Path $runDirectory "compile.log"
# Prevent inherited compiler switches from disabling assertions/warnings or
# redirecting objects/executables outside this run's target directory.
$previousCl = $env:CL
$previousTrailingCl = $env:_CL_
Remove-Item -LiteralPath Env:CL -ErrorAction SilentlyContinue
Remove-Item -LiteralPath Env:_CL_ -ErrorAction SilentlyContinue
Push-Location $runDirectory
try {
    & $compiler /nologo /std:c++17 /EHsc /O2 /W4 /WX /utf-8 "/Fo$object" "/Fe$executable" $source 2>&1 |
        Tee-Object -FilePath $compileLog
    if ($LASTEXITCODE -ne 0) {
        throw "Timer fixture compilation failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
    if ($null -ne $previousCl) {
        $env:CL = $previousCl
    }
    if ($null -ne $previousTrailingCl) {
        $env:_CL_ = $previousTrailingCl
    }
}

$startInfo = New-Object System.Diagnostics.ProcessStartInfo
$startInfo.FileName = $executable
$startInfo.WorkingDirectory = $runDirectory
$startInfo.UseShellExecute = $false
$startInfo.CreateNoWindow = $true
$startInfo.RedirectStandardOutput = $true
$startInfo.RedirectStandardError = $true
$process = New-Object System.Diagnostics.Process
$process.StartInfo = $startInfo
$resultPath = Join-Path $runDirectory "results.jsonl"
$utf8 = New-Object System.Text.UTF8Encoding($false)
$timedOut = $false
try {
    if (-not $process.Start()) {
        throw "Could not start the inert timer fixture."
    }
    # Retain the native handle immediately: WinPS 5 must not lose ExitCode
    # when a redirected short-lived child exits before the next property read.
    $null = $process.Handle
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(30000)) {
        $timedOut = $true
        # Only this owned inert test process; never a product or VM process.
        $process.Kill()
        $process.WaitForExit()
    }
    $nativeExitCode = $process.ExitCode
    [IO.File]::WriteAllText($resultPath, $stdout.GetAwaiter().GetResult(), $utf8)
    [IO.File]::WriteAllText((Join-Path $runDirectory "stderr.log"), $stderr.GetAwaiter().GetResult(), $utf8)
    $runReport = [ordered]@{
        CandidateSdkIntegrated = $false
        EngineGnGtestAndDllIntegration = "not_run"
        CompilerPath = $compiler
        CompilerVersion = (Get-Item -LiteralPath $compiler).VersionInfo.FileVersion
        CompilerSha256 = (Get-FileHash -LiteralPath $compiler -Algorithm SHA256).Hash
        CompilerOptions = @("/std:c++17", "/EHsc", "/O2", "/W4", "/WX", "/utf-8")
        ExecutableSha256 = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash
        NativeExitCode = $nativeExitCode
        TimedOut = $timedOut
        OuterTimeoutSeconds = 30
        HarnessWatchdogSeconds = 20
    }
    [IO.File]::WriteAllText((Join-Path $runDirectory "run.json"), ($runReport | ConvertTo-Json) + "`n", $utf8)
    if ($timedOut -or $nativeExitCode -ne 0) {
        throw "Inert fixture failed (exit=$nativeExitCode, timedOut=$timedOut). Evidence: $runDirectory"
    }
}
finally {
    $process.Dispose()
}
& $pythonCommand.Source $helper validate-results $resultPath
if ($LASTEXITCODE -ne 0) {
    throw "Timer fixture result validation failed with exit code $LASTEXITCODE."
}
Write-Output "Original expected contract failures reproduced; 10 candidate cases passed."
Write-Output "This standalone run does not integrate or validate the SDK or application. Engine GN/GTest/DLL integration in this run: not_run."
Write-Output "Evidence: $runDirectory"
