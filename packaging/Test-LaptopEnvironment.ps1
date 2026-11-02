param([string]$OutputDir = (Join-Path $PSScriptRoot 'Results'))
$ErrorActionPreference = 'Stop'
$bundle = $PSScriptRoot
$manifest = Get-Content -LiteralPath (Join-Path $bundle 'test-bundle.json') -Raw | ConvertFrom-Json
if ($manifest.product -cne 'Octave' -or $manifest.version -notmatch '^\d+\.\d+\.\d+(-[A-Za-z0-9.]+)?$' -or $manifest.signatureStatus -cne 'NotSigned') { throw 'Expected an Octave unsigned laptop evaluation bundle.' }
$name = 'Octave-'+$manifest.version+'-Setup'
$allowed = @(($name+'.exe'),($name+'.sha256'),($name+'.build.json'),'TEST-GUIDE.txt','Test-LaptopEnvironment.ps1')
if (@($manifest.files).Count -ne $allowed.Count -or @($manifest.files.name | Sort-Object -Unique).Count -ne $allowed.Count) { throw 'Invalid test-bundle file list.' }
foreach ($entry in $manifest.files) {
    if ($entry.name -cnotin $allowed -or $entry.sha256 -notmatch '^[A-Fa-f0-9]{64}$') { throw 'Unexpected bundle file.' }
    $item = Get-Item -LiteralPath (Join-Path $bundle $entry.name) -Force
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or (Get-FileHash -LiteralPath $item.FullName).Hash -ne $entry.sha256) { throw 'Test-bundle checksum mismatch or linked file.' }
}
$setup = Join-Path $bundle ($name+'.exe')
if ((Get-FileHash -LiteralPath $setup).Hash -ne $manifest.setupSha256 -or (Get-AuthenticodeSignature -LiteralPath $setup).Status.ToString() -cne 'NotSigned') { throw 'Setup differs from the declared unsigned evaluation file.' }
$output = [IO.Path]::GetFullPath($OutputDir)
$parent = $output
while ($parent) {
    if (Test-Path -LiteralPath (Join-Path $parent '.git')) { throw 'Keep environment reports outside Git checkouts.' }
    if (Test-Path -LiteralPath $parent) {
        if ((Get-Item -LiteralPath $parent -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Do not write reports through directory links.' }
    }
    $next = Split-Path $parent -Parent
    if ($next -eq $parent) { break }
    $parent = $next
}
$os = Get-CimInstance Win32_OperatingSystem
$system = Get-CimInstance Win32_ComputerSystem
$cpu = @(Get-CimInstance Win32_Processor | Select-Object Name)
$gpu = @(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion,CurrentHorizontalResolution,CurrentVerticalResolution)
$languages = @(Get-WinUserLanguageList | Select-Object LanguageTag,InputMethodTips)
Add-Type -AssemblyName System.Windows.Forms
$displays = @([Windows.Forms.Screen]::AllScreens | ForEach-Object { @{width=$_.Bounds.Width;height=$_.Bounds.Height;primary=$_.Primary} })
$report = [ordered]@{
    version=$manifest.version;setupSha256=$manifest.setupSha256;signatureStatus='NotSigned';bundleIntegrity='passed';productionAcceptance=$false
    os=@{name=$os.Caption;version=$os.Version;build=$os.BuildNumber;architecture=$os.OSArchitecture}
    cpu=$cpu;ramGiB=[math]::Round($system.TotalPhysicalMemory/1GB,1);graphics=$gpu;activeDisplays=$displays;inputMethods=$languages
    cleanConsumerEnvironment='not established';normalProfileRead=$false;automaticUpdatesEnabled=$false
    manualChecks=@('browser download/SmartScreen','fresh installation','Home/Library/Search/rendering','startup and idle memory','same-version repair/data preservation','earlier-version upgrade if available','keyboard/Narrator/RTL/grapheme editing','composition with an already-installed IME','physical different-DPI display transitions if available','uninstall and preserved data') | ForEach-Object { @{check=$_;status='not run';notes=''} }
}
New-Item -Path $output -ItemType Directory -Force | Out-Null
$resultPath = Join-Path $output ('environment-'+[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss')+'-'+[Guid]::NewGuid().ToString('N').Substring(0,8)+'.json')
$report | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath $resultPath -Encoding utf8
Write-Output "bundle_integrity=passed environment_report=$resultPath manual_acceptance=not-run"
