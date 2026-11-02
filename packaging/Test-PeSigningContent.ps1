param([Parameter(Mandatory)][string]$UnsignedFile, [Parameter(Mandatory)][string]$SignedFile)
$ErrorActionPreference = 'Stop'
foreach ($path in @($UnsignedFile,$SignedFile)) {
    $item = Get-Item -LiteralPath $path -Force
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -gt 256MB) { throw 'Expected a bounded ordinary PE file.' }
}
$original = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $UnsignedFile).Path)
$signed = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $SignedFile).Path)
if ($original.Length -lt 256 -or $signed.Length -le $original.Length) { throw 'Expected an appended PE signature.' }
$pe = [BitConverter]::ToInt32($original,0x3c)
if ($pe -lt 64 -or $pe -gt $original.Length-256 -or [BitConverter]::ToUInt32($original,$pe) -ne 0x4550) { throw 'Invalid PE header.' }
$optional = $pe + 24
$magic = [BitConverter]::ToUInt16($original,$optional)
if ($magic -eq 0x20b) { $security = $optional + 144 } elseif ($magic -eq 0x10b) { $security = $optional + 128 } else { throw 'Unsupported PE optional header.' }
if ([BitConverter]::ToUInt64($original,$security) -ne 0) { throw 'Expected an originally unsigned PE file.' }
$address = [BitConverter]::ToUInt32($signed,$security)
$size = [BitConverter]::ToUInt32($signed,($security+4))
if ($address -lt $original.Length -or $address -gt $original.Length+7 -or $address % 8 -ne 0 -or $size -lt 8 -or $size -gt 1MB -or [long]$address+$size -ne $signed.Length) { throw 'Invalid appended PE certificate range.' }
for ($offset=$original.Length; $offset -lt $address; $offset++) { if ($signed[$offset] -ne 0) { throw 'Unexpected data before PE signature.' } }
$prefix = [byte[]]::new($original.Length)
[Array]::Copy($signed,$prefix,$original.Length)
# Authenticode changes only CheckSum and the certificate directory in the PE
# headers, and appends the aligned certificate table. All code/data must match.
foreach ($bytes in @($original,$prefix)) {
    [Array]::Clear($bytes,($optional+64),4)
    [Array]::Clear($bytes,$security,8)
}
$hash = [Security.Cryptography.SHA256]::Create()
try {
    if ([Convert]::ToBase64String($hash.ComputeHash($original)) -cne [Convert]::ToBase64String($hash.ComputeHash($prefix))) { throw 'Provider changed PE code or data while signing.' }
} finally { $hash.Dispose() }
