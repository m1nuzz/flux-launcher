#ifndef AppVersion
#define AppVersion "0.1.123"
#endif
#ifndef BuildDir
#define BuildDir "target\x86_64-pc-windows-msvc\release"
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
UninstallDisplayIcon={app}\flux-launcher.ico
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
Source: "flux-launcher.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BuildDir}\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Flux Launcher"; Filename: "{app}\{#AppExeName}"; IconFilename: "{app}\flux-launcher.ico"; IconIndex: 0

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Flux Launcher"; ValueData: "{code:StartupCommand}"; Flags: uninsdeletevalue; Tasks: startup

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Launch Flux Launcher now"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\{#AppExeName}"; Parameters: "--shutdown"; Flags: waituntilterminated skipifdoesntexist; RunOnceId: "FluxLauncherShutdown"

[UninstallDelete]
Type: filesandordirs; Name: "{app}"
Type: filesandordirs; Name: "{group}"
Type: filesandordirs; Name: "{userappdata}\FluxLauncher"

[Code]
function StartupCommand(Param: String): String;
begin
  Result := '"' + ExpandConstant('{app}\{#AppExeName}') + '" --startup';
end;

function NeedsVcRedist: Boolean;
begin
  // Flux is built with the MSVC toolchain and imports VCRUNTIME140.dll, which
  // the per-user installer cannot assume is present on a clean machine. {sys}
  // resolves to the 64-bit system folder under ArchitecturesInstallIn64BitMode.
  Result := not FileExists(ExpandConstant('{sys}\vcruntime140.dll'));
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Bytes: Int64;
  ResultCode: Integer;
  RedistPath: String;
begin
  if CurStep <> ssPostInstall then
    Exit;
  // Unattended installs (winget, silent self-update) already satisfy the runtime
  // through the WinGet PackageDependency, so never block a silent install on a dialog.
  if WizardSilent then
    Exit;
  if not NeedsVcRedist then
    Exit;

  if MsgBox('Flux Launcher needs the Microsoft Visual C++ 2015-2022 (x64) runtime, which is not installed on this PC.'#13#10#13#10 +
    'Install it now? Flux will try to download it from Microsoft. Selecting No will continue without it, and Flux Launcher may fail to start.',
    mbInformation, MB_YESNO) <> IDYES then
    Exit;

  RedistPath := ExpandConstant('{tmp}\VC_redist.x64.exe');
  try
    // Raises an exception on any network/server error. Empty SHA-256 means the
    // current Microsoft build is accepted rather than pinned to one version.
    Bytes := DownloadTemporaryFile('https://aka.ms/vs/17/release/vc_redist.x64.exe', 'VC_redist.x64.exe', '', nil);
  except
    if MsgBox('Flux Launcher could not download the Visual C++ runtime automatically (no internet connection or a Microsoft server error).'#13#10#13#10 +
      'Open the official Microsoft download page in your web browser?',
      mbError, MB_YESNO) = IDYES then
      ShellExec('open', 'https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist', '', '', SW_SHOW, ewNoWait, ResultCode);
    Exit;
  end;

  if (Bytes <= 0) or (not FileExists(RedistPath)) then
  begin
    if MsgBox('Flux Launcher could not download the Visual C++ runtime automatically.'#13#10#13#10 +
      'Open the official Microsoft download page in your web browser?', mbError, MB_YESNO) = IDYES then
      ShellExec('open', 'https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist', '', '', SW_SHOW, ewNoWait, ResultCode);
    Exit;
  end;

  // The redist bootstrapper is requireAdministrator: verb 'open' raises the
  // standard Microsoft UAC prompt, so the runtime install is not fully silent.
  if ShellExec('open', RedistPath, '/install /quiet /norestart', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
  begin
    // 0 = installed, 1638 = a runtime is already present, 3010 = installed, restart pending.
    if not ((ResultCode = 0) or (ResultCode = 1638) or (ResultCode = 3010)) then
      MsgBox('Flux Launcher was installed, but the Visual C++ runtime setup reported error ' + IntToStr(ResultCode) + '. Flux Launcher may fail to start until the Microsoft Visual C++ 2015-2022 Redistributable (x64) is installed.', mbError, MB_OK);
  end
  else
    MsgBox('Flux Launcher could not launch the downloaded Visual C++ runtime installer. Please install it from the official Microsoft website.', mbError, MB_OK);
end;
