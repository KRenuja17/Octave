param([Parameter(Mandatory)][string]$TestExecutable, [Parameter(Mandatory)][string]$OutputDir, [switch]$Network)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$output = [IO.Path]::GetFullPath($OutputDir)
if ($output -eq $repo -or $output.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Measurements must be outside the repo.' }
New-Item -ItemType Directory -Path $output -Force | Out-Null
$run = Join-Path $output ('pipeline-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $run | Out-Null
$timer = [Diagnostics.Stopwatch]::StartNew()
$filter = if ($Network) { 'playback::soak::live_aac_network_pipeline' } else { 'playback::tests::one_hour_pipeline_soak' }
$marker = if ($Network) { 'network_pipeline=passed' } else { 'pipeline_hour=passed' }
$process = Start-Process -FilePath $TestExecutable -ArgumentList @($filter,'--ignored','--exact','--nocapture') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run 'stdout.txt') -RedirectStandardError (Join-Path $run 'stderr.txt')
$samples = [Collections.Generic.List[object]]::new()
$timedOut = $false
try {
    while (-not $process.HasExited) {
        if ($timer.Elapsed.TotalSeconds -gt 720) {
            $timedOut = $true
            $process.Kill()
            $process.WaitForExit()
            break
        }
        $process.Refresh()
        if (-not $process.HasExited) {
            $samples.Add([pscustomobject]@{seconds=$timer.Elapsed.TotalSeconds;privateBytes=$process.PrivateMemorySize64;workingSetBytes=$process.WorkingSet64;cpuSeconds=$process.TotalProcessorTime.TotalSeconds})
        }
        Start-Sleep -Milliseconds 100
    }
    $process.WaitForExit()
    $samples | Export-Csv -LiteralPath (Join-Path $run 'samples.csv') -NoTypeInformation
    $stdout = Get-Content -LiteralPath (Join-Path $run 'stdout.txt') -Raw
    $passed = $process.ExitCode -eq 0 -and -not $timedOut -and $stdout -match $marker
    $steady = @($samples | Where-Object { $_.seconds -ge 5 })
    $cpu = $null; $change = $null
    if ($steady.Count -ge 2) {
        $first = $steady[0]; $last = $steady[-1]
        $change = ($last.privateBytes-$first.privateBytes)/1MB
        $cpu = 100*($last.cpuSeconds-$first.cpuSeconds)/($last.seconds-$first.seconds)/[Environment]::ProcessorCount
    }
    @{
        workload=$(if ($Network) { 'live-unsigned-AAC-repeated-recording-in-memory-pipeline' } else { 'generated-48000Hz-stereo-hour-through-playback-worker' }); elapsedSeconds=$timer.Elapsed.TotalSeconds
        passed=$passed; exitCode=$process.ExitCode; wallBudgetExceeded=$timedOut; audioDeviceOpened=$false
        requestedAudioSeconds=$(if ($Network -and $env:OCTAVE_NETWORK_SOAK_SECONDS) { [int]$env:OCTAVE_NETWORK_SOAK_SECONDS } else { 3600 })
        peakPrivateMiB=($samples.privateBytes | Measure-Object -Maximum).Maximum / 1MB
        privateChangeAfter5sMiB=$change
        cpuPercentMachine=$cpu
        executableSha256=(Get-FileHash -LiteralPath $TestExecutable).Hash
        sourceCommit=(git -C $repo rev-parse HEAD); sourceDirty=[bool](git -C $repo status --porcelain)
        measuredAtUtc=[DateTime]::UtcNow.ToString('o')
        caveat=$(if ($Network) { 'Accelerated real unsigned AAC from one exact recording repeated through production preparation, EQ, twelve-second crossfade, consumed-clock and bounded queue code. PCM is consumed only in memory; no audio device is opened. No retries. Not a real-time hour, long single recording, physical listening, UI workload or representative-catalog acceptance.' } else { 'Accelerated generated PCM through the real playback worker, EQ, twelve-second tail and rodio queue. No network, AAC decoding, physical output or wall-clock hour. Not live playback acceptance.' })
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run 'measurement.json') -Encoding utf8
    Write-Output $stdout
    Write-Output $run
    if (-not $passed) { throw "Pipeline test failed; see $run" }
} finally {
    if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
}
