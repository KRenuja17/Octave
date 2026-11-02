param(
    [Parameter(Mandatory)][string]$File,
    [Parameter(Mandatory)][ValidatePattern('^[A-Fa-f0-9]{40}$')][string]$PublisherThumbprint
)
$ErrorActionPreference = 'Stop'
$item = Get-Item -LiteralPath $File -Force
if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Expected an ordinary signing target.' }
$signature = Get-AuthenticodeSignature -LiteralPath $item.FullName
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne $PublisherThumbprint -or -not $signature.TimeStamperCertificate) { throw "Trusted timestamped publisher signature required: $($item.Name)" }
$usages = @($signature.SignerCertificate.Extensions | Where-Object { $_.Oid.Value -eq '2.5.29.37' } | ForEach-Object { $_.EnhancedKeyUsages } | ForEach-Object { $_.Value })
if ('1.3.6.1.5.5.7.3.3' -notin $usages) { throw 'Publisher certificate lacks the code-signing usage.' }
