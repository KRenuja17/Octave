param(
    [Parameter(Mandatory)][string]$PackageDir,
    [Parameter(Mandatory)][string]$OutputDir,
    [string]$Compiler,
    [switch]$ValidateOnly,
    [switch]$AllowDirty
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$package = (Resolve-Path -LiteralPath $PackageDir).Path
$output = [IO.Path]::GetFullPath($OutputDir)
foreach ($directory in @($package,$output)) {
    if ($directory -eq $repo -or $directory.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Keep package input and setup output outside the source repository.' }
}
if ($output -eq $package -or $output.StartsWith($package + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Do not modify an existing package while building Setup.' }
$manifest = Get-Content -LiteralPath (Join-Path $package 'manifest.json') -Raw | ConvertFrom-Json
if ($manifest.product -ne 'Octave' -or $manifest.version -notmatch '^(\d+)\.(\d+)\.(\d+)(-[A-Za-z0-9.]+)?\z') { throw 'Expected a versioned Octave package.' }
$numeric = '{0}.{1}.{2}.0' -f $Matches[1],$Matches[2],$Matches[3]
$names = @($manifest.files.name)
if (-not $names.Count -or $names.Count -gt 128 -or @($names | Sort-Object -Unique).Count -ne $names.Count) { throw 'Invalid or duplicate package entries.' }
foreach ($required in @('octave-desktop.exe','octave-signature.exe','Install.ps1','Uninstall.ps1','LICENSE','PRIVACY.md','BUILD-INFO.json','THIRD-PARTY-NOTICES.txt','MPL-SOURCES.zip')) {
    if ($required -notin $names) { throw "Missing required package content: $required" }
}
foreach ($entry in $manifest.files) {
    if ($entry.name -notmatch '^[A-Za-z0-9][A-Za-z0-9_.-]*\z' -or $entry.sha256 -notmatch '^[A-Fa-f0-9]{64}\z' -or $entry.name -in @('Setup-Guard.ps1','octave-setup-guard.ps1','unins000.exe','unins000.dat')) { throw 'Unsafe or installer-reserved package entry.' }
    $file = Get-Item -LiteralPath (Join-Path $package $entry.name)
    if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Linked or nested package input is not allowed.' }
    if ((Get-FileHash -LiteralPath $file.FullName).Hash -ne $entry.sha256) { throw "Package checksum mismatch: $($entry.name)" }
}
foreach ($item in Get-ChildItem -LiteralPath $package -Force) {
    if ($item.Name -notin ($names + @('manifest.json')) -or $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unlisted, nested or linked package input is not allowed.' }
}
$info = Get-Content -LiteralPath (Join-Path $package 'BUILD-INFO.json') -Raw | ConvertFrom-Json
if ($info.dirty -or $info.profile -ne 'release' -or $info.target -ne 'x86_64-pc-windows-gnu' -or $info.version -ne $manifest.version -or $info.sourceCommit -notmatch '^[a-f0-9]{40}\z') { throw 'Setup requires a clean optimized Windows x64 version-matched package.' }
foreach ($kind in @('desktop','signature')) {
    & (Join-Path $PSScriptRoot 'Test-ExecutableMetadata.ps1') -Executable (Join-Path $package "octave-$kind.exe") -Version $manifest.version -Kind $kind | Out-Host
}
if ($ValidateOnly) { Write-Output 'setup_input=verified'; return }
if (-not $Compiler -or -not (Test-Path -LiteralPath $Compiler -PathType Leaf)) { throw 'Provide the portable Inno Setup 7.1.1 ISCC.exe compiler.' }
$compilerVersion = & $Compiler --version
if ($LASTEXITCODE -ne 0 -or ($compilerVersion | Out-String).Trim() -ne '7.1.1') { throw 'Expected pinned Inno Setup 7.1.1.' }
Push-Location $repo
try {
    $installerCommit = git rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Cannot identify installer source commit.' }
    $dirty = [bool](git status --porcelain)
    if ($dirty -and -not $AllowDirty) { throw 'Commit the validated installer source before building a deliverable.' }
    $name = 'Octave-' + $manifest.version + '-Setup'
    $setup = Join-Path $output ($name + '.exe')
    if (Test-Path -LiteralPath $setup) { throw 'Setup output already exists; use another directory to keep artifacts immutable.' }
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    $stage = Join-Path $output ('.setup-build-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $stage | Out-Null
    function Escape-Inno([string]$Value) { $Value.Replace('"','""').Replace('{','{{') }
    $include = Join-Path $stage 'payload.iss'
    $lines = foreach ($nameInPackage in ($names + @('manifest.json'))) {
        'Source: "' + (Escape-Inno (Join-Path $package $nameInPackage)) + '"; DestDir: "{app}"; Flags: ignoreversion'
    }
    [IO.File]::WriteAllLines($include, $lines, [Text.UTF8Encoding]::new($true))
    $arguments = @('--quiet','--no-ide-signtools',('-dOctaveVersion=' + $manifest.version),('-dNumericVersion=' + $numeric),('-dPackageDir=' + $package),('-dSetupOutputDir=' + $output),('-dPayloadInclude=' + $include),('-dGuardPath=' + (Join-Path $PSScriptRoot 'Setup-Guard.ps1')),(Join-Path $PSScriptRoot 'Octave.iss'))
    & $Compiler @arguments | Out-Host
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $setup)) { throw 'Setup compilation failed.' }
    $sha256 = (Get-FileHash -LiteralPath $setup).Hash
    [IO.File]::WriteAllText((Join-Path $output ($name + '.sha256')), $sha256 + '  ' + [IO.Path]::GetFileName($setup) + [Environment]::NewLine)
    @{ version=$manifest.version; setupSha256=$sha256; payloadSourceCommit=$info.sourceCommit; payloadManifestSha256=(Get-FileHash -LiteralPath (Join-Path $package 'manifest.json')).Hash; installerSourceCommit=$installerCommit; installerDirty=$dirty; compilerVersion=$compilerVersion; compilerSha256=(Get-FileHash -LiteralPath $Compiler).Hash; signatureStatus=(Get-AuthenticodeSignature -LiteralPath $setup).Status.ToString(); defaultInstallDir='%LOCALAPPDATA%\Programs\Octave'; preservesProfile=$true } | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $output ($name + '.build.json')) -Encoding utf8
    # Delete only this builder's generated include, never package files.
    Remove-Item -LiteralPath $include
    Remove-Item -LiteralPath $stage
    Write-Output $setup
} finally { Pop-Location }
