param(
    [Parameter(Mandatory)][string]$UnsignedPackageDir,
    [Parameter(Mandatory)][string]$ProviderPackageDir,
    [Parameter(Mandatory)][string]$OutputDir,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$ExpectedSourceCommit,
    [Parameter(Mandatory)][ValidatePattern('^[A-Fa-f0-9]{40}$')][string]$PublisherThumbprint
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$source = (Resolve-Path -LiteralPath $UnsignedPackageDir).Path
$returned = (Resolve-Path -LiteralPath $ProviderPackageDir).Path
$output = [IO.Path]::GetFullPath($OutputDir)
foreach ($parent in @($repo,$source,$returned)) {
    if ($output -eq $parent -or $output.StartsWith($parent+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Signed output must be outside the repository and input directories.' }
}
$manifest = Get-Content -LiteralPath (Join-Path $source 'manifest.json') -Raw | ConvertFrom-Json
if ($manifest.product -ne 'Octave' -or -not $manifest.files -or $manifest.version -notmatch '^\d+\.\d+\.\d+(-[A-Za-z0-9.]+)?$') { throw 'Not a versioned Octave package.' }
$names = @($manifest.files.name)
if (@($names | Sort-Object -Unique).Count -ne $names.Count -or 'manifest.json' -in $names -or 'SIGNING-INFO.json' -in $names) { throw 'Duplicate or reserved package filenames.' }
$signable = @('octave-desktop.exe','octave-signature.exe','Install.ps1','Uninstall.ps1')
foreach ($required in ($signable + @('BUILD-INFO.json','EVALUATION-NOTICE.txt','LICENSE','THIRD-PARTY-NOTICES.txt','MPL-SOURCES.zip','dependencies.csv'))) {
    if ($required -notin $names) { throw "Missing payload: $required" }
}
foreach ($name in $names) {
    if (-not $name -or $name -ne [IO.Path]::GetFileName($name) -or $name -match '[\\/:]' -or $name -in '.', '..') { throw 'Invalid package filename.' }
    if ([IO.Path]::GetExtension($name) -in '.exe','.ps1' -and $name -notin $signable) { throw 'Unexpected signing target.' }
}
foreach ($directory in @($source,$returned)) {
    $directoryInfo = Get-Item -LiteralPath $directory -Force
    if ($directoryInfo.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Linked input directories are not accepted.' }
    foreach ($item in Get-ChildItem -LiteralPath $directory -Force) {
        if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Name -notin ($names + @('manifest.json'))) { throw 'Unexpected or linked package content.' }
    }
}
foreach ($entry in $manifest.files) {
    if ((Get-FileHash -LiteralPath (Join-Path $source $entry.name) -Algorithm SHA256).Hash -ne $entry.sha256) { throw 'Unsigned input checksum mismatch.' }
    $file = Get-Item -LiteralPath (Join-Path $returned $entry.name) -Force
    if ($entry.name -notin $signable -and (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash -ne $entry.sha256) { throw "Provider changed a non-signing payload: $($entry.name)" }
}
if ((Get-FileHash -LiteralPath (Join-Path $source 'manifest.json')).Hash -ne (Get-FileHash -LiteralPath (Join-Path $returned 'manifest.json')).Hash) { throw 'Provider changed the original manifest.' }
$build = Get-Content -LiteralPath (Join-Path $source 'BUILD-INFO.json') -Raw | ConvertFrom-Json
if ($build.dirty -or -not $build.buildInvoked -or $build.profile -ne 'release' -or $build.sourceCommit -cne $ExpectedSourceCommit -or $build.version -cne $manifest.version) { throw 'Expected a clean optimized build of the reviewed source/version.' }
# SignPath's trusted-build connector and manual approval establish the origin of
# the changed executables. Authenticode verification alone is not proof of that
# origin. Retain the provider request/workflow evidence with the release review.
$destination = Join-Path $output ('Octave-'+$manifest.version+'-signed-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $destination -Force | Out-Null
foreach ($name in $names) { Copy-Item -LiteralPath (Join-Path $returned $name) -Destination $destination }
@{provider='SignPath';publisherThumbprint=$PublisherThumbprint;sourceCommit=$ExpectedSourceCommit;unsignedManifestSha256=(Get-FileHash -LiteralPath (Join-Path $source 'manifest.json')).Hash;assembledAtUtc=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $destination 'SIGNING-INFO.json') -Encoding utf8
Set-Content -LiteralPath (Join-Path $destination 'EVALUATION-NOTICE.txt') -Value 'Signed release-candidate evaluation build. Signatures do not establish consumer-machine, SmartScreen or accessibility acceptance. Automatic updates remain disabled. See RELEASE.md.'
$files = @(Get-ChildItem -LiteralPath $destination -File | ForEach-Object { @{name=$_.Name;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash} })
@{product='Octave';version=$manifest.version;files=$files} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $destination 'manifest.json') -Encoding utf8
& (Join-Path $PSScriptRoot 'Test-ReleaseSignatures.ps1') -PackageDir $destination -PublisherThumbprint $PublisherThumbprint | Out-Host
foreach ($kind in @('desktop','signature')) {
    & (Join-Path $PSScriptRoot 'Test-ExecutableMetadata.ps1') -Executable (Join-Path $destination "octave-$kind.exe") -Version $manifest.version -Kind $kind | Out-Host
}
# Failed verification above leaves no distributable ZIP.
$archive = $destination+'.zip'
Compress-Archive -LiteralPath $destination -DestinationPath $archive
$archiveHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
($archiveHash.ToLowerInvariant()+'  '+[IO.Path]::GetFileName($archive)) | Set-Content -LiteralPath ($archive+'.sha256') -Encoding ascii
@{product='Octave';version=$manifest.version;sourceCommit=$ExpectedSourceCommit;zip=[IO.Path]::GetFileName($archive);zipSha256=$archiveHash;manifestSha256=(Get-FileHash -LiteralPath (Join-Path $destination 'manifest.json')).Hash;publisherThumbprint=$PublisherThumbprint;signatureAndMetadataVerification='passed';consumerAcceptance='pending';accessibilityAcceptance='pending';publication='not approved';automaticUpdatesEnabled=$false} | ConvertTo-Json | Set-Content -LiteralPath ($archive+'.review.json') -Encoding utf8
Write-Output $destination
