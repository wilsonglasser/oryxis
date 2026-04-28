!include "MUI2.nsh"

; Build-time version. Override from CI:
;   makensis /DVERSION=0.5.2 installer.nsi
; Without it, falls back to a placeholder so a developer build still
; runs but doesn't pretend to be a real release.
!ifndef VERSION
    !define VERSION "0.0.0-dev"
!endif

Name "Oryxis"
OutFile "..\oryxis-setup-x86_64.exe"
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

    File "..\target\release\oryxis.exe"
    File "..\target\release\oryxis-mcp.exe"
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
