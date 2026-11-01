param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+(-[A-Za-z0-9.]+)?$')][string]$Version,
    [ValidateSet('desktop','signature')][string]$Kind = 'desktop',
    [switch]$AllowDebug
)
$ErrorActionPreference = 'Stop'
$file = Get-Item -LiteralPath $Executable
if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Expected an ordinary executable file.' }
$info = [Diagnostics.FileVersionInfo]::GetVersionInfo($file.FullName)
$binary = 'octave-' + $Kind
$description = if ($Kind -eq 'desktop') { 'Octave desktop music client' } else { 'Octave contained playback worker' }
if ($info.ProductName -ne 'Octave' -or $info.FileDescription -ne $description -or $info.CompanyName -ne 'Octave contributors' -or $info.OriginalFilename -ne ($binary+'.exe') -or $info.InternalName -ne $binary) { throw 'Executable product metadata mismatch.' }
if ($info.FileVersion -cne $Version -or $info.ProductVersion -cne $Version) { throw 'Executable and package versions differ. Rebuild with OCTAVE_BUILD_VERSION set to the package version.' }
$numbers = @($Version.Split('-')[0].Split('.') | ForEach-Object { [uint16]::Parse($_) }) + @(0)
$fileParts = @($info.FileMajorPart, $info.FileMinorPart, $info.FileBuildPart, $info.FilePrivatePart)
$productParts = @($info.ProductMajorPart, $info.ProductMinorPart, $info.ProductBuildPart, $info.ProductPrivatePart)
if (($numbers -join '.') -ne ($fileParts -join '.') -or ($numbers -join '.') -ne ($productParts -join '.')) { throw 'Executable numeric version mismatch.' }
if ($info.IsPreRelease -ne $Version.Contains('-')) { throw 'Executable prerelease flag mismatch.' }
if ($info.IsDebug -and -not $AllowDebug) { throw 'Debug executable cannot pass release metadata qualification.' }
Write-Output "executable_metadata=passed product=Octave kind=$Kind version=$Version"
