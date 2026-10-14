param([Parameter(Mandatory)][string]$WorkDir)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$root = [IO.Path]::GetFullPath($WorkDir)
if ($root -eq $repo -or $root.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Test output must be outside the repo.' }
$root = Join-Path $root ('installer-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root -Force | Out-Null
$target = Join-Path $root 'Octave'
function New-Fixture([string]$Name, [switch]$FailActivation) {
    $directory = Join-Path $root $Name
    New-Item -ItemType Directory -Path $directory | Out-Null
    $script = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Install.ps1') -Raw
    if ($FailActivation) { $script = $script.Replace('# Activation checkpoint: regression tests inject a failure here.', "throw 'Simulated activation failure'") }
    Set-Content -LiteralPath (Join-Path $directory 'Install.ps1') -Value $script
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'Uninstall.ps1') -Destination $directory
    Set-Content -LiteralPath (Join-Path $directory 'octave-desktop.exe') -Value $Name
    if ($Name -eq 'v1') { Set-Content -LiteralPath (Join-Path $directory 'obsolete.txt') -Value 'old managed file' }
    $files = @(Get-ChildItem -LiteralPath $directory -File | ForEach-Object { @{name=$_.Name; sha256=(Get-FileHash -LiteralPath $_.FullName).Hash} })
    @{product='Octave'; version=$Name; files=$files} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $directory 'manifest.json')
    return $directory
}
$v1 = New-Fixture 'v1'
& (Join-Path $v1 'Install.ps1') -InstallDir $target -NoRegistration
Set-Content -LiteralPath (Join-Path $target 'user-note.txt') -Value 'keep'
$v2 = New-Fixture 'v2'
& (Join-Path $v2 'Install.ps1') -InstallDir $target -NoRegistration
if ((Get-Content -LiteralPath (Join-Path $target 'octave-desktop.exe') -Raw).Trim() -ne 'v2') { throw 'Upgrade failed.' }
if (Test-Path -LiteralPath (Join-Path $target 'obsolete.txt')) { throw 'Obsolete package file retained.' }
Set-Content -LiteralPath (Join-Path $v2 'octave-desktop.exe') -Value 'tampered'
$rejected = $false
try { & (Join-Path $v2 'Install.ps1') -InstallDir $target -NoRegistration } catch { $rejected = $true }
if (-not $rejected) { throw 'Tampered package accepted.' }
$broken = New-Fixture 'v3' -FailActivation
$rolledBack = $false
try { & (Join-Path $broken 'Install.ps1') -InstallDir $target -NoRegistration } catch { $rolledBack = $true }
if (-not $rolledBack -or (Get-Content -LiteralPath (Join-Path $target 'octave-desktop.exe') -Raw).Trim() -ne 'v2') { throw 'Activation rollback failed.' }
if ((Get-Content -LiteralPath (Join-Path $target 'user-note.txt') -Raw).Trim() -ne 'keep') { throw 'Unrelated file lost.' }
& (Join-Path $target 'Uninstall.ps1') -InstallDir $target -NoRegistration
if (Test-Path -LiteralPath (Join-Path $target 'octave-desktop.exe')) { throw 'Uninstall failed.' }
if (-not (Test-Path -LiteralPath (Join-Path $target 'user-note.txt'))) { throw 'Uninstall removed unrelated file.' }
Write-Output "installer_integrity_upgrade_rollback_preservation=passed evidence=$root"
