; wspr installer: per-user install (no admin rights), Start menu shortcut,
; optional start with Windows, uninstaller listed in Settings > Apps.
; Build: makensis -DVERSION=0.2.0 -DEXE=path\to\wspr.exe installer/wspr.nsi

Unicode true
!ifndef VERSION
  !error "Pass -DVERSION=x.y.z"
!endif
!ifndef EXE
  !error "Pass -DEXE=path/to/wspr.exe"
!endif

!include "MUI2.nsh"
!include "Sections.nsh"

Name "wspr"
OutFile "wspr-setup-${VERSION}.exe"
InstallDir "$LOCALAPPDATA\Programs\wspr"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\wspr"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"

!define MUI_ICON "..\res\wspr.ico"
!define MUI_UNICON "..\res\wspr.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\wspr.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Start wspr now"

!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "wspr (required)" SecMain
  SectionIn RO
  ; An older version may be running; its exe would be locked.
  nsExec::Exec 'taskkill /IM wspr.exe /F'
  Sleep 500
  SetOutPath "$INSTDIR"
  File "/oname=wspr.exe" "${EXE}"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortcut "$SMPROGRAMS\wspr.lnk" "$INSTDIR\wspr.exe"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "wspr"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\wspr.exe"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "TonisOrmisson"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
SectionEnd

Section "Start with Windows" SecStartup
  WriteRegStr HKCU "${RUN_KEY}" "wspr" '"$INSTDIR\wspr.exe"'
SectionEnd

Section "-" SecNoStartup
  ; Unchecking the startup box on reinstall removes an earlier startup entry.
  SectionGetFlags ${SecStartup} $0
  IntOp $0 $0 & ${SF_SELECTED}
  StrCmp $0 ${SF_SELECTED} +2
  DeleteRegValue HKCU "${RUN_KEY}" "wspr"
SectionEnd

Function .onInit
  ; Silent mode is used by the in-app updater: keep the user's startup choice as it is.
  IfSilent 0 done
    ReadRegStr $0 HKCU "${RUN_KEY}" "wspr"
    StrCmp $0 "" 0 done
      !insertmacro UnselectSection ${SecStartup}
  done:
FunctionEnd

Function .onInstSuccess
  ; After a silent update, start the new version (the finish page is skipped).
  IfSilent 0 +2
    Exec '"$INSTDIR\wspr.exe"'
FunctionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /IM wspr.exe /F'
  Sleep 500
  Delete "$INSTDIR\wspr.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\wspr.lnk"
  DeleteRegValue HKCU "${RUN_KEY}" "wspr"
  DeleteRegKey HKCU "${UNINST_KEY}"
  ; Settings and history in %APPDATA%\wspr are kept on purpose.
SectionEnd
