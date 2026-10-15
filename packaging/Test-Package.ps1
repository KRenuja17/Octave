param([Parameter(Mandatory)][string]$PackageDir, [Parameter(Mandatory)][string]$PreviousPackageDir, [Parameter(Mandatory)][string]$WorkDir)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$root = [IO.Path]::GetFullPath($WorkDir)
if ($root -eq $repo -or $root.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Package QA must be outside the source repository.' }
foreach ($directory in @($PackageDir,$PreviousPackageDir)) {
    $manifest = Get-Content -LiteralPath (Join-Path $directory 'manifest.json') -Raw | ConvertFrom-Json
    if ($manifest.product -ne 'Octave') { throw 'Expected an Octave package.' }
    foreach ($entry in $manifest.files) {
        if ($entry.name -ne [IO.Path]::GetFileName($entry.name) -or $entry.name -match '[\\/:]' -or $entry.name -in '.', '..') { throw 'Invalid package filename.' }
        if ((Get-FileHash -LiteralPath (Join-Path $directory $entry.name)).Hash -ne $entry.sha256) { throw 'Package checksum mismatch.' }
    }
}
$root = Join-Path $root ('package-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
$target = Join-Path $root 'Octave'
$profile = Join-Path $root 'profile'
function Invoke-Smoke([string]$Executable) {
    $process = Start-Process -FilePath $Executable -ArgumentList @('--smoke-test','--data-dir',('"' + $profile + '"')) -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(30000)) { $process.Kill(); $process.WaitForExit(); throw 'Native smoke test timed out.' }
    if ($process.ExitCode -ne 0) { throw 'Native smoke test failed.' }
}
$savedPath = $env:PATH
try {
    $env:PATH = "$env:SystemRoot\System32;$env:SystemRoot"
    & (Join-Path $PreviousPackageDir 'Install.ps1') -InstallDir $target -NoRegistration
    Invoke-Smoke (Join-Path $target 'octave-desktop.exe')
    $database = Join-Path $profile 'listening.sqlite3'
    $before = (Get-FileHash -LiteralPath $database).Hash
    Set-Content -LiteralPath (Join-Path $target 'user-note.txt') -Value 'preserve'
    & (Join-Path $PackageDir 'Install.ps1') -InstallDir $target -NoRegistration
    if ((Get-FileHash -LiteralPath $database).Hash -ne $before) { throw 'Upgrade changed the profile.' }
    $manifest = Get-Content -LiteralPath (Join-Path $PackageDir 'manifest.json') -Raw | ConvertFrom-Json
    foreach ($entry in $manifest.files) {
        if ((Get-FileHash -LiteralPath (Join-Path $target $entry.name)).Hash -ne $entry.sha256) { throw 'Installed payload differs from package.' }
    }
    Invoke-Smoke (Join-Path $target 'octave-desktop.exe')
    $before = (Get-FileHash -LiteralPath $database).Hash
    & (Join-Path $target 'Uninstall.ps1') -InstallDir $target -NoRegistration
    if (Test-Path -LiteralPath (Join-Path $target 'octave-desktop.exe')) { throw 'Uninstall retained executable.' }
    if ((Get-FileHash -LiteralPath $database).Hash -ne $before -or (Get-Content -LiteralPath (Join-Path $target 'user-note.txt') -Raw).Trim() -ne 'preserve') { throw 'Uninstall changed preserved files.' }
    Invoke-Smoke (Join-Path $PackageDir 'octave-desktop.exe')
    $info = Get-Content -LiteralPath (Join-Path $PackageDir 'BUILD-INFO.json') -Raw | ConvertFrom-Json
    @{ sourceCommit=$info.sourceCommit; version=$manifest.version; packageManifestSha256=(Get-FileHash -LiteralPath (Join-Path $PackageDir 'manifest.json')).Hash; previousManifestSha256=(Get-FileHash -LiteralPath (Join-Path $PreviousPackageDir 'manifest.json')).Hash; upgrade=$true; cleanPathLaunch=$true; portableLaunch=$true; profilePreservation=$true; unrelatedFilePreservation=$true; freshMachine=$false; registrationTested=$false } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'result.json') -Encoding utf8
    Write-Output "package_acceptance=passed evidence=$root"
} finally { $env:PATH = $savedPath }
