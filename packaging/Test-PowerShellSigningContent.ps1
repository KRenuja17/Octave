param([Parameter(Mandatory)][string]$UnsignedFile, [Parameter(Mandatory)][string]$SignedFile)
$ErrorActionPreference = 'Stop'
foreach ($path in @($UnsignedFile,$SignedFile)) {
    $item = Get-Item -LiteralPath $path -Force
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -gt 1MB) { throw 'Expected a bounded ordinary PowerShell script.' }
}
$original = [IO.File]::ReadAllText((Resolve-Path -LiteralPath $UnsignedFile).Path).Replace("`r`n","`n").TrimEnd("`n")
$signed = [IO.File]::ReadAllText((Resolve-Path -LiteralPath $SignedFile).Path).Replace("`r`n","`n").TrimEnd("`n")
$marker = "`n# SIG # Begin signature block`n"
$start = $signed.IndexOf($marker,[StringComparison]::Ordinal)
if ($start -lt 0 -or $signed.IndexOf($marker,$start+1,[StringComparison]::Ordinal) -ge 0) { throw 'Expected exactly one appended PowerShell signature block.' }
$prefix = $signed.Substring(0,$start).TrimEnd("`n")
$block = $signed.Substring($start+$marker.Length)
if ($prefix -cne $original -or $block -cnotmatch '\A(?:# [A-Za-z0-9+/=]+\n)+# SIG # End signature block\z') { throw 'Provider changed PowerShell code or appended non-signature content.' }
