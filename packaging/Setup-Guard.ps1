param([Parameter(Mandatory)][string]$InstallDir)
$ErrorActionPreference = 'Stop'
try {
    $target = [IO.Path]::GetFullPath($InstallDir).TrimEnd('\')
    if (-not [IO.Path]::IsPathRooted($InstallDir) -or (Split-Path $target -Leaf) -ne 'Octave' -or $target.StartsWith('\\')) { throw 'Choose a local installation folder ending in Octave.' }
    $profile = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Octave'))
    if ($target -eq $profile -or $target.StartsWith($profile + '\', [StringComparison]::OrdinalIgnoreCase) -or $profile.StartsWith($target + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'The program must be installed separately from your Octave account and music data.' }
    # Do not follow junctions or install into a source checkout, including through parents.
    $parent = $target
    while ($parent) {
        if (Test-Path -LiteralPath (Join-Path $parent '.git')) { throw 'Do not install the application inside a source repository.' }
        if (Test-Path -LiteralPath $parent) {
            $item = Get-Item -LiteralPath $parent -Force
            if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Installation folders must not be files, junctions or symbolic links.' }
        }
        $next = Split-Path $parent -Parent
        if ($next -eq $parent) { break }
        $parent = $next
    }
    if (Test-Path -LiteralPath $target) {
        $items = @(Get-ChildItem -LiteralPath $target -Force)
        if ($items.Count) {
            $manifestPath = Join-Path $target 'manifest.json'
            if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw 'This folder contains other files and is not an Octave installation.' }
            $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
            if ($manifest.product -ne 'Octave' -or 'octave-desktop.exe' -notin @($manifest.files.name)) { throw 'This folder is not a recognized Octave installation.' }
        }
        if (Get-ChildItem -LiteralPath $target -Force -Recurse | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) { throw 'The existing installation contains a junction or symbolic link.' }
    }
    $executable = Join-Path $target 'octave-desktop.exe'
    if (Get-Process -Name octave-desktop -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $executable }) { throw 'Quit Octave, including its system-tray window, before installing, upgrading or uninstalling.' }
    exit 0
} catch {
    # Inno captures this bounded message in its local setup log, never account/profile contents.
    Write-Output $_.Exception.Message
    exit 1
}
