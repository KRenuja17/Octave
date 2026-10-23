param([Parameter(Mandatory)][string]$TestExecutable, [Parameter(Mandatory)][string]$OutputDir)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$output = [IO.Path]::GetFullPath($OutputDir)
if ($output -eq $repo -or $output.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Measurements must be outside the repo.' }
New-Item -ItemType Directory -Path $output -Force | Out-Null
$run = Join-Path $output ('pipeline-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $run | Out-Null
$timer = [Diagnostics.Stopwatch]::StartNew()
$process = Start-Process -FilePath $TestExecutable -ArgumentList @('playback::tests::one_hour_pipeline_soak','--ignored','--exact','--nocapture') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run 'stdout.txt') -RedirectStandardError (Join-Path $run 'stderr.txt')
$samples = [Collections.Generic.List[object]]::new()
try {
    while (-not $process.HasExited) {
        if ($timer.Elapsed.TotalSeconds -gt 660) { throw 'Pipeline measurement timed out.' }
        $process.Refresh()
        if (-not $process.HasExited) {
            $samples.Add([pscustomobject]@{seconds=$timer.Elapsed.TotalSeconds;privateBytes=$process.PrivateMemorySize64;workingSetBytes=$process.WorkingSet64;cpuSeconds=$process.TotalProcessorTime.TotalSeconds})
        }
        Start-Sleep -Milliseconds 100
    }
    $process.WaitForExit()
    $samples | Export-Csv -LiteralPath (Join-Path $run 'samples.csv') -NoTypeInformation
    $stdout = Get-Content -LiteralPath (Join-Path $run 'stdout.txt') -Raw
    if ($process.ExitCode -ne 0 -or $stdout -notmatch 'pipeline_hour=passed') { throw "Pipeline test failed; see $run" }
    $steady = @($samples | Where-Object { $_.seconds -ge 5 })
    if ($steady.Count -lt 2) { throw 'Not enough steady-state samples.' }
    $first = $steady[0]; $last = $steady[-1]
    @{
        workload='generated-48000Hz-stereo-hour-through-playback-worker'; elapsedSeconds=$timer.Elapsed.TotalSeconds
        peakPrivateMiB=($samples.privateBytes | Measure-Object -Maximum).Maximum / 1MB
        privateChangeAfter5sMiB=($last.privateBytes-$first.privateBytes)/1MB
        cpuPercentMachine=100*($last.cpuSeconds-$first.cpuSeconds)/($last.seconds-$first.seconds)/[Environment]::ProcessorCount
        executableSha256=(Get-FileHash -LiteralPath $TestExecutable).Hash
        measuredAtUtc=[DateTime]::UtcNow.ToString('o')
        caveat='Accelerated generated PCM through the real playback worker, EQ, twelve-second tail and rodio queue. No network, AAC decoding, physical output or wall-clock hour. Not live playback acceptance.'
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run 'measurement.json') -Encoding utf8
    Write-Output $stdout
    Write-Output $run
} finally {
    if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
}
