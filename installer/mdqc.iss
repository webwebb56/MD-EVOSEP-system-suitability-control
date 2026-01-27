; MD QC Agent Installer Script for Inno Setup
; https://jrsoftware.org/isinfo.php

#define MyAppName "MD QC Agent"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "Mass Dynamics"
#define MyAppURL "https://massdynamics.com"
#define MyAppExeName "mdqc.exe"

[Setup]
; Unique identifier for this application
AppId={{A1B2C3D4-E5F6-7890-ABCD-EF1234567890}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}

; Installation directory
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}

; Output settings
OutputDir=..\target\installer
OutputBaseFilename=mdqc-setup-{#MyAppVersion}
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}

; Compression
Compression=lzma2
SolidCompression=yes

; Privileges - allow per-user install without admin
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog

; Windows version
MinVersion=10.0

; Wizard settings
WizardStyle=modern
WizardSizePercent=100

; Allow user to create desktop icon
DisableProgramGroupPage=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "startonboot"; Description: "Start MD QC Agent when Windows starts"; GroupDescription: "Startup:"; Flags: checkedonce

[Files]
; Main executable
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion

; Icon file
Source: "..\assets\icon.ico"; DestDir: "{app}"; Flags: ignoreversion

; Default config template
Source: "config.template.toml"; DestDir: "{app}"; DestName: "config.template.toml"; Flags: ignoreversion

[Dirs]
; Create config and data directories
Name: "{commonappdata}\MassDynamics\QC"
Name: "{commonappdata}\MassDynamics\QC\templates"
Name: "{commonappdata}\MassDynamics\QC\logs"
Name: "{commonappdata}\MassDynamics\QC\spool"

[Icons]
; Start Menu
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Parameters: "tray"; Comment: "MD QC Agent - System Tray"
Name: "{autoprograms}\{#MyAppName} Diagnostics"; Filename: "{app}\{#MyAppExeName}"; Parameters: "doctor"; Comment: "Run system diagnostics"
Name: "{autoprograms}\{#MyAppName} Configuration"; Filename: "notepad.exe"; Parameters: """{commonappdata}\MassDynamics\QC\config.toml"""; Comment: "Edit configuration"

; Desktop icon (optional)
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Parameters: "tray"; Tasks: desktopicon

[Registry]
; Add to Windows startup if selected
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "MDQCAgent"; ValueData: """{app}\{#MyAppExeName}"" tray"; Flags: uninsdeletevalue; Tasks: startonboot

[Run]
; Copy template config if config doesn't exist
Filename: "cmd.exe"; Parameters: "/c if not exist ""{commonappdata}\MassDynamics\QC\config.toml"" copy ""{app}\config.template.toml"" ""{commonappdata}\MassDynamics\QC\config.toml"""; Flags: runhidden

; Launch tray after install
Filename: "{app}\{#MyAppExeName}"; Parameters: "tray"; Description: "Launch MD QC Agent"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Stop any running instance before uninstall
Filename: "taskkill.exe"; Parameters: "/F /IM {#MyAppExeName}"; Flags: runhidden; RunOnceId: "KillApp"

[Code]
// Check if Skyline is installed and show a message if not
procedure CurPageChanged(CurPageID: Integer);
var
  SkylinePath: String;
begin
  if CurPageID = wpFinished then
  begin
    // Check for Skyline in common locations
    SkylinePath := ExpandConstant('{localappdata}\Apps\2.0');
    if not DirExists(SkylinePath) then
    begin
      MsgBox('Note: Skyline does not appear to be installed.' + #13#10 + #13#10 +
             'MD QC Agent requires Skyline for data extraction.' + #13#10 +
             'Download Skyline from: https://skyline.ms', mbInformation, MB_OK);
    end;
  end;
end;
