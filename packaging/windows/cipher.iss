; Cipher's Windows installer (Inno Setup 6). Built by package.ps1, which passes
; /DAppVersion, /DSourceExe and /DOutputDir.
; Installs per user by default (no admin prompt), or for all users if chosen; adds a Start
; menu entry, an optional desktop icon, and opens .cipher files with Cipher.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\cipher.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\target\dist"
#endif

[Setup]
AppId={{27D6D891-184F-48B3-A964-A88D33174A9A}
AppName=Cipher
AppVersion={#AppVersion}
AppVerName=Cipher {#AppVersion}
AppPublisher=Cipher contributors
AppPublisherURL=https://github.com/Siddhj2206/cipher
DefaultDirName={autopf}\Cipher
DefaultGroupName=Cipher
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesAssociations=yes
SetupIconFile=..\icons\out\cipher.ico
UninstallDisplayIcon={app}\cipher.exe
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
OutputDir={#OutputDir}
OutputBaseFilename=Cipher-{#AppVersion}-windows-x64-setup

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "cipher.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Cipher"; Filename: "{app}\cipher.exe"
Name: "{autodesktop}\Cipher"; Filename: "{app}\cipher.exe"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Classes\.cipher"; ValueType: string; ValueName: ""; ValueData: "Cipher.Book"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Cipher.Book"; ValueType: string; ValueName: ""; ValueData: "Cipher Book"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Cipher.Book\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\cipher.exe,0"
Root: HKA; Subkey: "Software\Classes\Cipher.Book\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\cipher.exe"" ""%1"""

[Run]
Filename: "{app}\cipher.exe"; Description: "{cm:LaunchProgram,Cipher}"; Flags: nowait postinstall skipifsilent
