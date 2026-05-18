; Per-user installer — installs to %LOCALAPPDATA%\Programs\Oryxis for
; the current user only. Runs without UAC (RequestExecutionLevel user)
; so the auto-updater can apply updates silently. Registry entries go
; under HKCU; the system variant (installer.nsi) writes HKLM.
;
; Build-time defines mirror installer.nsi: /DVERSION, /DARCH, /DBINPATH.

!include "MUI2.nsh"
!include "LogicLib.nsh"

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
OutFile "..\oryxis-user-setup-${ARCH}.exe"
InstallDir "$LOCALAPPDATA\Programs\Oryxis"
InstallDirRegKey HKCU "Software\Oryxis" "InstallDir"
RequestExecutionLevel user

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "Oryxis"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "CompanyName" "Wilson Glasser"
VIAddVersionKey "FileDescription" "Oryxis SSH client installer (per-user)"
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

; Kill any running Oryxis app + MCP instances so Windows file locks
; don't block the upgrade. AI clients (Claude Desktop / Code) keep
; oryxis-mcp.exe alive as a child process across Oryxis app sessions,
; so the auto-updater would otherwise fail to overwrite the MCP
; binary even after the main app exits. taskkill ships with Windows;
; no extra plugin needed. Missing-process exit codes are ignored.
!macro KillRunningOryxis
    nsExec::Exec '"taskkill" /F /T /IM oryxis-mcp.exe'
    Pop $0
    nsExec::Exec '"taskkill" /F /T /IM oryxis.exe'
    Pop $0
    ; Brief pause so the OS releases handles before the first File
    ; write; without it taskkill + immediate overwrite can still race
    ; into a sharing violation.
    Sleep 1000
!macroend

; Warn (don't block) if the system variant is also installed. Side-by-side
; works but creates two Start Menu entries and the user is unlikely to
; want both. Suggest manual uninstall, never silently remove the system
; copy — that would need elevation we explicitly don't have.
Function .onInit
    !insertmacro KillRunningOryxis
    IfFileExists "$PROGRAMFILES64\Oryxis\uninstall.exe" 0 done
        MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION \
            "Oryxis is already installed for all users at $PROGRAMFILES64\Oryxis.$\r$\n$\r$\nInstalling the per-user version side-by-side is supported but not recommended. Uninstall the system version first via Settings > Apps, then run this installer again.$\r$\n$\r$\nClick OK to continue anyway, or Cancel to abort." \
            IDOK done
        Abort
    done:
FunctionEnd

Function un.onInit
    !insertmacro KillRunningOryxis
FunctionEnd

Section "Install"
    SetOutPath $INSTDIR

    ; Upgrading from a pre-v0.7 install: remove the bundled MCP binary
    ; if it's still there. The app now downloads MCP into
    ; `%USERPROFILE%\.oryxis\bin\` on demand. Missing-file is silent.
    Delete "$INSTDIR\oryxis-mcp.exe"

    File "${BINPATH}\oryxis.exe"
    File "..\resources\logo.ico"
    File "..\README.md"

    CreateDirectory "$SMPROGRAMS\Oryxis"
    CreateShortCut "$SMPROGRAMS\Oryxis\Oryxis.lnk" "$INSTDIR\oryxis.exe" "" "$INSTDIR\logo.ico"
    CreateShortCut "$SMPROGRAMS\Oryxis\Uninstall.lnk" "$INSTDIR\uninstall.exe"

    CreateShortCut "$DESKTOP\Oryxis.lnk" "$INSTDIR\oryxis.exe" "" "$INSTDIR\logo.ico"

    ; AppUserModelID on .lnk: see installer.nsi for the same TODO.
    ; The runtime fallback ships the JumpList anyway, just one
    ; cold-start late.

    WriteUninstaller "$INSTDIR\uninstall.exe"

    ; Add INSTDIR to the per-user PATH (HKCU\Environment). No admin
    ; needed; takes effect for new shells, broadcast wakes existing ones.
    EnVar::SetHKCU
    EnVar::AddValue "Path" "$INSTDIR"
    Pop $0

    ; Per-user uninstall registration. The HKCU\...\Uninstall key is
    ; surfaced in Settings > Apps under "Installed for the current
    ; user only", same as VSCode's user installer. DisplayVersion
    ; matters for winget too, even on per-user installs.
    WriteRegStr HKCU "Software\Oryxis" "InstallDir" "$INSTDIR"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "DisplayName" "Oryxis (User)"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "UninstallString" "$INSTDIR\uninstall.exe"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "DisplayIcon" "$INSTDIR\logo.ico"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "Publisher" "Wilson Glasser"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "URLInfoAbout" "https://oryxis.app/"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "HelpLink" "https://github.com/wilsonglasser/oryxis"
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "NoModify" 1
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "NoRepair" 1

    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\App Paths\oryxis.exe" "" "$INSTDIR\oryxis.exe"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\App Paths\oryxis.exe" "Path" "$INSTDIR"
SectionEnd

Section "Uninstall"
    EnVar::SetHKCU
    EnVar::DeleteValue "Path" "$INSTDIR"
    Pop $0

    Delete "$INSTDIR\oryxis.exe"
    ; Sweep the pre-v0.7 bundled MCP binary, if upgrading from an old
    ; install that still has it. New installs never lay this file down.
    Delete "$INSTDIR\oryxis-mcp.exe"
    Delete "$INSTDIR\logo.ico"
    Delete "$INSTDIR\README.md"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"

    Delete "$SMPROGRAMS\Oryxis\Oryxis.lnk"
    Delete "$SMPROGRAMS\Oryxis\Uninstall.lnk"
    RMDir "$SMPROGRAMS\Oryxis"
    Delete "$DESKTOP\Oryxis.lnk"

    DeleteRegKey HKCU "Software\Oryxis"
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis"
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\App Paths\oryxis.exe"
SectionEnd
