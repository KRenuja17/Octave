param([Parameter(Mandatory)][string]$Executable, [Parameter(Mandatory)][string]$OutputDir, [ValidateRange(10,3600)][int]$Seconds = 30)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$output = [IO.Path]::GetFullPath($OutputDir)
if ($output -eq $repo -or $output.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Measurements must be outside the repo.' }
New-Item -ItemType Directory -Path $output -Force | Out-Null
$results = @()
foreach ($page in @('home','library')) {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $Executable -ArgumentList @('--prototype','--page',$page) -WindowStyle Hidden -PassThru
    $samples = @()
    $windowMs = $null
    try {
        while ($timer.Elapsed.TotalSeconds -lt $Seconds) {
            $process.Refresh()
            if ($process.HasExited) { throw 'Application exited during measurement.' }
            if ($null -eq $windowMs -and $process.MainWindowHandle -ne 0) { $windowMs = $timer.Elapsed.TotalMilliseconds }
            $samples += [pscustomobject]@{ seconds=$timer.Elapsed.TotalSeconds; privateBytes=$process.PrivateMemorySize64; workingSetBytes=$process.WorkingSet64; cpuSeconds=$process.TotalProcessorTime.TotalSeconds }
            Start-Sleep -Milliseconds 200
        }
        $steady = @($samples | Where-Object { $_.seconds -ge 5 })
        $first = $steady[0]; $last = $steady[-1]
        $results += [pscustomobject]@{ workload="prototype-$page-idle"; durationSeconds=$Seconds; firstWindowHandleMs=$windowMs; peakPrivateMiB=($samples.privateBytes | Measure-Object -Maximum).Maximum / 1MB; finalPrivateMiB=$last.privateBytes / 1MB; privateChangeAfter5sMiB=($last.privateBytes-$first.privateBytes)/1MB; steadyCpuPercentMachine=100*($last.cpuSeconds-$first.cpuSeconds)/($last.seconds-$first.seconds)/[Environment]::ProcessorCount }
        $samples | Export-Csv -LiteralPath (Join-Path $output "$page-samples.csv") -NoTypeInformation
    } finally {
        if (-not $process.HasExited) {
            [void]$process.CloseMainWindow()
            if (-not $process.WaitForExit(5000)) { $process.Kill(); $process.WaitForExit() }
        }
    }
}
$record = @{ cpu=(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors); system=(Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer,Model,TotalPhysicalMemory); os=(Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,BuildNumber); executableSha256=(Get-FileHash -LiteralPath $Executable).Hash; measuredAtUtc=[DateTime]::UtcNow.ToString('o'); caveat='Warm/cached synthetic idle scenes, no audio or interaction. Window handle timing is not visible first-paint timing. CPU is normalized across logical processors. Not long-playback or active browsing acceptance.'; results=$results }
$record | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $output 'measurement.json') -Encoding utf8
$results | Format-Table
