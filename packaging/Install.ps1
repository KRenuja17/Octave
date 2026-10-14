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
if (-not ($manifest.files | Where-Object { $_.name -eq 'octave-desktop.exe' })) { throw 'Package executable is missing.' }
if (@($manifest.files.name | Sort-Object -Unique).Count -ne @($manifest.files).Count) { throw 'Duplicate package filenames.' }
if (Get-Process -Name octave-desktop -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq (Join-Path $target 'octave-desktop.exe') }) { throw 'Quit Octave before installing or upgrading.' }
$parent = Split-Path $target -Parent
$transactionId = [Guid]::NewGuid().ToString('N')
$stage = [IO.Path]::GetFullPath((Join-Path $parent ('.Octave-stage-' + $transactionId)))
$backup = [IO.Path]::GetFullPath((Join-Path $parent ('.Octave-backup-' + $transactionId)))
$failed = [IO.Path]::GetFullPath((Join-Path $parent ('.Octave-failed-' + $transactionId)))
foreach ($transactionPath in @($stage,$backup,$failed)) {
    if ((Split-Path $transactionPath -Parent) -ne $parent -or (Test-Path -LiteralPath $transactionPath)) { throw 'Unsafe upgrade transaction directory.' }
}
# Refuse junctions/symlinks before copying or moving an existing installation.
if (Test-Path -LiteralPath $target) {
    $items = @((Get-Item -LiteralPath $target -Force)) + @(Get-ChildItem -LiteralPath $target -Force -Recurse)
    if ($items | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) { throw 'Installation contains a reparse point; upgrade refused.' }
}
New-Item -ItemType Directory -Force -Path $stage | Out-Null
if (Test-Path -LiteralPath $target) {
    Get-ChildItem -LiteralPath $target -Force | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $stage -Recurse -Force }
    if (Test-Path -LiteralPath (Join-Path $target 'manifest.json')) {
        $oldManifest = Get-Content -LiteralPath (Join-Path $target 'manifest.json') -Raw | ConvertFrom-Json
        foreach ($oldEntry in $oldManifest.files) {
            if ($oldEntry.name -ne [IO.Path]::GetFileName($oldEntry.name) -or $oldEntry.name -match '[\\/:]' -or $oldEntry.name -in '.', '..') { throw 'Invalid previous manifest filename.' }
            if ($oldEntry.name -notin $manifest.files.name) {
                $obsolete = Join-Path $stage $oldEntry.name
                if (Test-Path -LiteralPath $obsolete -PathType Leaf) { Remove-Item -LiteralPath $obsolete }
            }
        }
    }
}
foreach ($entry in $manifest.files) {
    Copy-Item -LiteralPath (Join-Path $source $entry.name) -Destination (Join-Path $stage $entry.name) -Force
    if ((Get-FileHash -LiteralPath (Join-Path $stage $entry.name) -Algorithm SHA256).Hash -ne $entry.sha256) { throw 'Staged package verification failed; installed version is unchanged.' }
}
Copy-Item -LiteralPath (Join-Path $source 'manifest.json') -Destination (Join-Path $stage 'manifest.json') -Force
$hadPrevious = Test-Path -LiteralPath $target
try {
    if ($hadPrevious) { Move-Item -LiteralPath $target -Destination $backup }
    Move-Item -LiteralPath $stage -Destination $target
    # Activation checkpoint: regression tests inject a failure here.
} catch {
    if (Test-Path -LiteralPath $backup) {
        if (Test-Path -LiteralPath $target) { Move-Item -LiteralPath $target -Destination $failed }
        Move-Item -LiteralPath $backup -Destination $target
    }
    throw 'Upgrade activation failed. The previous installation was restored when available.'
}
if ($hadPrevious) { Write-Output "Previous installation retained for recovery: $backup" }
if (-not $NoRegistration) {
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut((Join-Path ([Environment]::GetFolderPath('Programs')) 'Octave.lnk'))
    $shortcut.TargetPath = Join-Path $target 'octave-desktop.exe'
    $shortcut.WorkingDirectory = $target
    $shortcut.Save()
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Octave'
    New-Item -Path $key -Force | Out-Null
    New-ItemProperty -Path $key -Name DisplayName -Value 'Octave' -Force | Out-Null
    New-ItemProperty -Path $key -Name DisplayVersion -Value $manifest.version -Force | Out-Null
    New-ItemProperty -Path $key -Name InstallLocation -Value $target -Force | Out-Null
    $uninstall = 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "' + (Join-Path $target 'Uninstall.ps1') + '" -InstallDir "' + $target + '"'
    New-ItemProperty -Path $key -Name UninstallString -Value $uninstall -Force | Out-Null
}
Write-Output "Installed Octave to $target. No administrator privileges are needed."
