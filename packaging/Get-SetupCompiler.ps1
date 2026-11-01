param([Parameter(Mandatory)][string]$ToolDir)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$root = [IO.Path]::GetFullPath($ToolDir)
if ($root -eq $repo -or $root.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Keep installer tools outside the source repository.' }
$version = '7.1.1'
$url = 'https://github.com/jrsoftware/issrc/releases/download/is-7_1_1/innosetup-7.1.1-x64.exe'
# Official immutable release asset digest; verified 2026-11-01.
# https://github.com/jrsoftware/issrc/releases/tag/is-7_1_1
$expected = '8A5A6711C3766A6E5FB74F280BA0E3551E719CE94538D901FDBD1F8F9920B3F0'
$compiler = Join-Path $root 'ISCC.exe'
$receipt = Join-Path $root 'octave-compiler.json'
if ((Test-Path -LiteralPath $compiler) -and (Test-Path -LiteralPath $receipt)) {
    $known = Get-Content -LiteralPath $receipt -Raw | ConvertFrom-Json
    if ($known.version -ne $version -or (Get-FileHash -LiteralPath $compiler).Hash -ne $known.compilerSha256) { throw 'Cached compiler does not match its receipt.' }
    Write-Output $compiler
    return
}
if ((Test-Path -LiteralPath $root) -and (Get-ChildItem -LiteralPath $root -Force)) { throw 'Use an empty directory for the portable installer compiler.' }
New-Item -ItemType Directory -Path $root -Force | Out-Null
$download = $root + '.download.exe'
Invoke-WebRequest -Uri $url -OutFile $download -TimeoutSec 120
if ((Get-FileHash -LiteralPath $download).Hash -ne $expected) { throw 'Installer compiler checksum mismatch; downloaded program was not executed.' }
$signature = Get-AuthenticodeSignature -LiteralPath $download
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notlike '*O=Pyrsys B.V.*') { throw 'Installer compiler publisher verification failed.' }
$arguments = @('/PORTABLE=1','/CURRENTUSER','/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/SP-','/NOICONS',('/DIR="' + $root + '"'))
$process = Start-Process -FilePath $download -ArgumentList $arguments -WindowStyle Hidden -PassThru
if (-not $process.WaitForExit(60000)) { throw 'Portable compiler extraction timed out.' }
if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $compiler)) { throw 'Portable compiler extraction failed.' }
@{ version=$version; url=$url; downloadSha256=$expected; compilerSha256=(Get-FileHash -LiteralPath $compiler).Hash; portable=$true } | ConvertTo-Json | Set-Content -LiteralPath $receipt -Encoding utf8
Remove-Item -LiteralPath $download
Write-Output $compiler
