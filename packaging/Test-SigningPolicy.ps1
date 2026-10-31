param([Parameter(Mandatory)][string]$WorkDir)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$root = [IO.Path]::GetFullPath($WorkDir)
if ($root -eq $repo -or $root.StartsWith($repo+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Fixtures must be outside the repository.' }
$root = Join-Path $root ('signing-policy-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root -Force | Out-Null
foreach ($name in @('octave-desktop.exe','Install.ps1','Uninstall.ps1')) { Set-Content -LiteralPath (Join-Path $root $name) -Value 'Unsigned fixture, never executed.' }
$files = @(Get-ChildItem -LiteralPath $root -File | ForEach-Object { @{name=$_.Name;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash} })
$manifest = @{product='Octave';version='0.0.0-fixture';files=$files}
$manifestPath = Join-Path $root 'manifest.json'
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifestPath
$thumbprint = '1234567890123456789012345678901234567890'
$gate = Join-Path $PSScriptRoot 'Test-ReleaseSignatures.ps1'
function Assert-Rejected([string]$Name, [scriptblock]$Action) {
    $rejected = $false
    try { & $Action } catch { $rejected = $true }
    if (-not $rejected) { throw "Policy accepted $Name" }
    Write-Output "rejected=$Name"
}
# First use the real Windows signature verifier, not the fixture override below.
Assert-Rejected 'unsigned-payload' { & $gate -PackageDir $root -PublisherThumbprint $thumbprint }
& {
    # Policy fixtures isolate publisher, timestamp, usage and manifest handling.
    # Mocked Valid is not evidence of an actual trusted signing operation.
    $fixtureSignature = [pscustomobject]@{Status='Valid';SignerCertificate=[pscustomobject]@{Thumbprint=$thumbprint;Extensions=@([pscustomobject]@{Oid=[pscustomobject]@{Value='2.5.29.37'};EnhancedKeyUsages=@([pscustomobject]@{Value='1.3.6.1.5.5.7.3.3'})})};TimeStamperCertificate=[pscustomobject]@{Subject='Synthetic timestamp'}}
    function Get-AuthenticodeSignature { param($LiteralPath) $fixtureSignature }
    & $gate -PackageDir $root -PublisherThumbprint $thumbprint
    $fixtureSignature.Status = 'HashMismatch'
    Assert-Rejected 'invalid-signature' { & $gate -PackageDir $root -PublisherThumbprint $thumbprint }
    $fixtureSignature.Status = 'Valid'
    Assert-Rejected 'wrong-publisher' { & $gate -PackageDir $root -PublisherThumbprint ('A'*40) }
    $timestamp = $fixtureSignature.TimeStamperCertificate
    $fixtureSignature.TimeStamperCertificate = $null
    Assert-Rejected 'missing-timestamp' { & $gate -PackageDir $root -PublisherThumbprint $thumbprint }
    $fixtureSignature.TimeStamperCertificate = $timestamp
    $usages = $fixtureSignature.SignerCertificate.Extensions
    $fixtureSignature.SignerCertificate.Extensions = @()
    Assert-Rejected 'missing-code-signing-usage' { & $gate -PackageDir $root -PublisherThumbprint $thumbprint }
    $fixtureSignature.SignerCertificate.Extensions = $usages
    Set-Content -LiteralPath (Join-Path $root 'Install.ps1') -Value 'Tampered'
    Assert-Rejected 'changed-payload' { & $gate -PackageDir $root -PublisherThumbprint $thumbprint }
}
$manifest.files = @($files) + @($files[0])
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifestPath
Assert-Rejected 'duplicate-manifest-name' { & $gate -PackageDir $root -PublisherThumbprint $thumbprint }
$manifest.files = @(@{name='../escape.exe';sha256='0'*64})
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifestPath
Assert-Rejected 'unsafe-or-missing-payload' { & $gate -PackageDir $root -PublisherThumbprint $thumbprint }
@{policyFixtures=$true; realUnsignedRejection=$true; actualTrustedSigning=$false} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'result.json') -Encoding utf8
Write-Output "signing_policy=passed evidence=$root"

