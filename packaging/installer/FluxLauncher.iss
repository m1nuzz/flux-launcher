#ifndef AppVersion
#define AppVersion "0.1.61"
#endif
#ifndef BuildDir
#define BuildDir "target\\x86_64-pc-windows-msvc\\release"
#endif

#define AppName "Flux Launcher"
#define AppPublisher "m1nuzz"
#define AppExeName "flux-launcher.exe"
#define AppDescription "A lightweight native Windows 11 launcher and file search tool"
#define AppUrl "https://github.com/m1nuzz/flux-launcher"

[Setup]
AppId={{C8F1C4D4-8F5A-4E1A-96C0-8D4D8C3D6C4A}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases/latest
AppCopyright=Copyright (C) 2026 m1nuzz
DefaultDirName={localappdata}\Programs\Flux Launcher
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=commandline
CloseApplications=yes
RestartApplications=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64
OutputBaseFilename=FluxLauncher-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile=flux-launcher.ico
UninstallDisplayIcon={app}\{#AppExeName}
VersionInfoVersion={#AppVersion}
VersionInfoDescription={#AppDescription}
VersionInfoProductName={#AppName}
VersionInfoCompany={#AppPublisher}
LicenseFile=..\..\LICENSE
OutputDir=..\..\artifacts\installer

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "startup"; Description: "Start Flux Launcher automatically with Windows"; GroupDescription: "Windows startup:"

[Files]
Source: "{#BuildDir}\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\\Flux Launcher"; Filename: "{app}\\{#AppExeName}"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Flux Launcher"; ValueData: "{code:StartupCommand}"; Flags: uninsdeletevalue; Tasks: startup

[Code]
function StartupCommand(Param: String): String;
begin
  Result := '"' + ExpandConstant('{app}\{#AppExeName}') + '" --startup';
end;
