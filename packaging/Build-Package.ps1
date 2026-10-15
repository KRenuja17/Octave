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
    $licenseRoot = Join-Path $PSScriptRoot 'licenses'
    $supplements = Get-Content -LiteralPath (Join-Path $licenseRoot 'sources.json') -Raw | ConvertFrom-Json
    foreach ($dependency in $dependencies) {
        [void]$notices.AppendLine("`n===== $($dependency.name) $($dependency.version) | $($dependency.license) | $($dependency.repository) =====")
        $root = Split-Path $dependency.manifest_path -Parent
        $licenses = @(Get-ChildItem -LiteralPath $root -Recurse -File | Where-Object {
            $_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE|OFL|UFL)([._-]|$)' -or
            $_.Name -match '[-_](license|licence|copyright)([._-]|$)' -or
            ($_.Extension -eq '.txt' -and (Test-Path -LiteralPath ([IO.Path]::ChangeExtension($_.FullName, '.ttf')) -PathType Leaf)) -or
            ($dependency.license_file -and $_.FullName -eq (Join-Path $root $dependency.license_file))
        } | Sort-Object FullName -Unique)
        if (-not $licenses.Count) {
            $supplement = @($supplements | Where-Object { $_.name -eq $dependency.name -and $_.version -eq $dependency.version })
            if ($supplement.Count -ne 1 -or -not $supplement[0].files.Count) { $missing += "$($dependency.name) $($dependency.version)" }
            else {
                $vcs = Get-Content -LiteralPath (Join-Path $root '.cargo_vcs_info.json') -Raw | ConvertFrom-Json
                if ($vcs.git.sha1 -ne $supplement[0].revision) { throw 'Supplemental notice revision does not match the locked crate.' }
                foreach ($notice in $supplement[0].files) {
                    if ($notice.file -ne [IO.Path]::GetFileName($notice.file) -or $notice.file -match '[\\/:]') { throw 'Invalid supplemental notice path.' }
                    $noticePath = Join-Path $licenseRoot $notice.file
                    if ((Get-FileHash -LiteralPath $noticePath).Hash -ne $notice.sha256) { throw 'Supplemental notice checksum mismatch.' }
                    [void]$notices.AppendLine("--- Source: $($notice.url) | SHA-256: $($notice.sha256) ---")
                    [void]$notices.AppendLine([IO.File]::ReadAllText($noticePath))
                }
            }
        }
        foreach ($license in $licenses) {
            if ($license.Length -gt 1MB) { throw "License file needs manual review: $($dependency.name)" }
            [void]$notices.AppendLine("--- $($license.FullName.Substring($root.Length + 1)) ---")
            [void]$notices.AppendLine([IO.File]::ReadAllText($license.FullName))
        }
    }
    if ($missing.Count) { throw "Missing dependency notices: $($missing -join ', '). Add exact-revision texts before packaging." }
    # Include the unmodified MPL crate archives, verified against Cargo.lock.
    $sourceStage = Join-Path $output ('mpl-sources-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $sourceStage | Out-Null
    $lock = [IO.File]::ReadAllText((Join-Path $repo 'Cargo.lock'))
    foreach ($dependency in $dependencies | Where-Object { $_.license -match 'MPL-2.0' }) {
        $crateRoot = Split-Path $dependency.manifest_path -Parent
        $registryRoot = Split-Path (Split-Path (Split-Path $crateRoot -Parent) -Parent) -Parent
        $registry = Split-Path (Split-Path $crateRoot -Parent) -Leaf
        $archiveName = "$($dependency.name)-$($dependency.version).crate"
        $archive = Join-Path $registryRoot "cache\$registry\$archiveName"
        $block = @($lock -split '\[\[package\]\]' | Where-Object { $_ -match ('(?m)^name = "' + [regex]::Escape($dependency.name) + '"\r?$') -and $_ -match ('(?m)^version = "' + [regex]::Escape($dependency.version) + '"\r?$') })
        if ($block.Count -ne 1 -or $block[0] -notmatch '(?m)^checksum = "([a-f0-9]{64})"') { throw 'Cannot identify MPL source checksum.' }
        $expected = $Matches[1]
        if ((Get-FileHash -LiteralPath $archive).Hash -ne $expected) { throw 'MPL source archive checksum mismatch.' }
        Copy-Item -LiteralPath $archive -Destination $sourceStage
    }
    Copy-Item -LiteralPath (Join-Path $licenseRoot 'pdeljanov-Symphonia-6d533f26-LICENSE') -Destination (Join-Path $sourceStage 'LICENSE-MPL-2.0')
    Set-Content -LiteralPath (Join-Path $sourceStage 'README.txt') -Value 'Unmodified locked MPL-2.0 crate source archives (gzip tar .crate files), supplied with Octave. Each original archive is checked against Cargo.lock. Octave makes no changes to these dependencies. The corresponding license is included. Extract with a tar-compatible tool. Exact package versions are in dependencies.csv.'
    Compress-Archive -Path (Join-Path $sourceStage '*') -DestinationPath (Join-Path $package 'MPL-SOURCES.zip')
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
