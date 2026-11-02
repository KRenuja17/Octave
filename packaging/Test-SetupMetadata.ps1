param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+(-[A-Za-z0-9.]+)?$')][string]$Version,
    [switch]$Uninstaller
)
$ErrorActionPreference = 'Stop'
$file = Get-Item -LiteralPath $Executable -Force
if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Expected an ordinary Setup executable.' }
$info = [Diagnostics.FileVersionInfo]::GetVersionInfo($file.FullName)
# Inno pads its resource strings. Never relax actual product/version equality.
if ($info.ProductName.TrimEnd() -cne 'Octave' -or $info.CompanyName.TrimEnd() -cne 'Octave contributors' -or $info.ProductVersion.TrimEnd() -cne $Version -or $info.OriginalFilename.TrimEnd() -cne "Octave-$Version-Setup.exe") { throw 'Native Setup product/version metadata mismatch.' }
$numbers = @($Version.Split('-')[0].Split('.') | ForEach-Object { [uint16]::Parse($_) }) + @(0)
if (($numbers -join '.') -ne (@($info.ProductMajorPart,$info.ProductMinorPart,$info.ProductBuildPart,$info.ProductPrivatePart) -join '.')) { throw 'Native Setup numeric product version mismatch.' }
if (-not $Uninstaller -and ($info.FileVersion.TrimEnd() -cne $Version -or $info.FileDescription.TrimEnd() -cne 'Octave Setup')) { throw 'Native Setup file metadata mismatch.' }
Write-Output "setup_metadata=passed uninstaller=$([bool]$Uninstaller) version=$Version"
