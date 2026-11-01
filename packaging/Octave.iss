; The builder supplies only manifest-listed payloads through an external include.
#ifndef OctaveVersion
  #error Build with packaging/Build-Setup.ps1
#endif

[Setup]
AppId=Octave
AppName=Octave
AppVersion={#OctaveVersion}
AppPublisher=Octave contributors
AppPublisherURL=https://github.com/KRenuja17/Octave
AppSupportURL=https://github.com/KRenuja17/Octave/issues
DefaultDirName={localappdata}\Programs\Octave
DefaultGroupName=Octave
PrivilegesRequired=lowest
SetupMutex=Octave.Setup
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupArchitecture=x64
MinVersion=10.0
WizardStyle=modern dynamic
DisableWelcomePage=no
DisableProgramGroupPage=yes
DisableDirPage=no
LicenseFile={#PackageDir}\LICENSE
OutputDir={#SetupOutputDir}
OutputBaseFilename=Octave-{#OctaveVersion}-Setup
Compression=lzma2
SolidCompression=yes
CloseApplications=no
RestartApplications=no
UninstallDisplayIcon={app}\octave-desktop.exe
UninstallDisplayName=Octave
VersionInfoVersion={#NumericVersion}
VersionInfoProductName=Octave
VersionInfoProductVersion={#NumericVersion}
VersionInfoTextVersion={#OctaveVersion}
VersionInfoProductTextVersion={#OctaveVersion}
VersionInfoDescription=Octave Setup
VersionInfoCompany=Octave contributors
VersionInfoOriginalFileName=Octave-{#OctaveVersion}-Setup.exe

[Tasks]
Name: desktopicon; Description: "Create a &desktop shortcut"; Flags: unchecked

[Files]
#include PayloadInclude
; Also embed a guard for upgrades before installed files are changed.
Source: "{#GuardPath}"; DestDir: "{app}"; DestName: "Setup-Guard.ps1"; Flags: ignoreversion
Source: "{#GuardPath}"; DestName: "octave-setup-guard.ps1"; Flags: dontcopy

[Icons]
Name: "{autoprograms}\Octave"; Filename: "{app}\octave-desktop.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\Octave"; Filename: "{app}\octave-desktop.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\octave-desktop.exe"; Description: "Open Octave"; Flags: nowait postinstall skipifsilent unchecked

[Code]
function CheckTarget(Guard: String): Boolean;
var
  ExitCode: Integer;
begin
  Result := Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + Guard +
    '" -InstallDir "' + ExpandConstant('{app}') + '"', '', SW_HIDE, ewWaitUntilTerminated, ExitCode)
    and (ExitCode = 0);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  ExtractTemporaryFile('octave-setup-guard.ps1');
  if not CheckTarget(ExpandConstant('{tmp}\octave-setup-guard.ps1')) then
    Result := 'Octave cannot install in this folder. Quit the installed Octave app and choose a local folder ending in Octave, separate from your data and source code.'
  else
    Result := '';
end;

function InitializeUninstall: Boolean;
begin
  Result := CheckTarget(ExpandConstant('{app}\Setup-Guard.ps1'));
  if not Result then
    MsgBox('Quit Octave before uninstalling. If the installation was changed, repair it by running Setup again first.', mbError, MB_OK);
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  LegacyLocation: String;
begin
  // Replace a matching old PowerShell install entry only after a successful install.
  if (CurStep = ssDone) and
    RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Octave', 'InstallLocation', LegacyLocation) and
    (CompareText(RemoveBackslashUnlessRoot(LegacyLocation), ExpandConstant('{app}')) = 0) then
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Octave');
end;
