param(
    [Parameter(Mandatory)][string]$PackageDir,
    [Parameter(Mandatory)][string]$WorkDir,
    [Parameter(Mandatory)][ValidatePattern('^[A-Fa-f0-9]{40}$')][string]$PublisherThumbprint,
    [Parameter(Mandatory)][ValidatePattern('^[A-Fa-f0-9]{64}$')][string]$ExpectedManifestSha256,
    [switch]$CleanConsumerEnvironment,
    [switch]$RunRegistrationCheck,
    [string]$Setup,
    [ValidatePattern('^[A-Fa-f0-9]{64}$')][string]$ExpectedSetupSha256,
    [ValidatePattern('^[A-Fa-f0-9]{64}$')][string]$ExpectedGuardSha256,
    [ValidatePattern('^[A-Fa-f0-9]{64}$')][string]$ExpectedUninstallerSha256,
    [switch]$RunSetupCheck
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$package = (Resolve-Path -LiteralPath $PackageDir).Path
$root = [IO.Path]::GetFullPath($WorkDir)
foreach ($parent in @($repo,$package)) {
    if ($root -eq $parent -or $root.StartsWith($parent+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Consumer evidence must stay outside source and package directories.' }
}
$manifestPath = Join-Path $package 'manifest.json'
if ((Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash -ne $ExpectedManifestSha256) { throw 'Package differs from the reviewed signed manifest.' }
& (Join-Path $PSScriptRoot 'Test-ReleaseSignatures.ps1') -PackageDir $package -PublisherThumbprint $PublisherThumbprint | Out-Host
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if ([bool]$Setup -ne [bool]$ExpectedSetupSha256 -or ($RunSetupCheck -and (-not $Setup -or -not $ExpectedGuardSha256 -or -not $ExpectedUninstallerSha256)) -or ($RunSetupCheck -and $RunRegistrationCheck)) { throw 'Native Setup checks require the reviewed Setup/hash and installed-material hashes; choose one registration method.' }
if ($Setup) {
    if ((Get-FileHash -LiteralPath $Setup).Hash -ne $ExpectedSetupSha256) { throw 'Setup differs from the reviewed signed executable.' }
    & (Join-Path $PSScriptRoot 'Test-TrustedSignature.ps1') -File $Setup -PublisherThumbprint $PublisherThumbprint
    & (Join-Path $PSScriptRoot 'Test-SetupMetadata.ps1') -Executable $Setup -Version $manifest.version | Out-Host
}
foreach ($kind in @('desktop','signature')) {
    & (Join-Path $PSScriptRoot 'Test-ExecutableMetadata.ps1') -Executable (Join-Path $package "octave-$kind.exe") -Version $manifest.version -Kind $kind | Out-Host
}
$root = Join-Path $root ('consumer-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root -Force | Out-Null
$os = Get-CimInstance Win32_OperatingSystem
$system = Get-CimInstance Win32_ComputerSystem
$gpu = @(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion)
$languages = @(Get-WinUserLanguageList | Select-Object LanguageTag,InputMethodTips)
$buildTools = @('rustc','cargo','gcc','cl','cmake','msbuild' | Where-Object { Get-Command $_ -ErrorAction SilentlyContinue })
$automated = 'not run'
$native = 'not run'
if ($RunRegistrationCheck) {
    # Uses the real host GPU; no Mesa, runtime installation or system changes
    # beyond the bounded current-user install/shortcut/uninstall test.
    & (Join-Path $PSScriptRoot 'Test-RegisteredPackage.ps1') -PackageDir $package -WorkDir (Join-Path $root 'registration') | Out-Host
    $automated = 'passed'
}
if ($RunSetupCheck) {
    & (Join-Path $PSScriptRoot 'Test-Setup.ps1') -Setup $Setup -PackageDir $package -WorkDir (Join-Path $root 'native-setup') -PublisherThumbprint $PublisherThumbprint -ExpectedGuardSha256 $ExpectedGuardSha256 -ExpectedUninstallerSha256 $ExpectedUninstallerSha256 | Out-Host
    $native = 'passed'
}
@{
    version=$manifest.version
    packageManifestSha256=$ExpectedManifestSha256.ToUpperInvariant()
    publisherThumbprint=$PublisherThumbprint.ToUpperInvariant()
    os=@{name=$os.Caption;version=$os.Version;build=$os.BuildNumber;architecture=$os.OSArchitecture}
    ramGiB=[Math]::Round($system.TotalPhysicalMemory/1GB,1)
    graphics=$gpu
    inputMethods=$languages
    buildToolsFound=$buildTools
    cleanEnvironmentAttested=[bool]$CleanConsumerEnvironment
    signedPayloadVerification='passed'
    hostRegistrationCheck=$automated
    nativeSetupSha256=$ExpectedSetupSha256
    nativeSetupCheck=$native
    acceptance='pending attended review'
    manualChecks=@('browser-download and SmartScreen','Start menu and rendering','upgrade and data preservation','RTL and installed-IME composition','physical DPI transition if available','uninstall and preserved data')
} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $root 'result.json') -Encoding utf8
Write-Output "consumer_evidence=recorded path=$root acceptance=pending-attended-review"
