param(
    [Parameter(Mandatory)][string]$PackageDir,
    [Parameter(Mandatory)][string]$WorkDir
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$root = [IO.Path]::GetFullPath($WorkDir)
if ($root -eq $repo -or $root.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Registration QA must be outside the repository.' }
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Octave'
$shortcutPath = Join-Path ([Environment]::GetFolderPath('Programs')) 'Octave.lnk'
# Never replace a user's existing registration, including an orphaned shortcut.
if ((Test-Path -LiteralPath $key) -or (Test-Path -LiteralPath $shortcutPath)) { throw 'Existing Octave registration found; use a separate Windows test account or clean VM.' }
$package = (Resolve-Path -LiteralPath $PackageDir).Path
$manifest = Get-Content -LiteralPath (Join-Path $package 'manifest.json') -Raw | ConvertFrom-Json
if ($manifest.product -ne 'Octave') { throw 'Not an Octave package.' }
foreach ($entry in $manifest.files) {
    if ($entry.name -ne [IO.Path]::GetFileName($entry.name) -or $entry.name -match '[\\/:]' -or $entry.name -in '.', '..') { throw 'Invalid package filename.' }
    if ((Get-FileHash -LiteralPath (Join-Path $package $entry.name)).Hash -ne $entry.sha256) { throw 'Package checksum mismatch.' }
}
$root = Join-Path $root ('registration-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root -Force | Out-Null
$target = Join-Path $root 'Octave'
$profile = Join-Path $root 'profile'
$savedPath = $env:PATH
$installed = $false
try {
    & (Join-Path $package 'Install.ps1') -InstallDir $target
    $installed = $true
    $registered = Get-ItemProperty -LiteralPath $key
    if ($registered.DisplayName -ne 'Octave' -or $registered.DisplayVersion -ne $manifest.version -or $registered.InstallLocation -ne $target) { throw 'Uninstall registration mismatch.' }
    $expectedUninstall = 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "' + (Join-Path $target 'Uninstall.ps1') + '" -InstallDir "' + $target + '"'
    if ($registered.UninstallString -ne $expectedUninstall) { throw 'Uninstall command mismatch.' }
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($shortcutPath)
    if ($shortcut.TargetPath -ne (Join-Path $target 'octave-desktop.exe') -or $shortcut.WorkingDirectory -ne $target) { throw 'Start menu shortcut mismatch.' }
    # Explicit profile: never touch the normal listening database during QA.
    $env:PATH = "$env:SystemRoot\System32;$env:SystemRoot"
    $stdout = Join-Path $root 'launch-stdout.log'
    $stderr = Join-Path $root 'launch-stderr.log'
    $process = Start-Process -FilePath $shortcut.TargetPath -ArgumentList @('--smoke-test','--data-dir',('"' + $profile + '"')) -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    if (-not $process.WaitForExit(30000)) { $process.Kill(); $process.WaitForExit(); throw 'Registered launch timed out.' }
    if ($process.ExitCode -ne 0) {
        $detail = Get-Content -LiteralPath $stderr -Raw
        throw "Registered launch failed (exit $($process.ExitCode)). $detail"
    }
    $database = Join-Path $profile 'listening.sqlite3'
    $before = (Get-FileHash -LiteralPath $database).Hash
    Set-Content -LiteralPath (Join-Path $target 'user-note.txt') -Value 'preserve'
    & (Join-Path $target 'Uninstall.ps1') -InstallDir $target
    if ((Test-Path -LiteralPath $key) -or (Test-Path -LiteralPath $shortcutPath)) { throw 'Uninstall left registration behind.' }
    if (Test-Path -LiteralPath (Join-Path $target 'octave-desktop.exe')) { throw 'Uninstall retained the executable.' }
    if ((Get-FileHash -LiteralPath $database).Hash -ne $before -or (Get-Content -LiteralPath (Join-Path $target 'user-note.txt') -Raw).Trim() -ne 'preserve') { throw 'Uninstall changed preserved data.' }
    $installed = $false
    @{version=$manifest.version; registration=$true; shortcut=$true; restrictedPathLaunch=$true; unregistration=$true; profilePreserved=$true; unrelatedFilePreserved=$true; freshMachine=$false; computer=[Environment]::OSVersion.VersionString; packageManifestSha256=(Get-FileHash -LiteralPath (Join-Path $package 'manifest.json')).Hash} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'result.json') -Encoding utf8
    Write-Output "registration_acceptance=passed evidence=$root"
} finally {
    $env:PATH = $savedPath
    # Only our newly created registration can reach this cleanup branch.
    if ($installed -and (Test-Path -LiteralPath (Join-Path $target 'Uninstall.ps1'))) {
        & (Join-Path $target 'Uninstall.ps1') -InstallDir $target
    }
}
