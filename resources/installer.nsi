!include "MUI2.nsh"

Name "Oryxis"
OutFile "..\oryxis-setup-x86_64.exe"
InstallDir "$PROGRAMFILES64\Oryxis"
InstallDirRegKey HKLM "Software\Oryxis" "InstallDir"
RequestExecutionLevel admin

!define MUI_ICON "..\resources\logo_64.png"
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
    File "..\resources\logo_64.png"
    File "..\README.md"

    ; Create start menu shortcuts
    CreateDirectory "$SMPROGRAMS\Oryxis"
    CreateShortCut "$SMPROGRAMS\Oryxis\Oryxis.lnk" "$INSTDIR\oryxis.exe" "" "$INSTDIR\logo_64.png"
    CreateShortCut "$SMPROGRAMS\Oryxis\Uninstall.lnk" "$INSTDIR\uninstall.exe"

    ; Desktop shortcut
    CreateShortCut "$DESKTOP\Oryxis.lnk" "$INSTDIR\oryxis.exe" "" "$INSTDIR\logo_64.png"

    ; Write uninstaller
    WriteUninstaller "$INSTDIR\uninstall.exe"

    ; Registry
    WriteRegStr HKLM "Software\Oryxis" "InstallDir" "$INSTDIR"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "DisplayName" "Oryxis"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "UninstallString" "$INSTDIR\uninstall.exe"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "DisplayIcon" "$INSTDIR\logo_64.png"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "Publisher" "Wilson Glasser"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis" "DisplayVersion" "0.1.0"
SectionEnd

Section "Uninstall"
    Delete "$INSTDIR\oryxis.exe"
    Delete "$INSTDIR\logo_64.png"
    Delete "$INSTDIR\README.md"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"

    Delete "$SMPROGRAMS\Oryxis\Oryxis.lnk"
    Delete "$SMPROGRAMS\Oryxis\Uninstall.lnk"
    RMDir "$SMPROGRAMS\Oryxis"
    Delete "$DESKTOP\Oryxis.lnk"

    DeleteRegKey HKLM "Software\Oryxis"
    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Oryxis"
SectionEnd
