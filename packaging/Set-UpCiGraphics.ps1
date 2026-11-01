param([Parameter(Mandatory)][string]$WorkDir)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'This runtime is only for the disposable GitHub CI environment.' }
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$root = [IO.Path]::GetFullPath($WorkDir)
if ($root -eq $repo -or $root.StartsWith($repo+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'CI graphics must stay outside the checkout and package.' }
New-Item -ItemType Directory -Path $root -Force | Out-Null
$archive = Join-Path $root 'mesa3d-26.2.5-release-msvc.7z'
# Pinned upstream release asset, digest published by GitHub's release API.
# This is a test driver, never a packaged application dependency.
Invoke-WebRequest -Uri 'https://github.com/pal1000/mesa-dist-win/releases/download/26.2.5/mesa3d-26.2.5-release-msvc.7z' -OutFile $archive
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne '997123EF4CE8896204AB912D5B54F19372C487FF9835BCEFAFA2FCA23B8953C2') { throw 'CI graphics archive checksum mismatch.' }
& 7z.exe x $archive ('-o'+$root) 'x64/opengl32.dll' 'x64/libgallium_wgl.dll' -y | Out-Host
if ($LASTEXITCODE -ne 0) { throw 'CI graphics extraction failed.' }
$bin = Join-Path $root 'x64'
foreach ($name in @('opengl32.dll','libgallium_wgl.dll')) {
    if (-not (Test-Path -LiteralPath (Join-Path $bin $name) -PathType Leaf)) { throw 'Pinned CI graphics payload missing.' }
}
Write-Output $bin
