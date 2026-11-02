param(
    [Parameter(Mandatory)][string]$PackageDir,
    [Parameter(Mandatory)][string]$SetupDir,
    [Parameter(Mandatory)][string]$MaterialsDir,
    [Parameter(Mandatory)][string]$Compiler,
    [Parameter(Mandatory)][string]$WorkDir
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$work = [IO.Path]::GetFullPath($WorkDir).TrimEnd('\')
foreach ($parent in @($repo,$PackageDir,$SetupDir,$MaterialsDir)) {
    $full = [IO.Path]::GetFullPath($parent).TrimEnd('\')
    if ($work -eq $full -or $work.StartsWith($full+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Fixtures must stay outside source and input artifacts.' }
}
$root = Join-Path $work ('setup-signing-'+[Guid]::NewGuid().ToString('N'))
$payload = Join-Path $root 'payload'
$original = Join-Path $root 'original-materials'
$returned = Join-Path $root 'returned-materials'
$setupInput = Join-Path $root 'setup-input'
$setupReturn = Join-Path $root 'setup-return'
$output = Join-Path $root 'output'
New-Item -Path $payload,$original,$returned,$setupInput,$setupReturn -ItemType Directory -Force | Out-Null
Get-ChildItem -LiteralPath $PackageDir -File | Copy-Item -Destination $payload
Get-ChildItem -LiteralPath $MaterialsDir -File | Copy-Item -Destination $original
Get-ChildItem -LiteralPath $SetupDir -File | Copy-Item -Destination $setupInput
$commit='1'*40; $publisher='2'*40
$manifest = Get-Content -LiteralPath (Join-Path $payload 'manifest.json') -Raw | ConvertFrom-Json
$version = $manifest.version
$name = "Octave-$version-Setup"
$materialPath = Join-Path $original 'materials.json'
$materials = Get-Content -LiteralPath $materialPath -Raw | ConvertFrom-Json
$materials.installerSourceCommit=$commit; $materials.installerDirty=$false; $materials.payloadSourceCommit=$commit
$materials | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $materialPath
Get-ChildItem -LiteralPath $original -File | Copy-Item -Destination $returned
$build = Get-Content -LiteralPath (Join-Path $payload 'BUILD-INFO.json') -Raw | ConvertFrom-Json
$build.sourceCommit=$commit
$build | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $payload 'BUILD-INFO.json')
@{sourceCommit=$commit;publisherThumbprint=$publisher;unsignedManifestSha256=$materials.payloadManifestSha256} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $payload 'SIGNING-INFO.json')
$files = @(Get-ChildItem -LiteralPath $payload -File | Where-Object Name -ne 'manifest.json' | ForEach-Object { @{name=$_.Name;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash} })
@{product='Octave';version=$version;files=$files} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $payload 'manifest.json')
$cases = [Collections.Generic.List[string]]::new()
function Assert-Rejected([string]$Case,[scriptblock]$Action,[string]$Expected='.') {
    $errorText=$null
    try { & $Action | Out-Null } catch { $errorText=$_.Exception.Message }
    if (-not $errorText -or $errorText -notmatch $Expected) { throw "Expected rejection for $Case; got: $errorText" }
    $cases.Add($Case)
    Write-Output "rejected=$Case"
}
function Add-FixtureSignature([string]$Path) {
    # An intentionally fake WIN_CERTIFICATE, never installed/trusted/executed.
    # Tests the code-preservation gate independently of mocked trust responses.
    $bytes=[IO.File]::ReadAllBytes($Path)
    $pe=[BitConverter]::ToInt32($bytes,0x3c)
    $optional=$pe+24
    $security=if([BitConverter]::ToUInt16($bytes,$optional)-eq 0x20b){$optional+144}else{$optional+128}
    $address=[int]([Math]::Ceiling($bytes.Length/8.0)*8)
    $result=[byte[]]::new($address+16)
    [Array]::Copy($bytes,$result,$bytes.Length)
    [Array]::Copy([BitConverter]::GetBytes([uint32]$address),0,$result,$security,4)
    [Array]::Copy([BitConverter]::GetBytes([uint32]16),0,$result,($security+4),4)
    [Array]::Copy([BitConverter]::GetBytes([uint32]16),0,$result,$address,4)
    $result[$address+5]=2; $result[$address+6]=2
    [IO.File]::WriteAllBytes($Path,$result)
}
$gate = Join-Path $PSScriptRoot 'Test-SetupMaterials.ps1'
$argsMaterials=@{UnsignedMaterialsDir=$original;SignedMaterialsDir=$returned;PackageDir=$payload;Compiler=$Compiler;ExpectedInstallerCommit=$commit;PublisherThumbprint=$publisher}
Assert-Rejected 'actual-unsigned-setup' { & (Join-Path $PSScriptRoot 'Test-TrustedSignature.ps1') -File (Join-Path $setupInput ($name+'.exe')) -PublisherThumbprint $publisher } 'Trusted timestamped publisher signature'
Assert-Rejected 'actual-unsigned-materials' { & $gate @argsMaterials } 'Trusted timestamped publisher signature'
Add-FixtureSignature (Join-Path $returned 'octave-uninstaller.exe')
$guardPath = Join-Path $returned 'Setup-Guard.ps1'
$guardText = [IO.File]::ReadAllText($guardPath)
[IO.File]::WriteAllText($guardPath,($guardText+"`r`n# SIG # Begin signature block`r`n# U3ludGhldGlj`r`n# SIG # End signature block`r`n"),[Text.UTF8Encoding]::new($false))
$infoPath=Join-Path $setupInput ($name+'.build.json')
$info=Get-Content -LiteralPath $infoPath -Raw | ConvertFrom-Json
$info.installerSourceCommit=$commit; $info.payloadSourceCommit=$commit; $info.installerDirty=$false
$info.payloadManifestSha256=(Get-FileHash -LiteralPath (Join-Path $payload 'manifest.json')).Hash
$info | Add-Member -NotePropertyName signedUninstaller -NotePropertyValue $true -Force
$info | Add-Member -NotePropertyName signedGuardSha256 -NotePropertyValue (Get-FileHash -LiteralPath (Join-Path $returned 'Setup-Guard.ps1')).Hash -Force
$info | Add-Member -NotePropertyName signedUninstallerSha256 -NotePropertyValue (Get-FileHash -LiteralPath (Join-Path $returned 'octave-uninstaller.exe')).Hash -Force
$info | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $infoPath
Get-ChildItem -LiteralPath $setupInput -File | Copy-Item -Destination $setupReturn
$returnedExe=Join-Path $setupReturn ($name+'.exe')
Add-FixtureSignature $returnedExe
$complete=Join-Path $PSScriptRoot 'Complete-SignedSetup.ps1'
$argsComplete=@{UnsignedSetupDir=$setupInput;ProviderSetupDir=$setupReturn;PackageDir=$payload;SignedMaterialsDir=$returned;OutputDir=$output;ExpectedSourceCommit=$commit;PublisherThumbprint=$publisher}
& {
    $fixtureSignature=[pscustomobject]@{Status='Valid';SignerCertificate=[pscustomobject]@{Thumbprint=$publisher;Extensions=@([pscustomobject]@{Oid=[pscustomobject]@{Value='2.5.29.37'};EnhancedKeyUsages=@([pscustomobject]@{Value='1.3.6.1.5.5.7.3.3'})})};TimeStamperCertificate=[pscustomobject]@{Subject='Synthetic timestamp'}}
    function Get-AuthenticodeSignature { param($LiteralPath) $fixtureSignature }
    & $gate @argsMaterials | Out-Host
    $cases.Add('unchanged-materials-policy')
    Assert-Rejected 'wrong-native-version' { & (Join-Path $PSScriptRoot 'Test-SetupMetadata.ps1') -Executable (Join-Path $returned 'octave-uninstaller.exe') -Version '0.0.0-fixture' -Uninstaller } 'metadata mismatch'
    $good = & $complete @argsComplete
    if (-not (Test-Path -LiteralPath (Join-Path $good ($name+'.sha256')))) { throw 'Successful fixture has no final checksum.' }
    $review=Get-Content -LiteralPath (Join-Path $good ($name+'.review.json')) -Raw | ConvertFrom-Json
    if ($review.publication -cne 'not approved' -or $review.consumerAcceptance -cne 'pending') { throw 'Fixture bypassed attended release review.' }
    $cases.Add('unchanged-setup-policy')
    $wrong=$argsMaterials.Clone(); $wrong.ExpectedInstallerCommit='3'*40
    Assert-Rejected 'wrong-installer-source' { & $gate @wrong } 'reviewed build'
    $wrong=$argsMaterials.Clone(); $wrong.PublisherThumbprint='3'*40
    Assert-Rejected 'wrong-publisher' { & $gate @wrong } 'reviewed build'
    Set-Content -LiteralPath (Join-Path $returned 'extra.exe') -Value 'Authored fixture'
    Assert-Rejected 'extra-material' { & $gate @argsMaterials } 'Unexpected'
    Remove-Item -LiteralPath (Join-Path $returned 'extra.exe')
    Set-Content -LiteralPath (Join-Path $returned 'materials.json') -Value '{}'
    Assert-Rejected 'changed-materials-receipt' { & $gate @argsMaterials } 'changed Setup materials receipt'
    Copy-Item -LiteralPath $materialPath -Destination (Join-Path $returned 'materials.json') -Force
    $signedGuard = [IO.File]::ReadAllText($guardPath)
    [IO.File]::WriteAllText($guardPath,('# altered guard'+"`n"+$signedGuard))
    Assert-Rejected 'changed-guard-code' { & $gate @argsMaterials } 'changed PowerShell code'
    [IO.File]::WriteAllText($guardPath,$signedGuard,[Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText($guardPath,($signedGuard+"`nWrite-Output altered`n"),[Text.UTF8Encoding]::new($false))
    Assert-Rejected 'code-after-signature-block' { & $gate @argsMaterials } 'changed PowerShell code'
    [IO.File]::WriteAllText($guardPath,$signedGuard,[Text.UTF8Encoding]::new($false))
    $bytes=[IO.File]::ReadAllBytes((Join-Path $returned 'octave-uninstaller.exe')); $bytes[512]=$bytes[512] -bxor 1
    [IO.File]::WriteAllBytes((Join-Path $returned 'octave-uninstaller.exe'),$bytes)
    Assert-Rejected 'changed-uninstaller-code' { & $gate @argsMaterials } 'changed PE code'
    Copy-Item -LiteralPath (Join-Path $original 'octave-uninstaller.exe') -Destination (Join-Path $returned 'octave-uninstaller.exe') -Force
    Add-FixtureSignature (Join-Path $returned 'octave-uninstaller.exe')
    $fixtureSignature.TimeStamperCertificate=$null
    Assert-Rejected 'missing-material-timestamp' { & $gate @argsMaterials } 'Trusted timestamped publisher signature'
    Assert-Rejected 'missing-setup-timestamp' { & $complete @argsComplete } 'Trusted timestamped publisher signature'
    $fixtureSignature.TimeStamperCertificate=[pscustomobject]@{Subject='Synthetic timestamp'}
    $fixtureSignature.Status='HashMismatch'
    Assert-Rejected 'invalid-signature' { & $complete @argsComplete } 'Trusted timestamped publisher signature'
    $fixtureSignature.Status='Valid'
    $bytes=[IO.File]::ReadAllBytes($returnedExe); $bytes[512]=$bytes[512] -bxor 1
    [IO.File]::WriteAllBytes($returnedExe,$bytes)
    Assert-Rejected 'changed-final-setup-code' { & $complete @argsComplete } 'changed PE code'
    Copy-Item -LiteralPath (Join-Path $setupInput ($name+'.exe')) -Destination $returnedExe -Force
    Assert-Rejected 'missing-pe-certificate-table' { & $complete @argsComplete } 'appended PE signature'
    Add-FixtureSignature $returnedExe
    Set-Content -LiteralPath (Join-Path $setupReturn ($name+'.build.json')) -Value '{}'
    Assert-Rejected 'changed-provider-build-receipt' { & $complete @argsComplete } 'changed Setup build evidence'
    Copy-Item -LiteralPath $infoPath -Destination (Join-Path $setupReturn ($name+'.build.json')) -Force
    $wrong=$argsComplete.Clone(); $wrong.ExpectedSourceCommit='3'*40
    Assert-Rejected 'wrong-final-source' { & $complete @wrong } 'reviewed clean source'
    $info.signedUninstaller=$false
    $info | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $infoPath
    Copy-Item -LiteralPath $infoPath -Destination (Join-Path $setupReturn ($name+'.build.json')) -Force
    Assert-Rejected 'unsigned-embedded-uninstaller' { & $complete @argsComplete } 'reviewed clean source'
    if (@(Get-ChildItem -LiteralPath $output -Directory).Count -ne 1) { throw 'Rejected returns created additional review packages.' }
}
@{policyCases=$cases;realUnsignedRejection=$true;nativeUninstallerPrepared=$true;actualTrustedSigning=$false;fixtureFilesExecuted=$false} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $root 'result.json')
foreach ($directory in @($payload,$original,$returned,$setupInput,$setupReturn,$output)) {
    $resolved=(Resolve-Path -LiteralPath $directory).Path
    if ((Split-Path $resolved -Parent) -ne $root -or (Get-Item -LiteralPath $resolved -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Unsafe fixture cleanup path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
$global:LASTEXITCODE=0
Write-Output "setup_signing_policy=passed cases=$($cases.Count) evidence=$root"
