param(
    [Parameter(Mandatory)][string]$PlaybackExecutable,
    [Parameter(Mandatory)][string]$InspectorExecutable,
    [Parameter(Mandatory)][string]$Manifest,
    [Parameter(Mandatory)][string]$OutputDir,
    [switch]$OpenMutedDefaultOutput
)
$ErrorActionPreference='Stop'
if (-not $OpenMutedDefaultOutput) { throw 'Explicit -OpenMutedDefaultOutput is required.' }
$repo=[IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$output=[IO.Path]::GetFullPath($OutputDir)
if ($output -eq $repo -or $output.StartsWith($repo+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Matrix evidence must be outside the repository.' }
$cases=@(Get-Content -LiteralPath $Manifest -Raw | ConvertFrom-Json)
if ($cases.Count -lt 2 -or $cases.Count -gt 20) { throw 'Supply 2 to 20 predeclared cases.' }
$ids=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($case in $cases) {
    if ($case.videoId -notmatch '\A[A-Za-z0-9_-]{11}\z' -or -not $ids.Add($case.videoId) -or $case.role -notin @('sample','long') -or -not $case.category) { throw 'Invalid or duplicate case.' }
}
if (-not @($cases | Where-Object {$_.role -eq 'long'}).Count) { throw 'A long-recording case is required.' }
$commit=git -C $repo rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Cannot identify source.' }
$dirty=[bool](git -C $repo status --porcelain)
if ($LASTEXITCODE -ne 0 -or $dirty) { throw 'Commit validated source before measuring.' }
$run=Join-Path $output ('matrix-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $run -Force | Out-Null
Copy-Item -LiteralPath $Manifest -Destination (Join-Path $run 'cases.json')
$record=@{
    state='inspecting'; sourceCommit=$commit; sourceDirty=$false
    manifestSha256=(Get-FileHash -LiteralPath $Manifest).Hash
    playbackSha256=(Get-FileHash -LiteralPath $PlaybackExecutable).Hash
    inspectorSha256=(Get-FileHash -LiteralPath $InspectorExecutable).Hash
    requestedAtUtc=[DateTime]::UtcNow.ToString('o')
    outputSelection='default'; playerVolume=0; signIn=$false
    automaticPlaybackRetries=$false; recordingSubstitution=$false
    transportReconnectPolicy='One network-only replay within original request deadline.'
    protocol='One inspection per row; accessible rows get 30s start/middle/late windows; stop a row after failure. First accessible long row >=3660s gets one continuous non-repeating hour; never switch after selection. 90min overall bound. Access failures remain capability failures.'
    executionComplete=$false; capabilityAccepted=$false; cases=@()
}
$timer=[Diagnostics.Stopwatch]::StartNew()
$rows=[Collections.Generic.List[object]]::new()
function Save-State {
    $record.cases=@($rows.ToArray())
    $record.elapsedSeconds=$timer.Elapsed.TotalSeconds
    $record | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $run 'run.json') -Encoding utf8
}
Write-Output "matrix_evidence=$run"
foreach ($case in $cases) {
    $row=[pscustomobject]@{videoId=$case.videoId; category=$case.category; role=$case.role; state='not_run'; durationSeconds=$null; inspectionOutcome=$null; inspectionExitCode=$null; stages=@(); evidence=$null}
    $rows.Add($row)
    Save-State
    if ($timer.Elapsed.TotalSeconds -gt 5400) { $row.state='budget_not_run'; continue }
    $directory=Join-Path $run ('case-'+$rows.Count.ToString('D2')+'-'+$case.videoId)
    New-Item -ItemType Directory -Path $directory | Out-Null
    $row.evidence=$directory
    $stdout=Join-Path $directory 'inspection.stdout.txt'; $stderr=Join-Path $directory 'inspection.stderr.txt'
    $process=$null
    try {
        $process=Start-Process -FilePath $InspectorExecutable -ArgumentList @($case.videoId,'--inspect') -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        if (-not $process.WaitForExit(45000)) {
            $process.Kill(); $process.WaitForExit(); $row.inspectionOutcome='OuterTimeout'; $row.state='inspection_failed'
        } else {
            $text=Get-Content -LiteralPath $stdout -Raw
            if ($process.ExitCode -eq 0 -and $text -match 'stream_inspection=passed duration_seconds=(\d+) content_bytes=(\d+) bitrate_bps=(\d+)') {
                $row.durationSeconds=[long]$Matches[1]
                $row.inspectionOutcome='ResolvedDirectAac'; $row.state='accessible'
            } else {
                $errorText=Get-Content -LiteralPath $stderr -Raw
                $row.inspectionOutcome=if ($errorText -match 'resolve_error=([A-Za-z]+)(?:\((\d+)\))?') { $Matches[1] + $(if ($Matches[2]) { ' ('+$Matches[2]+')' }) } else { 'UnclassifiedFailure' }
                $row.state='access_failed'
            }
        }
        $row.inspectionExitCode=$process.ExitCode
    } finally {
        if ($process -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    }
    Write-Output "matrix_inspection id=$($row.videoId) outcome=$($row.inspectionOutcome) duration_seconds=$($row.durationSeconds) audio_device_opened=false"
    Save-State
    Start-Sleep -Milliseconds 1000
}
$selected=@($rows | Where-Object {$_.role -eq 'long' -and $_.state -eq 'accessible' -and $_.durationSeconds -ge 3660} | Select-Object -First 1)
$record.selectedLongId=if ($selected.Count) {$selected[0].videoId} else {$null}
$record.state='playing'; Save-State
foreach ($row in $rows) {
    if ($row.state -ne 'accessible') { continue }
    if ($row.durationSeconds -lt 35) { $row.state='insufficient_duration'; Save-State; continue }
    $windows=@(
        @{name='start'; offset=0; hour=$false},
        @{name='middle'; offset=[int][Math]::Floor($row.durationSeconds/2); hour=$false},
        @{name='late'; offset=[int]($row.durationSeconds-35); hour=$false}
    )
    if ($row.videoId -eq $record.selectedLongId) { $windows+=@{name='single-hour';offset=0;hour=$true} }
    foreach ($window in $windows) {
        if ($timer.Elapsed.TotalSeconds + $(if ($window.hour) {3660} else {90}) -gt 5400) { $row.state='budget_not_run'; break }
        $stageRoot=Join-Path $row.evidence $window.name
        $arguments=@{Executable=$PlaybackExecutable; VideoId=$row.videoId; OutputDir=$stageRoot; OpenMutedDefaultOutput=$true}
        if ($window.hour) { $arguments.SingleHour=$true } else { $arguments.OffsetSeconds=$window.offset }
        Write-Output "matrix_playback id=$($row.videoId) stage=$($window.name) offset_seconds=$($window.offset) output=default player_volume=0"
        try { & (Join-Path $PSScriptRoot 'Measure-MutedPlayback.ps1') @arguments | Write-Output }
        catch { Write-Output "matrix_stage_failed id=$($row.videoId) stage=$($window.name)" }
        $files=@(Get-ChildItem -LiteralPath $stageRoot -Recurse -File -Filter result.json)
        if ($files.Count -ne 1) { throw 'Measurement did not produce exactly one retained result.' }
        $result=Get-Content -LiteralPath $files[0].FullName -Raw | ConvertFrom-Json
        $row.stages+=@{name=$window.name;offsetSeconds=$window.offset;passed=[bool]$result.passed;result=$files[0].FullName}
        if (-not $result.passed) { $row.state='playback_failed'; break }
    }
    if ($row.state -eq 'accessible') { $row.state=if ($row.videoId -eq $record.selectedLongId) {'long_hour_passed'} else {'sampled_pass'} }
    Save-State
}
$record.executionComplete=-not [bool]@($rows | Where-Object {$_.state -in @('not_run','accessible','budget_not_run')}).Count
$record.longHourPassed=[bool]@($rows | Where-Object {$_.state -eq 'long_hour_passed'}).Count
$record.capabilityAccepted=$record.longHourPassed -and -not [bool]@($rows | Where-Object {$_.state -notin @('sampled_pass','long_hour_passed')}).Count
$record.state=if ($record.capabilityAccepted -and $record.longHourPassed) {'passed'} else {'completed_with_failures'}
$record.completedAtUtc=[DateTime]::UtcNow.ToString('o')
Save-State
$record | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $run 'result.json') -Encoding utf8
Write-Output "matrix_complete state=$($record.state) execution_complete=$($record.executionComplete) capability_accepted=$($record.capabilityAccepted) long_hour_passed=$($record.longHourPassed) evidence=$run"
