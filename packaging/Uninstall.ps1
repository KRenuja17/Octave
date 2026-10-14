param([string]$InstallDir = $PSScriptRoot, [switch]$NoRegistration)
$ErrorActionPreference = 'Stop'
$target = [IO.Path]::GetFullPath($InstallDir)
if ((Split-Path $target -Leaf) -ne 'Octave') { throw 'Expected the Octave installation directory.' }
if ($target -ne [IO.Path]::GetFullPath($PSScriptRoot)) { throw 'Run the uninstaller inside the installation being removed.' }
if (Get-Process -Name octave-desktop -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq (Join-Path $target 'octave-desktop.exe') }) { throw 'Quit Octave before uninstalling.' }
$manifest = Get-Content -LiteralPath (Join-Path $target 'manifest.json') -Raw | ConvertFrom-Json
if ($manifest.product -ne 'Octave') { throw 'Not an Octave installation.' }
foreach ($entry in $manifest.files) {
    if ($entry.name -ne [IO.Path]::GetFileName($entry.name) -or $entry.name -match '[\\/:]' -or $entry.name -in '.', '..') { throw 'Invalid installed manifest filename.' }
}
foreach ($entry in $manifest.files) {
    $file = Join-Path $target $entry.name
    if (Test-Path -LiteralPath $file -PathType Leaf) { Remove-Item -LiteralPath $file }
}
Remove-Item -LiteralPath (Join-Path $target 'manifest.json')
if (-not $NoRegistration) {
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Octave'
    if ((Get-ItemProperty -Path $key -ErrorAction SilentlyContinue).InstallLocation -eq $target) {
        Remove-Item -LiteralPath $key
        $shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Octave.lnk'
        if (Test-Path -LiteralPath $shortcut) { Remove-Item -LiteralPath $shortcut }
    }
}
# Never recurse: leave any additional user files intact.
if (-not (Get-ChildItem -LiteralPath $target -Force)) { Remove-Item -LiteralPath $target }
Write-Output 'Octave removed. Listening data in LocalAppData\Octave was preserved.'
