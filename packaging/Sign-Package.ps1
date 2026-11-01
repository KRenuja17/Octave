param(
    [Parameter(Mandatory)][string]$PackageDir,
    [Parameter(Mandatory)][string]$OutputDir,
    [Parameter(Mandatory)][ValidatePattern('^[A-Fa-f0-9]{40}$')][string]$CertificateThumbprint,
    [Parameter(Mandatory)][uri]$TimestampServer
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$source = (Resolve-Path -LiteralPath $PackageDir).Path
$output = [IO.Path]::GetFullPath($OutputDir)
if ($output -eq $repo -or $output.StartsWith($repo+'\',[StringComparison]::OrdinalIgnoreCase) -or $output -eq $source -or $output.StartsWith($source+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Signing output must be outside the repository and input package.' }
# The Windows API used by Set-AuthenticodeSignature cannot timestamp over HTTPS.
# The timestamp itself is signed; verification below requires that timestamp.
if ($TimestampServer.Scheme -ne 'http' -or $TimestampServer.UserInfo -or $TimestampServer.Fragment) { throw 'This Windows signing API requires the provider HTTP timestamp URL without credentials; use the provider SDK for HTTPS/RFC3161 signing.' }
$certificate = Get-Item -LiteralPath "Cert:\CurrentUser\My\$CertificateThumbprint" -ErrorAction Stop
$usages = @($certificate.Extensions | Where-Object { $_.Oid.Value -eq '2.5.29.37' } | ForEach-Object { $_.EnhancedKeyUsages } | ForEach-Object { $_.Value })
if (-not $certificate.HasPrivateKey -or $certificate.NotBefore -gt (Get-Date) -or $certificate.NotAfter -le (Get-Date) -or '1.3.6.1.5.5.7.3.3' -notin $usages) { throw 'A current code-signing certificate with an accessible private key is required.' }
if ($certificate.Subject -eq $certificate.Issuer) { throw 'Self-signed certificates are not accepted for production packages.' }
$build = Get-Content -LiteralPath (Join-Path $source 'BUILD-INFO.json') -Raw | ConvertFrom-Json
if ($build.dirty -or -not $build.buildInvoked -or $build.profile -ne 'release') { throw 'Sign only a clean optimized source build.' }
$manifest = Get-Content -LiteralPath (Join-Path $source 'manifest.json') -Raw | ConvertFrom-Json
if ($manifest.product -ne 'Octave' -or -not $manifest.files) { throw 'Not an Octave package.' }
$names = @($manifest.files.name)
if (@($names | Sort-Object -Unique).Count -ne $names.Count) { throw 'Duplicate package filenames.' }
foreach ($required in @('octave-desktop.exe','octave-signature.exe','Install.ps1','Uninstall.ps1','BUILD-INFO.json','EVALUATION-NOTICE.txt')) { if ($required -notin $names) { throw "Missing payload: $required" } }
foreach ($entry in $manifest.files) {
    if ($entry.name -ne [IO.Path]::GetFileName($entry.name) -or $entry.name -match '[\\/:]' -or $entry.name -in '.', '..') { throw 'Invalid package filename.' }
    $file = Get-Item -LiteralPath (Join-Path $source $entry.name) -Force
    if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -or (Get-FileHash -LiteralPath $file.FullName).Hash -ne $entry.sha256) { throw 'Input payload verification failed.' }
}
# Signing is a separate immutable artifact; the unsigned input stays available.
foreach ($kind in @('desktop','signature')) {
    & (Join-Path $PSScriptRoot 'Test-ExecutableMetadata.ps1') -Executable (Join-Path $source "octave-$kind.exe") -Version $manifest.version -Kind $kind | Out-Host
}
$destination = Join-Path $output ('Octave-'+$manifest.version+'-signed-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $destination -Force | Out-Null
foreach ($entry in $manifest.files) { Copy-Item -LiteralPath (Join-Path $source $entry.name) -Destination $destination }
foreach ($file in Get-ChildItem -LiteralPath $destination -File | Where-Object { $_.Extension -in '.exe','.ps1' }) {
    $signature = Set-AuthenticodeSignature -LiteralPath $file.FullName -Certificate $certificate -HashAlgorithm SHA256 -IncludeChain NotRoot -TimestampServer $TimestampServer.AbsoluteUri
    if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate) { throw 'Signing or timestamp validation failed; incomplete artifact must not be released.' }
}
@{publisherThumbprint=$certificate.Thumbprint; sourceCommit=$build.sourceCommit; signedAtUtc=[DateTime]::UtcNow.ToString('o'); hashAlgorithm='SHA256'; timestampServer=$TimestampServer.AbsoluteUri} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $destination 'SIGNING-INFO.json') -Encoding utf8
Set-Content -LiteralPath (Join-Path $destination 'EVALUATION-NOTICE.txt') -Value 'Signed release-candidate evaluation build. Signing does not establish listening or fresh-machine acceptance. No production update feed or automatic updater is enabled. See README.md.'
$files = @(Get-ChildItem -LiteralPath $destination -File | ForEach-Object { @{name=$_.Name;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash} })
@{product='Octave';version=$manifest.version;files=$files} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $destination 'manifest.json') -Encoding utf8
& (Join-Path $PSScriptRoot 'Test-ReleaseSignatures.ps1') -PackageDir $destination -PublisherThumbprint $CertificateThumbprint
Compress-Archive -LiteralPath $destination -DestinationPath ($destination+'.zip')
Write-Output $destination
