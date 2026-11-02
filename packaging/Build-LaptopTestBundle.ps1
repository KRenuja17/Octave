param([Parameter(Mandatory)][string]$Setup, [Parameter(Mandatory)][string]$OutputDir)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$output = [IO.Path]::GetFullPath($OutputDir).TrimEnd('\')
if ($output -eq $repo -or $output.StartsWith($repo+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Test bundles must stay outside the source repository.' }
$file = Get-Item -LiteralPath $Setup -Force
if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Expected an ordinary Setup executable.' }
$stem = [IO.Path]::GetFileNameWithoutExtension($file.Name)
$receipt = Join-Path $file.DirectoryName ($stem+'.build.json')
$build = Get-Content -LiteralPath $receipt -Raw | ConvertFrom-Json
$version = $build.version
if ($version -notmatch '^\d+\.\d+\.\d+(-[A-Za-z0-9.]+)?$' -or $file.Name -cne "Octave-$version-Setup.exe" -or $build.installerDirty -ne $false -or $build.installerSourceCommit -notmatch '^[a-f0-9]{40}$' -or $build.payloadSourceCommit -notmatch '^[a-f0-9]{40}$' -or $build.setupSha256 -ne (Get-FileHash -LiteralPath $file.FullName).Hash) { throw 'Expected a verified clean-source Setup and its original receipt.' }
$signature = (Get-AuthenticodeSignature -LiteralPath $file.FullName).Status.ToString()
if ($signature -cne 'NotSigned') { throw 'This builder is for explicitly unsigned evaluation handoff, not signed production.' }
& (Join-Path $PSScriptRoot 'Test-SetupMetadata.ps1') -Executable $file.FullName -Version $version | Out-Host
$sidecar = Join-Path $file.DirectoryName ($stem+'.sha256')
$expected = $build.setupSha256+'  '+$file.Name
if ([IO.File]::ReadAllText($sidecar).Trim() -ine $expected) { throw 'Setup checksum sidecar mismatch.' }
$name = "Octave-$version-Laptop-Test"
$destination = Join-Path $output $name
$archive = $destination+'.zip'
if ((Test-Path -LiteralPath $destination) -or (Test-Path -LiteralPath $archive) -or (Test-Path -LiteralPath ($archive+'.sha256'))) { throw 'Keep test artifacts immutable; choose another output directory.' }
New-Item -Path $destination -ItemType Directory -Force | Out-Null
# Explicit allowlist: never archive a research directory, app profile or browser.
foreach ($source in @($file.FullName,$receipt,$sidecar,(Join-Path $PSScriptRoot 'Test-LaptopEnvironment.ps1'))) { Copy-Item -LiteralPath $source -Destination $destination }
$guide = @'
Octave laptop evaluation - test whenever convenient

This is an UNSIGNED evaluation installer, not a signed production release.
No account data, cookies, browser profiles or credentials from the developer
machine are included. It runs without Rust, Visual Studio, GCC or Inno Setup.

1. Copy this ZIP and its .sha256 file to the laptop. Before extraction, compare
   Get-FileHash -Algorithm SHA256 <zip> with the independently supplied checksum.
   The internal hashes detect accidental changes; they are not publisher signing.
2. Extract the whole folder. Read the installer version and checksum sidecar.
3. Optional: open Windows PowerShell in this folder and run:
   .\Test-LaptopEnvironment.ps1 -OutputDir .\Results
   If script execution is restricted, leave it restricted and record the message.
   The report stays local and contains only OS/CPU/RAM/GPU/display/input-method
   facts, artifact hashes and blank check results. It does not read Google/YouTube
   data, the personal Octave profile, browser storage or machine/user identifiers.
   It installs nothing and does not launch Octave. Read it before sharing.
4. Open Octave-<version>-Setup.exe. Record any Windows/browser/SmartScreen warning
   exactly, including the publisher. If blocked, stop and record it; keep Windows
   security protections enabled. Unsigned results cannot qualify signed production.
5. Use the current-user default destination: LocalAppData\Programs\Octave.
   Launch Octave from Start. Check Home, Library, Search, Settings, Account,
   bottom player, compact resizing, minimize/restore and artwork/text rendering.
   Use local test data and signed-out playback where available; signing in is
   unnecessary. Record startup time and idle memory in Task Manager if practical.
6. Make a local test playlist/preference. Quit and rerun the same Setup for repair;
   confirm the data survives. Same-version repair is not a version upgrade.
   Test an earlier-version upgrade only if a separately reviewed baseline exists.
7. Check keyboard/Narrator and mixed Arabic/Hebrew/Latin, marks, emoji, selection,
   deletion and undo. Composition needs an already installed IME; do not install
   one for this test. Physical DPI transitions need two connected displays with
   different scales. Mark unavailable checks honestly, not passed.
8. Quit and uninstall through Windows Installed apps. Check the Start entry and
   program files are removed while local test/profile data is preserved. Octave's
   data directory is separate: LocalAppData\Octave. Leave personal data intact.

Record each result as passed, failed, not run, unavailable or not applicable.
The optional JSON report supplies a blank checklist; a text note is also fine.
Keep screenshots free of account names, notifications and private library data.
All hands-on checks remain unverified until performed. Automatic updates remain
disabled. Signing/provider approval and a later signed-file retest are separate.

For SmartScreen/download-reputation evidence, a USB/local ZIP transfer is only
installation/rendering evidence. The final signed release must also be downloaded
through a browser from its reviewed release URL with normal security intact.
'@
[IO.File]::WriteAllText((Join-Path $destination 'TEST-GUIDE.txt'),$guide,[Text.UTF8Encoding]::new($false))
$files = @(Get-ChildItem -LiteralPath $destination -File | ForEach-Object { @{name=$_.Name;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash} })
@{product='Octave';version=$version;setupSha256=$build.setupSha256;payloadSourceCommit=$build.payloadSourceCommit;installerSourceCommit=$build.installerSourceCommit;signatureStatus='NotSigned';containsAccountData=$false;productionAcceptance=$false;files=$files} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $destination 'test-bundle.json') -Encoding utf8
Compress-Archive -LiteralPath $destination -DestinationPath $archive
$hash = (Get-FileHash -LiteralPath $archive).Hash
($hash.ToLowerInvariant()+'  '+[IO.Path]::GetFileName($archive)) | Set-Content -LiteralPath ($archive+'.sha256') -Encoding ascii
Write-Output $archive
