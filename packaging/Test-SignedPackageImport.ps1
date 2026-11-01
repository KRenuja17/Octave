param(
    [Parameter(Mandatory)][string]$WorkDir,
    [Parameter(Mandatory)][string]$DesktopExecutable,
    [Parameter(Mandatory)][string]$WorkerExecutable,
    [Parameter(Mandatory)][string]$Version
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$root = [IO.Path]::GetFullPath($WorkDir)
if ($root -eq $repo -or $root.StartsWith($repo+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Fixtures must stay outside the repository.' }
$root = Join-Path $root ('signed-import-fixtures-'+[Guid]::NewGuid().ToString('N'))
$source = Join-Path $root 'unsigned'
$returned = Join-Path $root 'returned'
$output = Join-Path $root 'output'
New-Item -ItemType Directory -Path $source,$returned -Force | Out-Null
Copy-Item -LiteralPath $DesktopExecutable -Destination (Join-Path $source 'octave-desktop.exe')
Copy-Item -LiteralPath $WorkerExecutable -Destination (Join-Path $source 'octave-signature.exe')
foreach ($name in @('Install.ps1','Uninstall.ps1','EVALUATION-NOTICE.txt','LICENSE','THIRD-PARTY-NOTICES.txt','MPL-SOURCES.zip','dependencies.csv')) {
    Set-Content -LiteralPath (Join-Path $source $name) -Value 'Authored policy fixture; never execute or distribute.'
}
$commit = '1'*40
$thumbprint = '2'*40
$build = @{dirty=$false;buildInvoked=$true;profile='release';version=$Version;sourceCommit=$commit}
$build | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $source 'BUILD-INFO.json')
$files = @(Get-ChildItem -LiteralPath $source -File | ForEach-Object { @{name=$_.Name;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash} })
@{product='Octave';version=$Version;files=$files} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $source 'manifest.json')
Get-ChildItem -LiteralPath $source -File | Copy-Item -Destination $returned
$gate = Join-Path $PSScriptRoot 'Complete-SignedPackage.ps1'
$arguments = @{UnsignedPackageDir=$source;ProviderPackageDir=$returned;OutputDir=$output;ExpectedSourceCommit=$commit;PublisherThumbprint=$thumbprint}
function Assert-Rejected([string]$Name, [scriptblock]$Action) {
    $rejected=$false
    try { & $Action | Out-Null } catch { $rejected=$true }
    if (-not $rejected) { throw "Signed import accepted $Name" }
    Write-Output "rejected=$Name"
}
# Real Windows verification must reject these unsigned files, before any mock.
Assert-Rejected 'actual-unsigned-return' { & $gate @arguments }
if (Get-ChildItem -LiteralPath $output -Filter '*.zip' -ErrorAction SilentlyContinue) { throw 'Failed import created a distributable ZIP.' }
& {
    # A synthetic valid response tests import policy; it is not trusted signing.
    $fixtureSignature=[pscustomobject]@{Status='Valid';SignerCertificate=[pscustomobject]@{Thumbprint=$thumbprint;Extensions=@([pscustomobject]@{Oid=[pscustomobject]@{Value='2.5.29.37'};EnhancedKeyUsages=@([pscustomobject]@{Value='1.3.6.1.5.5.7.3.3'})})};TimeStamperCertificate=[pscustomobject]@{Subject='Synthetic timestamp'}}
    function Get-AuthenticodeSignature { param($LiteralPath) $fixtureSignature }
    $result = & $gate @arguments
    if (-not (Test-Path -LiteralPath ($result+'.zip.sha256'))) { throw 'Successful fixture did not produce review hashes.' }
    $wrongSource=$arguments.Clone(); $wrongSource.ExpectedSourceCommit='3'*40
    Assert-Rejected 'wrong-source' { & $gate @wrongSource }
    Set-Content -LiteralPath (Join-Path $returned 'LICENSE') -Value 'Changed license'
    Assert-Rejected 'changed-support-file' { & $gate @arguments }
    Copy-Item -LiteralPath (Join-Path $source 'LICENSE') -Destination (Join-Path $returned 'LICENSE') -Force
    Set-Content -LiteralPath (Join-Path $returned 'manifest.json') -Value '{}'
    Assert-Rejected 'changed-original-manifest' { & $gate @arguments }
    Copy-Item -LiteralPath (Join-Path $source 'manifest.json') -Destination (Join-Path $returned 'manifest.json') -Force
    Set-Content -LiteralPath (Join-Path $returned 'extra.exe') -Value 'Unlisted fixture'
    Assert-Rejected 'extra-provider-payload' { & $gate @arguments }
    Remove-Item -LiteralPath (Join-Path $returned 'extra.exe')
    $fixtureSignature.TimeStamperCertificate=$null
    Assert-Rejected 'missing-timestamp' { & $gate @arguments }
    if (@(Get-ChildItem -LiteralPath $output -Filter '*.zip').Count -ne 1) { throw 'Rejected imports produced extra ZIPs.' }
}
@{policyFixtures=$true;realUnsignedRejection=$true;actualTrustedSigning=$false;version=$Version} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'result.json')
# Retain the small receipt, not repeated executable/archive copies. Every
# disposable path was created in this unique fixture directory above.
foreach ($directory in @($source,$returned,$output)) {
    $resolved = (Resolve-Path -LiteralPath $directory).Path
    if ((Split-Path $resolved -Parent) -ne $root -or (Split-Path $resolved -Leaf) -notin 'unsigned','returned','output') { throw 'Unsafe fixture cleanup path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
Write-Output "signed_import_policy=passed evidence=$root"
