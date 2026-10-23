param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9_-]{11}$')][string]$VideoId,
    [Parameter(Mandatory)][string]$OutputDir,
    [switch]$OpenMutedDefaultOutput,
    [switch]$OneHour
)
$ErrorActionPreference = 'Stop'
if (-not $OpenMutedDefaultOutput) { throw 'Explicit -OpenMutedDefaultOutput is required before opening an audio device.' }
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$output = [IO.Path]::GetFullPath($OutputDir)
if ($output -eq $repo -or $output.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Measurements must be outside the repository.' }
$run = Join-Path $output ('probe-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $run -Force | Out-Null
$stdout = Join-Path $run 'stdout.txt'
$stderr = Join-Path $run 'stderr.txt'
$seconds = if ($OneHour) { 3600 } else { 30 }
$commit = git -C $repo rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Cannot identify source commit.' }
$dirty = [bool](git -C $repo status --porcelain)
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect source state.' }
$record = @{
    state='starting'; requestedPlaybackSeconds=$seconds; startupBudgetSeconds=15
    recordingId=$VideoId; outputSelection='default'; playerVolume=0
    signIn=$false; automaticRetries=$false; systemVolumeChanged=$false
    sourceCommit=$commit; sourceDirty=$dirty
    executableSha256=(Get-FileHash -LiteralPath $Executable).Hash
    requestedAtUtc=[DateTime]::UtcNow.ToString('o')
    caveat='Real-time repeated-recording workload through production preparation and gapless joins. Observed consumption omits unobserved outgoing tails. No audible quality, speaker/lyric latency, other-device, single-long-recording, representative-catalog, UI or account/Premium acceptance. CPU is normalized across logical processors; memory samples belong to the standalone player process.'
}
$record | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $run 'run.json') -Encoding utf8
$arguments = @($VideoId,'--muted-default-output')
if ($OneHour) { $arguments += '--one-hour' }
$process = $null
$writer = $null
$samples = [Collections.Generic.List[object]]::new()
$timedOut = $false
$timer = [Diagnostics.Stopwatch]::StartNew()
$culture = [Globalization.CultureInfo]::InvariantCulture
Write-Output "evidence=$run"
try {
    $process = Start-Process -FilePath $Executable -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $record.processId = $process.Id
    $writer = [IO.StreamWriter]::new((Join-Path $run 'samples.csv'),$false,[Text.UTF8Encoding]::new($false))
    $writer.AutoFlush = $true
    $writer.WriteLine('seconds,privateBytes,workingSetBytes,cpuSeconds')
    $seen = 0
    $heartbeat = 0
    while (-not $process.HasExited) {
        if ($timer.Elapsed.TotalSeconds -gt $seconds + 60) {
            $timedOut = $true
            $process.Kill()
            $process.WaitForExit()
            break
        }
        $process.Refresh()
        if (-not $process.HasExited) {
            $sample = [pscustomobject]@{seconds=$timer.Elapsed.TotalSeconds; privateBytes=$process.PrivateMemorySize64; workingSetBytes=$process.WorkingSet64; cpuSeconds=$process.TotalProcessorTime.TotalSeconds}
            $samples.Add($sample)
            $writer.WriteLine(($sample.seconds.ToString('F3',$culture)+','+$sample.privateBytes.ToString($culture)+','+$sample.workingSetBytes.ToString($culture)+','+$sample.cpuSeconds.ToString('F6',$culture)))
            $lines = @(Get-Content -LiteralPath $stdout)
            if ($lines.Count -gt $seen) { $lines | Select-Object -Skip $seen | Write-Output; $seen = $lines.Count }
            if ($timer.Elapsed.TotalSeconds -ge $heartbeat) {
                $record.state = 'running'
                $record.elapsedProcessSeconds = $timer.Elapsed.TotalSeconds
                $record.currentPrivateMiB = $sample.privateBytes / 1MB
                $record.lastOutput = $lines | Select-Object -Last 1
                $record | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $run 'run.json') -Encoding utf8
                $heartbeat += 30
            }
        }
        Start-Sleep -Milliseconds 500
    }
    $process.WaitForExit()
    $out = Get-Content -LiteralPath $stdout -Raw
    $started = $out -match 'playback_started startup_ms=(\d+)'
    $record.startupMs = if ($started) { [int]$Matches[1] } else { $null }
    $record.audioDeviceOpened = $started
    $record.passed = $process.ExitCode -eq 0 -and -not $timedOut -and $out -match 'muted_default_output_probe=passed'
    $record.state = if ($record.passed) { 'passed' } else { 'failed' }
    if ($out -match 'muted_default_output_probe=passed wall_seconds=([0-9.]+) consumed_delta_seconds=([0-9.]+) buffer_events=(\d+) buffering_ms=(\d+) transitions=(\d+) quality_changes=(\d+)') {
        $record.observedWindowSeconds = [double]::Parse($Matches[1],$culture)
        $record.observedConsumedSeconds = [double]::Parse($Matches[2],$culture)
        $record.bufferEvents = [int]$Matches[3]
        $record.bufferingMs = [int]$Matches[4]
        $record.transitions = [int]$Matches[5]
        $record.qualityChanges = [int]$Matches[6]
    }
    $record.exitCode = $process.ExitCode
    $record.wallBudgetExceeded = $timedOut
    $record.elapsedProcessSeconds = $timer.Elapsed.TotalSeconds
    $record.completedAtUtc = [DateTime]::UtcNow.ToString('o')
    $record.peakPrivateMiB = ($samples.privateBytes | Measure-Object -Maximum).Maximum / 1MB
    $steady = @($samples | Where-Object { $_.seconds -ge 60 })
    if ($steady.Count -ge 2) {
        $first = $steady[0]; $last = $steady[-1]
        $record.privateChangeAfter60sMiB = ($last.privateBytes-$first.privateBytes)/1MB
        $record.steadyCpuPercentMachine = 100*($last.cpuSeconds-$first.cpuSeconds)/($last.seconds-$first.seconds)/[Environment]::ProcessorCount
    }
    $record | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $run 'result.json') -Encoding utf8
    $record | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $run 'run.json') -Encoding utf8
    if (-not $record.passed) { throw "Muted playback failed without retries; see $run" }
    Write-Output $out
    Write-Output "measurement=passed evidence=$run"
} catch {
    if (-not (Test-Path -LiteralPath (Join-Path $run 'result.json'))) {
        $record.state = 'failed'
        $record.passed = $false
        $record.measurementError = $_.Exception.Message
        $record.elapsedProcessSeconds = $timer.Elapsed.TotalSeconds
        $record.completedAtUtc = [DateTime]::UtcNow.ToString('o')
        $record | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $run 'result.json') -Encoding utf8
        $record | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $run 'run.json') -Encoding utf8
    }
    throw
} finally {
    if ($null -ne $process -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    if ($null -ne $writer) { $writer.Dispose() }
}
