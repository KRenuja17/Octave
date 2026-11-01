param([Parameter(Mandatory)][string]$PackageDir, [Parameter(Mandatory)][string]$WorkDir)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$work = [IO.Path]::GetFullPath($WorkDir)
$source = (Resolve-Path -LiteralPath $PackageDir).Path
if ($work -eq $repo -or $work.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase) -or $work -eq $source -or $work.StartsWith($source + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture output must stay outside source and input packages.' }
$root = Join-Path $work ('setup-input-' + [Guid]::NewGuid().ToString('N'))
$fixture = Join-Path $root 'package'
New-Item -ItemType Directory -Path $fixture -Force | Out-Null
Get-ChildItem -LiteralPath $source -File | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $fixture }
$builder = Join-Path $PSScriptRoot 'Build-Setup.ps1'
$manifestPath = Join-Path $fixture 'manifest.json'
$original = [IO.File]::ReadAllText($manifestPath)
function Assert-Rejected([string]$Case) {
    $rejected = $false
    try { & $builder -PackageDir $fixture -OutputDir (Join-Path $root 'output') -ValidateOnly | Out-Host } catch { $rejected = $true }
    if (-not $rejected) { throw "Setup accepted $Case" }
}
& $builder -PackageDir $fixture -OutputDir (Join-Path $root 'output') -ValidateOnly | Out-Host
Set-Content -LiteralPath (Join-Path $fixture 'README.md') -Value 'Synthetic tampered content'
Assert-Rejected 'a changed payload'
Copy-Item -LiteralPath (Join-Path $source 'README.md') -Destination $fixture -Force
Set-Content -LiteralPath (Join-Path $fixture 'unlisted.txt') -Value 'Never package unlisted content'
Assert-Rejected 'unlisted content'
Remove-Item -LiteralPath (Join-Path $fixture 'unlisted.txt')
$manifest = $original | ConvertFrom-Json
$manifest.files += $manifest.files[0]
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifestPath
Assert-Rejected 'duplicate entries'
$manifest.files = @(@{name='../escape.exe';sha256=('0'*64)})
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifestPath
Assert-Rejected 'unsafe entries'
[IO.File]::WriteAllText($manifestPath, $original)
$buildPath = Join-Path $fixture 'BUILD-INFO.json'
$build = Get-Content -LiteralPath $buildPath -Raw | ConvertFrom-Json
$build.dirty = $true
$build | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $buildPath
$manifest = $original | ConvertFrom-Json
($manifest.files | Where-Object name -eq 'BUILD-INFO.json').sha256 = (Get-FileHash -LiteralPath $buildPath).Hash
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifestPath
Assert-Rejected 'a dirty build with matching hashes'
if (Test-Path -LiteralPath (Join-Path $root 'output')) { throw 'Validation changed output files.' }
@{validAccepted=$true; tamperRejected=$true; extraRejected=$true; duplicatesRejected=$true; unsafeRejected=$true; dirtyRejected=$true} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'result.json')
Write-Output "setup_input_policy=passed evidence=$root"
