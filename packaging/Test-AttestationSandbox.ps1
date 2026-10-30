param(
    [Parameter(Mandatory)][string]$Runtime,
    [Parameter(Mandatory)][string]$OutputDir
)
$ErrorActionPreference = 'Stop'
# Official Deno v2.9.8 Windows x64 executable, extracted from the verified release.
$expected = '924397800533D3EB0ED0C889B20AD70E18B970D93ACD8135BC043F425E528266'
if ((Get-FileHash -LiteralPath $Runtime -Algorithm SHA256).Hash -ne $expected) {
    throw 'Unqualified runtime: review and pin the executable before running this gate.'
}
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$output = [IO.Path]::GetFullPath($OutputDir)
if ($output -eq $repo -or $output.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Sandbox evidence must be outside the repository.'
}
$run = Join-Path $output ('permissions-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $run -Force | Out-Null
$script = Join-Path $PSScriptRoot 'attestation\permission-check.js'
$arguments = @('run','--no-config','--no-npm','--no-remote','--no-prompt',
    '--deny-read','--deny-write','--deny-net','--deny-env','--deny-run',
    '--deny-ffi','--deny-sys','--deny-import',('"' + $script + '"'))
$process = $null
$previousCache = $env:DENO_DIR
try {
    $env:DENO_DIR = Join-Path $run 'cache'
    $process = Start-Process -FilePath $Runtime -ArgumentList $arguments -WorkingDirectory $run `
        -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run 'stdout.json') `
        -RedirectStandardError (Join-Path $run 'stderr.txt')
    if (-not $process.WaitForExit(10000)) { throw 'Permission gate exceeded ten seconds.' }
    if ($process.ExitCode -ne 0) { throw 'Permission gate failed.' }
    $checks = Get-Content -LiteralPath (Join-Path $run 'stdout.json') -Raw | ConvertFrom-Json
    foreach ($name in @('read','write','net','env','run','ffi','sys')) {
        if ($checks.$name -isnot [bool] -or $checks.$name -ne $true) { throw "Permission not denied: $name" }
    }
    @{passed=$true; runtimeSha256=$expected; scriptSha256=(Get-FileHash -LiteralPath $script).Hash;
      remoteChallengeExecuted=$false; limitation='Permission checks do not prove immunity to runtime vulnerabilities.'} |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run 'result.json') -Encoding utf8
    Write-Output "attestation_permission_gate=passed evidence=$run"
} finally {
    if ($process -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    $env:DENO_DIR = $previousCache
}
