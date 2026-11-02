param(
    [Parameter(Mandatory)][string]$PackageDir,
    [Parameter(Mandatory)][string]$OutputDir,
    [string]$Compiler,
    [switch]$ValidateOnly,
    [switch]$AllowDirty,
    [switch]$PrepareSigning,
    [string]$UnsignedMaterialsDir,
    [string]$SignedMaterialsDir,
    [ValidatePattern('^[A-Fa-f0-9]{40}$')][string]$PublisherThumbprint
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$package = (Resolve-Path -LiteralPath $PackageDir).Path
$output = [IO.Path]::GetFullPath($OutputDir)
foreach ($directory in @($package,$output)) {
    if ($directory -eq $repo -or $directory.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Keep package input and setup output outside the source repository.' }
}
if ($output -eq $package -or $output.StartsWith($package + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Do not modify an existing package while building Setup.' }
if ($PrepareSigning -and ($ValidateOnly -or $SignedMaterialsDir)) { throw 'Prepare signing is a separate build phase.' }
if ([bool]$SignedMaterialsDir -ne [bool]$UnsignedMaterialsDir -or [bool]$SignedMaterialsDir -ne [bool]$PublisherThumbprint) { throw 'Signed Setup requires original/returned materials and the approved publisher.' }
if ($SignedMaterialsDir -and ($AllowDirty -or $ValidateOnly)) { throw 'Signed Setup requires a clean real build.' }
foreach ($materialsDir in @($UnsignedMaterialsDir,$SignedMaterialsDir) | Where-Object { $_ }) {
    $materialsPath = [IO.Path]::GetFullPath($materialsDir)
    if ($output -eq $materialsPath -or $output.StartsWith($materialsPath+'\',[StringComparison]::OrdinalIgnoreCase) -or $materialsPath.StartsWith($output+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Setup output and signing inputs must be separate.' }
}
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
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect installer source state.' }
    if ($dirty -and -not $AllowDirty) { throw 'Commit the validated installer source before building a deliverable.' }
    $name = 'Octave-' + $manifest.version + '-Setup'
    $setup = Join-Path $output ($name + '.exe')
    if (Test-Path -LiteralPath $setup) { throw 'Setup output already exists; use another directory to keep artifacts immutable.' }
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    $stage = Join-Path $output ('.setup-build-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $stage | Out-Null
    function Escape-Inno([string]$Value) { $Value.Replace('"','""').Replace('{','{{') }
    $include = Join-Path $stage 'payload.iss'
    $guard = Join-Path $PSScriptRoot 'Setup-Guard.ps1'
    $uninstallerCache = Join-Path $stage 'uninstaller'
    if ($PrepareSigning -or $SignedMaterialsDir) { New-Item -ItemType Directory -Path $uninstallerCache | Out-Null }
    if ($PrepareSigning -and @(Get-ChildItem -LiteralPath $output -Force | Where-Object FullName -ne $stage).Count) { throw 'Prepare signing requires an empty output directory.' }
    if ($SignedMaterialsDir) {
        & (Join-Path $PSScriptRoot 'Test-ReleaseSignatures.ps1') -PackageDir $package -PublisherThumbprint $PublisherThumbprint | Out-Host
        & (Join-Path $PSScriptRoot 'Test-SetupMaterials.ps1') -UnsignedMaterialsDir $UnsignedMaterialsDir -SignedMaterialsDir $SignedMaterialsDir -PackageDir $package -Compiler $Compiler -ExpectedInstallerCommit $installerCommit -PublisherThumbprint $PublisherThumbprint | Out-Host
        $materials = Get-Content -LiteralPath (Join-Path $UnsignedMaterialsDir 'materials.json') -Raw | ConvertFrom-Json
        Copy-Item -LiteralPath (Join-Path $SignedMaterialsDir 'octave-uninstaller.exe') -Destination (Join-Path $uninstallerCache $materials.uninstallerCacheName)
        $guard = Join-Path $SignedMaterialsDir 'Setup-Guard.ps1'
    }
    $lines = foreach ($nameInPackage in ($names + @('manifest.json'))) {
        'Source: "' + (Escape-Inno (Join-Path $package $nameInPackage)) + '"; DestDir: "{app}"; Flags: ignoreversion'
    }
    [IO.File]::WriteAllLines($include, $lines, [Text.UTF8Encoding]::new($true))
    $arguments = @('--quiet','--no-ide-signtools',('-dOctaveVersion=' + $manifest.version),('-dNumericVersion=' + $numeric),('-dPackageDir=' + $package),('-dSetupOutputDir=' + $output),('-dPayloadInclude=' + $include),('-dGuardPath=' + $guard))
    if ($PrepareSigning -or $SignedMaterialsDir) { $arguments += '-dSignedUninstallerDir=' + $uninstallerCache }
    $arguments += Join-Path $PSScriptRoot 'Octave.iss'
    # Console ISCC stops with exit 2 when its exact generated uninstaller needs
    # a signature. It does not open an interactive signing prompt.
    $savedErrorPreference = $ErrorActionPreference
    try {
        # Windows PowerShell 5 turns native stderr into error records. Capture
        # the expected preparation diagnostic without terminating prematurely.
        $ErrorActionPreference = 'Continue'
        $compilerOutput = & $Compiler @arguments 2>&1
        $compilerExit = $LASTEXITCODE
    } finally { $ErrorActionPreference = $savedErrorPreference }
    if ($PrepareSigning) {
        $raw = @(Get-ChildItem -LiteralPath $uninstallerCache -File -Filter 'uninst-*.e64')
        if ($compilerExit -ne 2 -or $raw.Count -ne 1 -or (Test-Path -LiteralPath $setup) -or ($compilerOutput | Out-String) -notmatch '(?s)Signed uninstaller mode is enabled\..*attach your digital signature') { throw "Unexpected signing preparation result: $compilerOutput" }
        & (Join-Path $PSScriptRoot 'Test-SetupMetadata.ps1') -Executable $raw[0].FullName -Version $manifest.version -Uninstaller | Out-Host
        Copy-Item -LiteralPath $raw[0].FullName -Destination (Join-Path $output 'octave-uninstaller.exe')
        Copy-Item -LiteralPath $guard -Destination (Join-Path $output 'Setup-Guard.ps1')
        $files = @('octave-uninstaller.exe','Setup-Guard.ps1' | ForEach-Object { @{name=$_;sha256=(Get-FileHash -LiteralPath (Join-Path $output $_)).Hash} })
        @{product='Octave';version=$manifest.version;payloadSourceCommit=$info.sourceCommit;payloadManifestSha256=(Get-FileHash -LiteralPath (Join-Path $package 'manifest.json')).Hash;installerSourceCommit=$installerCommit;installerDirty=$dirty;compilerVersion='7.1.1';compilerSha256=(Get-FileHash -LiteralPath $Compiler).Hash;uninstallerCacheName=$raw[0].Name;files=$files} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $output 'materials.json') -Encoding utf8
        # Only generated files in this unique compiler stage are removed.
        Remove-Item -LiteralPath $raw[0].FullName
        Remove-Item -LiteralPath $uninstallerCache
        Remove-Item -LiteralPath $include
        Remove-Item -LiteralPath $stage
        $global:LASTEXITCODE = 0
        Write-Output $output
        return
    }
    $compilerOutput | Out-Host
    if ($compilerExit -ne 0 -or -not (Test-Path -LiteralPath $setup)) { throw 'Setup compilation failed.' }
    & (Join-Path $PSScriptRoot 'Test-SetupMetadata.ps1') -Executable $setup -Version $manifest.version | Out-Host
    $sha256 = (Get-FileHash -LiteralPath $setup).Hash
    [IO.File]::WriteAllText((Join-Path $output ($name + '.sha256')), $sha256 + '  ' + [IO.Path]::GetFileName($setup) + [Environment]::NewLine)
    @{ version=$manifest.version; setupSha256=$sha256; payloadSourceCommit=$info.sourceCommit; payloadManifestSha256=(Get-FileHash -LiteralPath (Join-Path $package 'manifest.json')).Hash; installerSourceCommit=$installerCommit; installerDirty=$dirty; compilerVersion=$compilerVersion; compilerSha256=(Get-FileHash -LiteralPath $Compiler).Hash; signatureStatus=(Get-AuthenticodeSignature -LiteralPath $setup).Status.ToString(); signedUninstaller=[bool]$SignedMaterialsDir; signedGuardSha256=$(if($SignedMaterialsDir){(Get-FileHash -LiteralPath $guard).Hash}else{$null}); signedUninstallerSha256=$(if($SignedMaterialsDir){(Get-FileHash -LiteralPath (Join-Path $SignedMaterialsDir 'octave-uninstaller.exe')).Hash}else{$null}); defaultInstallDir='%LOCALAPPDATA%\Programs\Octave'; preservesProfile=$true } | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $output ($name + '.build.json')) -Encoding utf8
    # Delete only this builder's generated include, never package files.
    Remove-Item -LiteralPath $include
    if ($SignedMaterialsDir) { Remove-Item -LiteralPath (Join-Path $uninstallerCache $materials.uninstallerCacheName); Remove-Item -LiteralPath $uninstallerCache }
    Remove-Item -LiteralPath $stage
    Write-Output $setup
} finally { Pop-Location }
