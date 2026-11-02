param(
    [Parameter(Mandatory)][string]$Setup,
    [Parameter(Mandatory)][string]$PackageDir,
    [Parameter(Mandatory)][string]$WorkDir,
    [string]$PreviousPackageDir,
    [string]$SoftwareOpenGlDir,
    [ValidatePattern('^[A-Fa-f0-9]{40}$')][string]$PublisherThumbprint,
    [ValidatePattern('^[A-Fa-f0-9]{64}$')][string]$ExpectedGuardSha256,
    [ValidatePattern('^[A-Fa-f0-9]{64}$')][string]$ExpectedUninstallerSha256
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$work = [IO.Path]::GetFullPath($WorkDir)
$package = (Resolve-Path -LiteralPath $PackageDir).Path
if ($work -eq $repo -or $work.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase) -or $work.StartsWith($package + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Setup tests must stay outside source and package directories.' }
$setupFile = (Resolve-Path -LiteralPath $Setup).Path
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Octave_is1'
$legacyKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Octave'
$shortcutPath = Join-Path ([Environment]::GetFolderPath('Programs')) 'Octave.lnk'
if ((Test-Path -LiteralPath $key) -or (Test-Path -LiteralPath $legacyKey) -or (Test-Path -LiteralPath $shortcutPath)) { throw 'An Octave registration already exists. Use a clean Windows test account; do not overwrite the user installation.' }
$manifest = Get-Content -LiteralPath (Join-Path $package 'manifest.json') -Raw | ConvertFrom-Json
$signedCheck = [bool]$PublisherThumbprint
if ($signedCheck -ne [bool]$ExpectedGuardSha256 -or $signedCheck -ne [bool]$ExpectedUninstallerSha256) { throw 'Signed Setup checks require the approved publisher and both installed material hashes.' }
if ($signedCheck) {
    & (Join-Path $PSScriptRoot 'Test-ReleaseSignatures.ps1') -PackageDir $package -PublisherThumbprint $PublisherThumbprint | Out-Host
    & (Join-Path $PSScriptRoot 'Test-TrustedSignature.ps1') -File $setupFile -PublisherThumbprint $PublisherThumbprint
    & (Join-Path $PSScriptRoot 'Test-SetupMetadata.ps1') -Executable $setupFile -Version $manifest.version | Out-Host
}
$root = Join-Path $work ('setup-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root -Force | Out-Null
$profile = Join-Path $root 'synthetic-profile'
$target = Join-Path $root 'fresh\Octave'
$savedPath = $env:PATH
$savedDriver = $env:GALLIUM_DRIVER
$savedSoftware = $env:LIBGL_ALWAYS_SOFTWARE
$installed = $false
$legacyOwned = $false
function Invoke-Setup([string]$Name, [string]$Destination, [switch]$Reject) {
    $arguments = @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/SP-','/TASKS=""',('/DIR="' + $Destination + '"'),('/LOG="' + (Join-Path $root ($Name + '.log')) + '"'))
    $process = Start-Process -FilePath $setupFile -ArgumentList $arguments -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(30000)) { throw 'Native setup timed out.' }
    if (($process.ExitCode -eq 0) -eq [bool]$Reject) { throw "Unexpected setup result: $Name, exit $($process.ExitCode)" }
}
function Assert-Installed {
    foreach ($entry in $manifest.files) {
        if ((Get-FileHash -LiteralPath (Join-Path $target $entry.name)).Hash -ne $entry.sha256) { throw "Installed payload hash mismatch: $($entry.name)" }
    }
    if ($signedCheck) {
        foreach ($name in @('Setup-Guard.ps1','unins000.exe')) {
            $file = Join-Path $target $name
            $expected = if ($name -eq 'Setup-Guard.ps1') { $ExpectedGuardSha256 } else { $ExpectedUninstallerSha256 }
            if ((Get-FileHash -LiteralPath $file).Hash -ne $expected) { throw "Installed signing material differs: $name" }
            & (Join-Path $PSScriptRoot 'Test-TrustedSignature.ps1') -File $file -PublisherThumbprint $PublisherThumbprint
        }
    }
    $registration = Get-ItemProperty -LiteralPath $key
    if ($registration.DisplayName -ne 'Octave' -or $registration.DisplayVersion -ne $manifest.version -or $registration.InstallLocation.TrimEnd('\') -ne $target) { throw 'Native uninstall registration mismatch.' }
    if ($registration.UninstallString -notlike ('*' + $target + '\unins000.exe*')) { throw 'Native uninstaller is not registered.' }
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcutPath)
    if ($link.TargetPath -ne (Join-Path $target 'octave-desktop.exe') -or $link.WorkingDirectory -ne $target) { throw 'Start menu shortcut mismatch.' }
}
function Invoke-Uninstall {
    $process = Start-Process -FilePath (Join-Path $target 'unins000.exe') -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART') -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(30000) -or $process.ExitCode -ne 0) { throw 'Native uninstall failed.' }
    if ((Test-Path -LiteralPath $key) -or (Test-Path -LiteralPath $shortcutPath) -or (Test-Path -LiteralPath (Join-Path $target 'octave-desktop.exe'))) { throw 'Native uninstall left managed registration or app files.' }
}
try {
    Invoke-Setup 'fresh-install' $target
    $installed = $true
    Assert-Installed
    Set-Content -LiteralPath (Join-Path $target 'user-note.txt') -Value 'preserve'
    if ($SoftwareOpenGlDir) {
        foreach ($name in @('opengl32.dll','libgallium_wgl.dll')) { Copy-Item -LiteralPath (Join-Path $SoftwareOpenGlDir $name) -Destination (Join-Path $target $name) }
        [IO.File]::WriteAllText((Join-Path $target 'octave-desktop.exe.local'), 'CI-only per-application OpenGL redirection')
        $env:GALLIUM_DRIVER = 'llvmpipe'
        $env:LIBGL_ALWAYS_SOFTWARE = 'true'
    }
    $env:PATH = "$env:SystemRoot\System32;$env:SystemRoot"
    $process = Start-Process -FilePath (Join-Path $target 'octave-desktop.exe') -ArgumentList @('--smoke-test','--page','library','--data-dir',('"' + $profile + '"')) -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(30000)) { $process.Kill(); throw 'Installed app launch timed out.' }
    if ($process.ExitCode -ne 0) { throw 'Installed app launch failed.' }
    $profileHash = (Get-FileHash -LiteralPath (Join-Path $profile 'listening.sqlite3')).Hash
    Set-Content -LiteralPath (Join-Path $target 'README.md') -Value 'Synthetic damaged managed file'
    Invoke-Setup 'repair' $target
    Assert-Installed
    if ((Get-FileHash -LiteralPath (Join-Path $profile 'listening.sqlite3')).Hash -ne $profileHash) { throw 'Repair changed the profile.' }
    $unowned = Join-Path $root 'unrelated\Octave'
    New-Item -ItemType Directory -Path $unowned -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $unowned 'keep.txt') -Value 'untouched'
    Invoke-Setup 'unowned-folder-rejected' $unowned -Reject
    Assert-Installed
    if ((Get-Content -LiteralPath (Join-Path $unowned 'keep.txt')).Trim() -ne 'untouched') { throw 'Setup changed unrelated data.' }
    Invoke-Uninstall
    $installed = $false
    if ((Get-Content -LiteralPath (Join-Path $target 'user-note.txt')).Trim() -ne 'preserve' -or (Get-FileHash -LiteralPath (Join-Path $profile 'listening.sqlite3')).Hash -ne $profileHash) { throw 'Uninstall changed preserved data.' }

    $target = Join-Path $root 'legacy\Octave'
    $previous = if ($PreviousPackageDir) { $PreviousPackageDir } else { $package }
    & (Join-Path $previous 'Install.ps1') -InstallDir $target -NoRegistration | Out-Host
    New-Item -Path $legacyKey | Out-Null
    New-ItemProperty -Path $legacyKey -Name InstallLocation -Value $target | Out-Null
    $legacyOwned = $true
    Set-Content -LiteralPath (Join-Path $target 'user-note.txt') -Value 'preserve'
    Invoke-Setup 'legacy-upgrade' $target
    $installed = $true
    Assert-Installed
    if (Test-Path -LiteralPath $legacyKey) { throw 'Matching legacy registration was not migrated.' }
    $legacyOwned = $false
    Invoke-Uninstall
    $installed = $false
    if ((Get-Content -LiteralPath (Join-Path $target 'user-note.txt')).Trim() -ne 'preserve' -or (Get-FileHash -LiteralPath (Join-Path $profile 'listening.sqlite3')).Hash -ne $profileHash) { throw 'Upgrade/uninstall changed preserved data.' }
    @{version=$manifest.version; setupSha256=(Get-FileHash -LiteralPath $setupFile).Hash; manifestSha256=(Get-FileHash -LiteralPath (Join-Path $package 'manifest.json')).Hash; freshInstall=$true; repair=$true; legacyUpgrade=$true; startMenu=$true; windowsUninstall=$true; restrictedPathLaunch=$true; profilePreserved=$true; unrelatedFilesPreserved=$true; unownedTargetRejected=$true; signedSetupAndInstalledMaterialsVerified=$signedCheck; normalUserProfileUsed=$false; freshConsumerMachine=$false; graphicsEnvironment=$(if($SoftwareOpenGlDir){'CI-only Mesa llvmpipe'}else{'host driver'})} | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $root 'result.json') -Encoding utf8
    Write-Output "native_setup_acceptance=passed evidence=$root"
} finally {
    $env:PATH = $savedPath
    $env:GALLIUM_DRIVER = $savedDriver
    $env:LIBGL_ALWAYS_SOFTWARE = $savedSoftware
    if ($installed -and (Test-Path -LiteralPath (Join-Path $target 'unins000.exe'))) { Invoke-Uninstall }
    if ($legacyOwned -and (Test-Path -LiteralPath $legacyKey) -and (Get-ItemProperty -LiteralPath $legacyKey).InstallLocation -eq $target) { Remove-Item -LiteralPath $legacyKey }
}
