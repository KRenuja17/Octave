param(
    [Parameter(Mandatory)][string]$PackageDir,
    [Parameter(Mandatory)][ValidatePattern('^[A-Fa-f0-9]{40}$')][string]$PublisherThumbprint
)
$ErrorActionPreference = 'Stop'
$package = (Resolve-Path -LiteralPath $PackageDir).Path
$manifest = Get-Content -LiteralPath (Join-Path $package 'manifest.json') -Raw | ConvertFrom-Json
if ($manifest.product -ne 'Octave' -or -not $manifest.files) { throw 'Not an Octave package.' }
$names = @($manifest.files.name)
if (@($names | Sort-Object -Unique).Count -ne $names.Count) { throw 'Duplicate package filenames.' }
foreach ($required in @('octave-desktop.exe','octave-signature.exe','Install.ps1','Uninstall.ps1')) {
    if ($required -notin $names) { throw "Missing signed payload: $required" }
}
foreach ($item in Get-ChildItem -LiteralPath $package -Force) {
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or ($item.Name -ne 'manifest.json' -and $item.Name -notin $names)) { throw 'Unexpected or linked package content.' }
}
foreach ($entry in $manifest.files) {
    if ($entry.name -ne [IO.Path]::GetFileName($entry.name) -or $entry.name -match '[\\/:]' -or $entry.name -in '.', '..') { throw 'Invalid package filename.' }
    $file = Join-Path $package $entry.name
    if ((Get-FileHash -LiteralPath $file).Hash -ne $entry.sha256) { throw "Payload hash mismatch: $($entry.name)" }
    if ([IO.Path]::GetExtension($file) -notin '.exe','.ps1') { continue }
    & (Join-Path $PSScriptRoot 'Test-TrustedSignature.ps1') -File $file -PublisherThumbprint $PublisherThumbprint
}
Write-Output 'release_signatures=passed'
