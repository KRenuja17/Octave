param([Parameter(Mandatory)][string]$OutputDir, [string]$GccBin, [switch]$SkipBuild, [switch]$AllowDirty, [ValidatePattern('^\d+\.\d+\.\d+(-[A-Za-z0-9.]+)?$')][string]$Version = '0.1.0-rc.1', [ValidateSet('release','debug')][string]$Profile = 'release')
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$output = [IO.Path]::GetFullPath($OutputDir)
if ($output -eq $repo -or $output.StartsWith($repo + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Packages must be written outside the source repository.' }
if (-not $env:CARGO_TARGET_DIR) { throw 'Set CARGO_TARGET_DIR to an external build directory.' }
$buildRoot = [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
if ($buildRoot -eq $repo -or $buildRoot.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Build output must be outside the repo.' }
if ($env:CARGO_BUILD_TARGET) { throw 'Use the native x64 GNU host without a CARGO_BUILD_TARGET override.' }
$compiler = rustc -vV
if ($LASTEXITCODE -ne 0 -or -not ($compiler -contains 'host: x86_64-pc-windows-gnu')) { throw 'The package requires the Windows x64 GNU Rust host toolchain.' }
Push-Location $repo
try {
    $commit = git rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Cannot identify source commit.' }
    $dirty = [bool](git status --porcelain)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect source state.' }
    if ($dirty -and -not $AllowDirty) { throw 'Commit validated changes first, or use -AllowDirty for a development package.' }
    if (-not $SkipBuild) {
        if ($Profile -eq 'release') { cargo build -p octave-desktop --release --locked } else { cargo build -p octave-desktop --locked }
        if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }
    }
    $version = $Version
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
    foreach ($name in @('LICENSE','RELEASE.md')) { Copy-Item -LiteralPath (Join-Path $repo $name) -Destination $package }
    $metadata = cargo metadata --locked --format-version 1 --filter-platform x86_64-pc-windows-gnu | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Dependency inventory failed.' }
    $dependencies = @($metadata.packages | Where-Object { $_.source -and $_.id -in $metadata.resolve.nodes.id } | Sort-Object name,version)
    $dependencies | Select-Object name,version,license,repository | Export-Csv -LiteralPath (Join-Path $package 'dependencies.csv') -NoTypeInformation
    $notices = [Text.StringBuilder]::new()
    [void]$notices.AppendLine('Octave third-party notices. Target-filtered workspace dependency superset, including test/build tools. SPDX alternatives are retained; this inventory is not a license clearance opinion.')
    [void]$notices.AppendLine('Octave fixture artwork is original project artwork under MIT. Windows system fonts are loaded from the OS and are not redistributed.')
    $missing = @()
    foreach ($dependency in $dependencies) {
        [void]$notices.AppendLine("`n===== $($dependency.name) $($dependency.version) | $($dependency.license) | $($dependency.repository) =====")
        $root = Split-Path $dependency.manifest_path -Parent
        $licenses = @(Get-ChildItem -LiteralPath $root -Recurse -File | Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE|OFL|UFL)([._-]|$)' -or ($dependency.license_file -and $_.FullName -eq (Join-Path $root $dependency.license_file)) } | Sort-Object FullName -Unique)
        if (-not $licenses.Count) { $missing += "$($dependency.name) $($dependency.version)"; [void]$notices.AppendLine('No packaged license text found; manual review required.') }
        foreach ($license in $licenses) {
            if ($license.Length -gt 1MB) { throw "License file needs manual review: $($dependency.name)" }
            [void]$notices.AppendLine("--- $($license.FullName.Substring($root.Length + 1)) ---")
            [void]$notices.AppendLine([IO.File]::ReadAllText($license.FullName))
        }
    }
    [IO.File]::WriteAllText((Join-Path $package 'THIRD-PARTY-NOTICES.txt'), $notices.ToString())
    $rustVersion = rustc --version
    if ($LASTEXITCODE -ne 0) { throw 'Cannot identify Rust compiler.' }
    $gccVersion = & (Join-Path $GccBin 'gcc.exe') -dumpfullversion
    if ($LASTEXITCODE -ne 0) { throw 'Cannot identify GCC compiler.' }
    @{ sourceCommit=$commit; dirty=$dirty; buildInvoked=(-not $SkipBuild); version=$version; profile=$Profile; target='x86_64-pc-windows-gnu'; rust=$rustVersion; gcc=$gccVersion; cargoLockSha256=(Get-FileHash -LiteralPath (Join-Path $repo 'Cargo.lock')).Hash; builtAtUtc=[DateTime]::UtcNow.ToString('o'); missingLicenseTexts=$missing } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $package 'BUILD-INFO.json') -Encoding utf8
    Set-Content -LiteralPath (Join-Path $package 'EVALUATION-NOTICE.txt') -Value 'Unsigned release-candidate evaluation build. See RELEASE.md for open acceptance gates. MIT applies to Octave; third-party obligations require distribution review. No production update feed or automatic updater is enabled.'
    $files = @(Get-ChildItem -LiteralPath $package -File | ForEach-Object { @{ name=$_.Name; sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash } })
    @{product='Octave'; version=$version; files=$files} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $package 'manifest.json') -Encoding utf8
    Compress-Archive -LiteralPath $package -DestinationPath ($package + '.zip')
    Write-Output $package
} finally { Pop-Location }
