param([Parameter(Mandatory)][string]$OutputDir, [string]$GccBin, [switch]$SkipBuild, [ValidateSet('release','debug')][string]$Profile = 'release')
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$output = [IO.Path]::GetFullPath($OutputDir)
if ($output -eq $repo -or $output.StartsWith($repo + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Packages must be written outside the source repository.' }
if (-not $env:CARGO_TARGET_DIR) { throw 'Set CARGO_TARGET_DIR to an external build directory.' }
Push-Location $repo
try {
    if (-not $SkipBuild) {
        if ($Profile -eq 'release') { cargo build -p octave-desktop --release --locked } else { cargo build -p octave-desktop --locked }
        if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }
    }
    $version = '0.1.0-beta-preview'
    $package = Join-Path $output ('Octave-' + $version + '-' + $Profile + '-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
    if (Test-Path -LiteralPath $package) { throw 'Package directory already exists.' }
    New-Item -ItemType Directory -Path $package -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR "$Profile\octave-desktop.exe") -Destination $package
    if (-not $GccBin) { $GccBin = Split-Path (Get-Command gcc -ErrorAction Stop).Source -Parent }
    $objdump = Join-Path $GccBin 'objdump.exe'
    $imports = (& $objdump -p (Join-Path $package 'octave-desktop.exe') | Out-String)
    if ($LASTEXITCODE -ne 0) { throw 'Executable import inspection failed.' }
    # Resolve the small GNU runtime dependency closure; Windows supplies system DLLs.
    for ($pass = 0; $pass -lt 3; $pass++) {
        foreach ($dll in @('libgcc_s_seh-1.dll','libstdc++-6.dll','libwinpthread-1.dll')) {
            if ($imports -match ('DLL Name:\s*' + [regex]::Escape($dll)) -and -not (Test-Path -LiteralPath (Join-Path $package $dll))) {
                $runtime = Join-Path $GccBin $dll
                Copy-Item -LiteralPath $runtime -Destination $package
                $imports += (& $objdump -p $runtime | Out-String)
                if ($LASTEXITCODE -ne 0) { throw 'Runtime import inspection failed.' }
            }
        }
    }
    foreach ($name in @('Install.ps1','Uninstall.ps1')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $package }
    Copy-Item -LiteralPath (Join-Path $repo 'README.md') -Destination $package
    $metadata = cargo metadata --locked --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Dependency inventory failed.' }
    $metadata.packages | Select-Object name,version,license,repository | Export-Csv -LiteralPath (Join-Path $package 'dependencies.csv') -NoTypeInformation
    Set-Content -LiteralPath (Join-Path $package 'BETA-NOTICE.txt') -Value 'Local evaluation build. Unsigned. Account connection uses an advanced memory-only browser session; authenticated playback is not implemented. No public distribution approval or completed license-notice audit is implied. Run octave-desktop.exe for portable use; run Install.ps1 for per-user installation.'
    $files = @(Get-ChildItem -LiteralPath $package -File | ForEach-Object { @{ name=$_.Name; sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash } })
    @{product='Octave'; version=$version; files=$files} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $package 'manifest.json') -Encoding utf8
    Compress-Archive -LiteralPath $package -DestinationPath ($package + '.zip')
    Write-Output $package
} finally { Pop-Location }
