param(
    [Parameter(Mandatory)][string]$UnsignedSetupDir,
    [Parameter(Mandatory)][string]$ProviderSetupDir,
    [Parameter(Mandatory)][string]$PackageDir,
    [Parameter(Mandatory)][string]$SignedMaterialsDir,
    [Parameter(Mandatory)][string]$OutputDir,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$ExpectedSourceCommit,
    [Parameter(Mandatory)][ValidatePattern('^[A-Fa-f0-9]{40}$')][string]$PublisherThumbprint
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$output = [IO.Path]::GetFullPath($OutputDir)
foreach ($inputDir in @($repo,$UnsignedSetupDir,$ProviderSetupDir,$PackageDir,$SignedMaterialsDir)) {
    $inputPath = (Resolve-Path -LiteralPath $inputDir).Path.TrimEnd('\')
    if ($output -eq $inputPath -or $output.StartsWith($inputPath+'\',[StringComparison]::OrdinalIgnoreCase) -or $inputPath.StartsWith($output.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Signed Setup output must be separate from source and inputs.' }
}
$manifest = Get-Content -LiteralPath (Join-Path $PackageDir 'manifest.json') -Raw | ConvertFrom-Json
if ($manifest.product -cne 'Octave' -or $manifest.version -notmatch '^\d+\.\d+\.\d+(-[A-Za-z0-9.]+)?$') { throw 'Expected a versioned Octave package.' }
$name = 'Octave-' + $manifest.version + '-Setup'
$names = @(($name+'.exe'),($name+'.sha256'),($name+'.build.json'))
foreach ($dir in @($UnsignedSetupDir,$ProviderSetupDir)) {
    $item = Get-Item -LiteralPath $dir -Force
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Linked Setup directories are not accepted.' }
    $files = @(Get-ChildItem -LiteralPath $dir -Force)
    if ($files.Count -ne 3) { throw 'Expected exactly the Setup executable and its original receipts.' }
    foreach ($file in $files) {
        if ($file.Name -cnotin $names -or $file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unexpected or linked Setup return.' }
        if ($file.Extension -ne '.exe' -and (Get-FileHash -LiteralPath $file.FullName).Hash -ne (Get-FileHash -LiteralPath (Join-Path $UnsignedSetupDir $file.Name)).Hash) { throw 'Provider changed Setup build evidence.' }
    }
}
$info = Get-Content -LiteralPath (Join-Path $UnsignedSetupDir ($name+'.build.json')) -Raw | ConvertFrom-Json
$original = Join-Path $UnsignedSetupDir ($name+'.exe')
$returned = Join-Path $ProviderSetupDir ($name+'.exe')
if ($info.version -cne $manifest.version -or $info.installerDirty -ne $false -or $info.installerSourceCommit -cne $ExpectedSourceCommit -or $info.payloadSourceCommit -cne $ExpectedSourceCommit -or $info.payloadManifestSha256 -ne (Get-FileHash -LiteralPath (Join-Path $PackageDir 'manifest.json')).Hash -or $info.setupSha256 -ne (Get-FileHash -LiteralPath $original).Hash -or $info.signedUninstaller -ne $true -or $info.signedGuardSha256 -ne (Get-FileHash -LiteralPath (Join-Path $SignedMaterialsDir 'Setup-Guard.ps1')).Hash -or $info.signedUninstallerSha256 -ne (Get-FileHash -LiteralPath (Join-Path $SignedMaterialsDir 'octave-uninstaller.exe')).Hash) { throw 'Setup does not bind the reviewed clean source, signed package and signed materials.' }
& (Join-Path $PSScriptRoot 'Test-ReleaseSignatures.ps1') -PackageDir $PackageDir -PublisherThumbprint $PublisherThumbprint | Out-Host
foreach ($file in @($returned,(Join-Path $SignedMaterialsDir 'Setup-Guard.ps1'),(Join-Path $SignedMaterialsDir 'octave-uninstaller.exe'))) {
    & (Join-Path $PSScriptRoot 'Test-TrustedSignature.ps1') -File $file -PublisherThumbprint $PublisherThumbprint
}
& (Join-Path $PSScriptRoot 'Test-PeSigningContent.ps1') -UnsignedFile $original -SignedFile $returned
& (Join-Path $PSScriptRoot 'Test-SetupMetadata.ps1') -Executable $returned -Version $manifest.version | Out-Host
$destination = Join-Path $output ('Octave-'+$manifest.version+'-signed-setup-'+[Guid]::NewGuid().ToString('N'))
New-Item -Path $destination -ItemType Directory -Force | Out-Null
Copy-Item -LiteralPath $returned -Destination $destination
Copy-Item -LiteralPath (Join-Path $UnsignedSetupDir ($name+'.build.json')) -Destination $destination
$finalHash = (Get-FileHash -LiteralPath $returned).Hash
($finalHash.ToLowerInvariant()+'  '+$name+'.exe') | Set-Content -LiteralPath (Join-Path $destination ($name+'.sha256')) -Encoding ascii
@{product='Octave';version=$manifest.version;sourceCommit=$ExpectedSourceCommit;setupSha256=$finalHash;unsignedSetupSha256=$info.setupSha256;payloadManifestSha256=$info.payloadManifestSha256;publisherThumbprint=$PublisherThumbprint;signaturesAndMetadata='passed';peCodeUnchanged=$true;signedUninstaller=$true;signedGuardSha256=$info.signedGuardSha256;signedUninstallerSha256=$info.signedUninstallerSha256;consumerAcceptance='pending';accessibilityAcceptance='pending';publication='not approved';automaticUpdatesEnabled=$false} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $destination ($name+'.review.json')) -Encoding utf8
Write-Output $destination
