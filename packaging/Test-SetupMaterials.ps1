param(
    [Parameter(Mandatory)][string]$UnsignedMaterialsDir,
    [Parameter(Mandatory)][string]$SignedMaterialsDir,
    [Parameter(Mandatory)][string]$PackageDir,
    [Parameter(Mandatory)][string]$Compiler,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$ExpectedInstallerCommit,
    [Parameter(Mandatory)][ValidatePattern('^[A-Fa-f0-9]{40}$')][string]$PublisherThumbprint
)
$ErrorActionPreference = 'Stop'
$names = @('octave-uninstaller.exe','Setup-Guard.ps1','materials.json')
foreach ($dir in @($UnsignedMaterialsDir,$SignedMaterialsDir)) {
    $directory = Get-Item -LiteralPath $dir -Force
    if (-not $directory.PSIsContainer -or ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Expected an ordinary materials directory.' }
    $items = @(Get-ChildItem -LiteralPath $dir -Force)
    if ($items.Count -ne $names.Count) { throw 'Unexpected Setup signing materials.' }
    foreach ($item in $items) {
        if ($item.Name -notin $names -or $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unexpected or linked Setup signing material.' }
    }
}
$originalReceipt = Join-Path $UnsignedMaterialsDir 'materials.json'
if ((Get-FileHash -LiteralPath $originalReceipt).Hash -ne (Get-FileHash -LiteralPath (Join-Path $SignedMaterialsDir 'materials.json')).Hash) { throw 'Provider changed Setup materials receipt.' }
$receipt = Get-Content -LiteralPath $originalReceipt -Raw | ConvertFrom-Json
$build = Get-Content -LiteralPath (Join-Path $PackageDir 'BUILD-INFO.json') -Raw | ConvertFrom-Json
$manifest = Get-Content -LiteralPath (Join-Path $PackageDir 'manifest.json') -Raw | ConvertFrom-Json
$signing = Get-Content -LiteralPath (Join-Path $PackageDir 'SIGNING-INFO.json') -Raw | ConvertFrom-Json
if ($receipt.product -cne 'Octave' -or $receipt.version -cne $manifest.version -or $receipt.payloadSourceCommit -cne $build.sourceCommit -or $receipt.installerDirty -ne $false -or $receipt.installerSourceCommit -cne $ExpectedInstallerCommit -or $receipt.compilerVersion -cne '7.1.1' -or $receipt.compilerSha256 -ne (Get-FileHash -LiteralPath $Compiler).Hash -or $receipt.payloadManifestSha256 -ne $signing.unsignedManifestSha256 -or $signing.sourceCommit -cne $build.sourceCommit -or $signing.publisherThumbprint -ne $PublisherThumbprint) { throw 'Setup materials differ from reviewed build, compiler or payload.' }
if ($receipt.uninstallerCacheName -cnotmatch '^uninst-7\.1\.1-[a-f0-9]{10}\.e64$') { throw 'Invalid uninstaller cache filename.' }
if (@($receipt.files).Count -ne 2 -or @($receipt.files.name | Sort-Object -Unique).Count -ne 2) { throw 'Invalid Setup materials manifest.' }
foreach ($entry in $receipt.files) {
    if ($entry.name -cnotin @('octave-uninstaller.exe','Setup-Guard.ps1') -or $entry.sha256 -notmatch '^[A-Fa-f0-9]{64}$' -or (Get-FileHash -LiteralPath (Join-Path $UnsignedMaterialsDir $entry.name)).Hash -ne $entry.sha256) { throw 'Original Setup material checksum mismatch.' }
    & (Join-Path $PSScriptRoot 'Test-TrustedSignature.ps1') -File (Join-Path $SignedMaterialsDir $entry.name) -PublisherThumbprint $PublisherThumbprint
}
if ((Get-FileHash -LiteralPath (Join-Path $UnsignedMaterialsDir 'Setup-Guard.ps1')).Hash -ne (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'Setup-Guard.ps1')).Hash) { throw 'Setup guard differs from reviewed installer source.' }
foreach ($dir in @($UnsignedMaterialsDir,$SignedMaterialsDir)) {
    & (Join-Path $PSScriptRoot 'Test-SetupMetadata.ps1') -Executable (Join-Path $dir 'octave-uninstaller.exe') -Version $manifest.version -Uninstaller | Out-Host
}
& (Join-Path $PSScriptRoot 'Test-PeSigningContent.ps1') -UnsignedFile (Join-Path $UnsignedMaterialsDir 'octave-uninstaller.exe') -SignedFile (Join-Path $SignedMaterialsDir 'octave-uninstaller.exe')
& (Join-Path $PSScriptRoot 'Test-PowerShellSigningContent.ps1') -UnsignedFile (Join-Path $UnsignedMaterialsDir 'Setup-Guard.ps1') -SignedFile (Join-Path $SignedMaterialsDir 'Setup-Guard.ps1')
Write-Output 'setup_materials=verified'
