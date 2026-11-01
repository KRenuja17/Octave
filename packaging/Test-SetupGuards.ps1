param([Parameter(Mandatory)][string]$WorkDir)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$work = [IO.Path]::GetFullPath($WorkDir)
if ($work -eq $repo -or $work.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Keep setup tests outside the repository.' }
$root = Join-Path $work ('setup-guards-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root -Force | Out-Null
$guard = Join-Path $PSScriptRoot 'Setup-Guard.ps1'
$powershell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
function Assert-Guard([string]$Target, [bool]$Accepted) {
    $result = & $powershell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $guard -InstallDir $Target 2>&1
    if (($LASTEXITCODE -eq 0) -ne $Accepted) { throw "Unexpected guard result for $Target : $result" }
}
Assert-Guard (Join-Path $root 'fresh\Octave') $true
Assert-Guard (Join-Path $root 'wrong-name') $false
Assert-Guard 'relative\Octave' $false
$unowned = Join-Path $root 'unowned\Octave'
New-Item -ItemType Directory -Path $unowned -Force | Out-Null
Set-Content -LiteralPath (Join-Path $unowned 'keep.txt') -Value 'untouched'
Assert-Guard $unowned $false
$checkout = Join-Path $root 'checkout'
New-Item -ItemType Directory -Path (Join-Path $checkout '.git') -Force | Out-Null
Assert-Guard (Join-Path $checkout 'Octave') $false
$owned = Join-Path $root 'owned\Octave'
New-Item -ItemType Directory -Path $owned -Force | Out-Null
@{product='Other';files=@(@{name='octave-desktop.exe'})} | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $owned 'manifest.json')
Assert-Guard $owned $false
@{product='Octave';files=@(@{name='octave-desktop.exe'})} | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $owned 'manifest.json')
Assert-Guard $owned $true
# A synthetic LocalAppData root, never the user's actual account data.
$savedLocalAppData = $env:LOCALAPPDATA
try {
    $env:LOCALAPPDATA = Join-Path $root 'synthetic-data'
    Assert-Guard (Join-Path $env:LOCALAPPDATA 'Octave') $false
} finally { $env:LOCALAPPDATA = $savedLocalAppData }
$junction = Join-Path $root 'redirect'
New-Item -ItemType Junction -Path $junction -Target (Join-Path $root 'owned') | Out-Null
Assert-Guard (Join-Path $junction 'Octave') $false
# Windows PowerShell 5 prompts on Remove-Item for a populated junction even
# without -Recurse. Remove only the known link, never enumerate/delete its target.
if (-not ((Get-Item -LiteralPath $junction -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Expected the test junction.' }
[IO.Directory]::Delete($junction)
if ((Get-Content -LiteralPath (Join-Path $unowned 'keep.txt')).Trim() -ne 'untouched') { throw 'Guard changed an unrelated file.' }
@{guardCases=9; localDataRead=$false; registryChanged=$false; junctionRejected=$true} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'result.json')
# The final negative native-process check deliberately exits 1. Do not leak it
# into GitHub's PowerShell wrapper after all assertions have passed.
$global:LASTEXITCODE = 0
Write-Output "setup_guards=passed evidence=$root"
