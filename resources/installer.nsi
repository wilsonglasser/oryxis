; System installer — installs to %ProgramFiles%\Oryxis for all users.
; Requires UAC elevation (RequestExecutionLevel admin); winget points
; here. The per-user variant lives in installer-user.nsi and runs
; without elevation under %LOCALAPPDATA%\Programs\Oryxis.
;
; Build-time defines (override from CI):
;   /DVERSION=0.5.2          — SemVer; falls back to 0.0.0-dev
;   /DARCH=x86_64|aarch64    — used in OutFile suffix; defaults to x86_64
;   /DBINPATH=..\target\release
;                            — directory holding oryxis.exe and
;                              oryxis-mcp.exe; defaults to the
;                              x86_64 release path so a local
;                              `cargo build --release` followed by
;                              `makensis installer.nsi` still works
;                              without flags.

!include "MUI2.nsh"

!ifndef VERSION
    !define VERSION "0.0.0-dev"
!endif
!ifndef ARCH
    !define ARCH "x86_64"
!endif
!ifndef BINPATH
    !define BINPATH "..\target\release"
!endif

Name "Oryxis"
OutFile "..\oryxis-setup-${ARCH}.exe"
InstallDir "$PROGRAMFILES64\Oryxis"
InstallDirRegKey HKLM "Software\Oryxis" "InstallDir"
RequestExecutionLevel admin

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "Oryxis"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "CompanyName" "Wilson Glasser"
VIAddVersionKey "FileDescription" "Oryxis SSH client installer"
VIAddVersionKey "LegalCopyright" "AGPL-3.0-or-later"

!define MUI_ICON "..\resources\logo.ico"
!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Install"
    SetOutPath $INSTDIR

    File "${BINPATH}\oryxis.exe"
    File "${BINPATH}\oryxis-mcp.exe"
    File "..\resources\logo.ico"
    File "..\README.md"

    ; Create start menu shortcuts
    CreateDirectory "$SMPROGRAMS\Oryxis"
    CreateShortCut "$SMPROGRAMS\Oryxis\Oryxis.lnk" "$INSTDIR\oryxis.exe" "" "$INSTDIR\logo.ico"
    CreateShortCut "$SMPROGRAMS\Oryxis\Uninstall.lnk" "$INSTDIR\uninstall.exe"

    ; Desktop shortcut
    CreateShortCut "$DESKTOP\Oryxis.lnk" "$INSTDIR\oryxis.exe" "" "$INSTDIR\logo.ico"

    ; Write uninstaller
    WriteUninstaller "$INSTDIR\uninstall.exe"

    ; Add INSTDIR to the system PATH so `oryxis` and `oryxis-mcp`
    ; resolve from any shell. EnVar handles dedup (no duplicate
    ; entries on reinstall) and broadcasts WM_SETTINGCHANGE so open
    ; shells pick the new value up. Errors are non-fatal.
    EnVar::SetHKLM
    EnVar::AddValue "Path" "$INSTDIR"
    Pop $0

    ; Registry — uninstall info. winget detects installed packages
    ; via these keys, so `DisplayVersion` MUST match the package
    ; version (passed in via `/DVERSION` from the release workflow)
    ; or upgrades won't be recognised.
    WriteRegStr HKLM "Software\Oryxis" "InstallDir" "$INSTDIR"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "DisplayName" "Oryxis"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "UninstallString" "$INSTDIR\uninstall.exe"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "DisplayIcon" "$INSTDIR\logo.ico"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "Publisher" "Wilson Glasser"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "DisplayVersion" "${VERSION}"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "InstallLocation" "$INSTDIR"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "URLInfoAbout" "https://oryxis.app/"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "HelpLink" "https://github.com/wilsonglasser/oryxis"
    WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "NoModify" 1
    WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "NoRepair" 1

    ; Registry — App Paths (makes Windows Search find the app)
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\oryxis.exe" "" "$INSTDIR\oryxis.exe"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\oryxis.exe" "Path" "$INSTDIR"
SectionEnd

Section "Uninstall"
    EnVar::SetHKLM
    EnVar::DeleteValue "Path" "$INSTDIR"
    Pop $0

    Delete "$INSTDIR\oryxis.exe"
    Delete "$INSTDIR\oryxis-mcp.exe"
    Delete "$INSTDIR\logo.ico"
    Delete "$INSTDIR\README.md"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"

    Delete "$SMPROGRAMS\Oryxis\Oryxis.lnk"
    Delete "$SMPROGRAMS\Oryxis\Uninstall.lnk"
    RMDir "$SMPROGRAMS\Oryxis"
    Delete "$DESKTOP\Oryxis.lnk"

    DeleteRegKey HKLM "Software\Oryxis"
    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis"
    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\oryxis.exe"
SectionEnd
