param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\Octave'),
    [switch]$NoRegistration
)
$ErrorActionPreference = 'Stop'
$target = [IO.Path]::GetFullPath($InstallDir)
if ((Split-Path $target -Leaf) -ne 'Octave') { throw 'The installation directory must end with Octave.' }
if ($target -eq [IO.Path]::GetPathRoot($target)) { throw 'Cannot install into a drive root.' }
$source = [IO.Path]::GetFullPath($PSScriptRoot)
if ($source -eq $target) { throw 'Extract this package outside the installation directory first.' }
$manifest = Get-Content -LiteralPath (Join-Path $source 'manifest.json') -Raw | ConvertFrom-Json
if ($manifest.product -ne 'Octave') { throw 'Not an Octave package.' }
if ((Test-Path -LiteralPath $target) -and (Get-ChildItem -LiteralPath $target -Force)) {
    $existing = Join-Path $target 'manifest.json'
    if (-not (Test-Path -LiteralPath $existing)) { throw 'Refusing to overwrite a directory that is not an Octave installation.' }
    if ((Get-Content -LiteralPath $existing -Raw | ConvertFrom-Json).product -ne 'Octave') { throw 'The destination belongs to another application.' }
}
foreach ($entry in $manifest.files) {
    if ($entry.name -ne [IO.Path]::GetFileName($entry.name) -or $entry.name -match '[\\/:]' -or $entry.name -in '.', '..') { throw 'Invalid package filename.' }
    $payload = Join-Path $source $entry.name
    if ((Get-FileHash -LiteralPath $payload -Algorithm SHA256).Hash -ne $entry.sha256) { throw "Package checksum mismatch: $($entry.name)" }
}
if (Get-Process -Name octave-desktop -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq (Join-Path $target 'octave-desktop.exe') }) { throw 'Quit Octave before installing or upgrading.' }
New-Item -ItemType Directory -Force -Path $target | Out-Null
foreach ($entry in $manifest.files) { Copy-Item -LiteralPath (Join-Path $source $entry.name) -Destination (Join-Path $target $entry.name) -Force }
Copy-Item -LiteralPath (Join-Path $source 'manifest.json') -Destination (Join-Path $target 'manifest.json') -Force
if (-not $NoRegistration) {
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut((Join-Path ([Environment]::GetFolderPath('Programs')) 'Octave.lnk'))
    $shortcut.TargetPath = Join-Path $target 'octave-desktop.exe'
    $shortcut.WorkingDirectory = $target
    $shortcut.Save()
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Octave'
    New-Item -Path $key -Force | Out-Null
    New-ItemProperty -Path $key -Name DisplayName -Value 'Octave Beta Preview' -Force | Out-Null
    New-ItemProperty -Path $key -Name DisplayVersion -Value $manifest.version -Force | Out-Null
    New-ItemProperty -Path $key -Name InstallLocation -Value $target -Force | Out-Null
    $uninstall = 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "' + (Join-Path $target 'Uninstall.ps1') + '" -InstallDir "' + $target + '"'
    New-ItemProperty -Path $key -Name UninstallString -Value $uninstall -Force | Out-Null
}
Write-Output "Installed Octave to $target. No administrator privileges are needed."
