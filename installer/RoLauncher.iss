#ifndef Version
  #error Release Version must be supplied by build-installer.ps1
#endif
#ifndef Payload
  #error Payload must point to the complete packaged application
#endif

[Setup]
AppId={{F2E0EED4-6CF4-4FBA-9B4A-52E2C52BC5B5}
AppName=RoLauncher
AppVersion={#Version}
AppPublisher=oangsa
AppPublisherURL=https://github.com/oangsa/RoLauncher
AppSupportURL=https://github.com/oangsa/RoLauncher/issues
AppUpdatesURL=https://github.com/oangsa/RoLauncher/releases
DefaultDirName={localappdata}\Programs\RoLauncher
DefaultGroupName=RoLauncher
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
LicenseFile=..\LICENSE
SetupIconFile=..\assets\RoLauncher.ico
UninstallDisplayIcon={app}\RoLauncher.exe
DisableProgramGroupPage=yes
WizardStyle=modern
Compression=lzma2/normal
SolidCompression=yes
CloseApplications=no
RestartApplications=no
AppMutex=Local\RoLauncherSupervisor,Local\RbxToolsSupervisor
OutputBaseFilename=rolauncher-v{#Version}-setup-x64
#ifdef TestInstaller
Uninstallable=no
CreateUninstallRegKey=no
UsePreviousAppDir=no
#else
UsePreviousAppDir=yes
#endif

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#Payload}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
#ifndef TestInstaller
Name: "{userprograms}\RoLauncher"; Filename: "{app}\RoLauncher.exe"
Name: "{userdesktop}\RoLauncher"; Filename: "{app}\RoLauncher.exe"; Tasks: desktopicon
#endif

[Run]
Filename: "{app}\RoLauncher.exe"; Description: "Launch RoLauncher"; Flags: nowait postinstall skipifsilent

[Code]
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ExistingMS, ExistingLS, NewMS, NewLS: Cardinal;
begin
  Result := '';
  NewMS := ({#VersionMajor} shl 16) or {#VersionMinor};
  NewLS := {#VersionPatch} shl 16;
  if GetVersionNumbers(ExpandConstant('{app}\RoLauncher.exe'), ExistingMS, ExistingLS) then
    if (ExistingMS > NewMS) or ((ExistingMS = NewMS) and (ExistingLS > NewLS)) then
      Result := 'A newer version of RoLauncher is already installed. This installer will not downgrade it.';
end;
