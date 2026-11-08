; Signet installer (NSIS). Built by scripts/package-windows.ps1:
;   makensis /DVERSION=<x.y.z> /DSOURCE=<bundle folder> /DOUTFILE=<installer path> installer.nsi
; Behaves like the electron-builder one of the Electron app: per-user install (no administrator rights) into
; %LOCALAPPDATA%\Programs\Signet, a directory page, Start Menu and desktop shortcuts, an uninstaller registered in
; "Apps & features" under the SAME key as the Electron installer (so installing this over an Electron install replaces it),
; silent mode (/S, /D=dir) and the electron-updater switches (--updated, --force-run).
Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!ifndef VERSION
  !error "VERSION is required"
!endif
!ifndef SOURCE
  !error "SOURCE is required"
!endif
!ifndef OUTFILE
  !define OUTFILE "Signet-${VERSION}-x64-setup.exe"
!endif

!define APP_NAME "Signet"
!define APP_EXE "signet.exe"
; electron-builder derives this GUID from the appId (dev.signet.app): same key = in-place upgrade of an Electron install.
!define APP_GUID "{ea93c96e-a71c-5caa-a6d0-f986425b44b7}"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_GUID}"
!define CAPS_KEY "Software\Clients\StartMenuInternet\Signet"

Name "${APP_NAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
InstallDirRegKey HKCU "Software\${APP_GUID}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails nevershow
ShowUninstDetails nevershow
BrandingText "${APP_NAME} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "FileDescription" "${APP_NAME} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "MIT"

!define MUI_ICON "${SOURCE}\signet.ico"
!define MUI_UNICON "${SOURCE}\signet.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Launch ${APP_NAME}"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "French"

Var ForceRun

Function .onInit
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "--force-run" $R1
  ${IfNot} ${Errors}
    StrCpy $ForceRun 1
  ${EndIf}
FunctionEnd

Section "Install"
  SetOutPath "$INSTDIR"

  ; An Electron (or older) install: run its uninstaller first, silently, keeping the user data (--updated).
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "UninstallString"
  ${If} $0 != ""
    ReadRegStr $1 HKCU "${UNINSTALL_KEY}" "InstallLocation"
    ${If} $1 != ""
    ${AndIf} ${FileExists} "$1\Uninstall ${APP_NAME}.exe"
      ExecWait '"$1\Uninstall ${APP_NAME}.exe" /S --updated _?=$1'
      Delete "$1\Uninstall ${APP_NAME}.exe"
    ${EndIf}
  ${EndIf}

  File /r "${SOURCE}\*.*"
  WriteUninstaller "$INSTDIR\Uninstall ${APP_NAME}.exe"

  CreateShortCut "$SMPROGRAMS\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
  CreateShortCut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0

  ; Apps & features entry (per user).
  WriteRegStr HKCU "Software\${APP_GUID}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME} ${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "Christophe Vandevoir"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall ${APP_NAME}.exe" /currentuser'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall ${APP_NAME}.exe" /currentuser /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" $0

  ; Default browser: the program ids, the capabilities (Settings > Default apps lists Signet) and the signet:// scheme.
  WriteRegStr HKCU "Software\Classes\SignetHTML" "" "Signet HTML Document"
  WriteRegStr HKCU "Software\Classes\SignetHTML\DefaultIcon" "" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKCU "Software\Classes\SignetHTML\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'
  WriteRegStr HKCU "Software\Classes\SignetURL" "" "Signet URL"
  WriteRegStr HKCU "Software\Classes\SignetURL" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\SignetURL\DefaultIcon" "" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKCU "Software\Classes\SignetURL\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'
  WriteRegStr HKCU "Software\Classes\signet" "" "URL:Signet"
  WriteRegStr HKCU "Software\Classes\signet" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\signet\DefaultIcon" "" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKCU "Software\Classes\signet\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'

  WriteRegStr HKCU "${CAPS_KEY}" "" "${APP_NAME}"
  WriteRegStr HKCU "${CAPS_KEY}\DefaultIcon" "" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKCU "${CAPS_KEY}\shell\open\command" "" '"$INSTDIR\${APP_EXE}"'
  WriteRegStr HKCU "${CAPS_KEY}\Capabilities" "ApplicationName" "${APP_NAME}"
  WriteRegStr HKCU "${CAPS_KEY}\Capabilities" "ApplicationDescription" "A sidebar-first web browser"
  WriteRegStr HKCU "${CAPS_KEY}\Capabilities" "ApplicationIcon" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKCU "${CAPS_KEY}\Capabilities\FileAssociations" ".htm" "SignetHTML"
  WriteRegStr HKCU "${CAPS_KEY}\Capabilities\FileAssociations" ".html" "SignetHTML"
  WriteRegStr HKCU "${CAPS_KEY}\Capabilities\FileAssociations" ".xht" "SignetHTML"
  WriteRegStr HKCU "${CAPS_KEY}\Capabilities\FileAssociations" ".xhtml" "SignetHTML"
  WriteRegStr HKCU "${CAPS_KEY}\Capabilities\URLAssociations" "http" "SignetURL"
  WriteRegStr HKCU "${CAPS_KEY}\Capabilities\URLAssociations" "https" "SignetURL"
  WriteRegStr HKCU "${CAPS_KEY}\Capabilities\URLAssociations" "signet" "SignetURL"
  WriteRegStr HKCU "Software\RegisteredApplications" "${APP_NAME}" "${CAPS_KEY}\Capabilities"

  ; Started by the updater (electron-updater passes --force-run): relaunch the app when the silent install is done.
  ${If} $ForceRun == 1
    Exec '"$INSTDIR\${APP_EXE}"'
  ${EndIf}
SectionEnd

Section "Uninstall"
  ; Safety: only remove a folder that is really an install of Signet.
  ${IfNot} ${FileExists} "$INSTDIR\${APP_EXE}"
    Goto done
  ${EndIf}
  Delete "$SMPROGRAMS\${APP_NAME}.lnk"
  Delete "$DESKTOP\${APP_NAME}.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "Software\${APP_GUID}"
  DeleteRegKey HKCU "Software\Classes\SignetHTML"
  DeleteRegKey HKCU "Software\Classes\SignetURL"
  DeleteRegKey HKCU "Software\Classes\signet"
  DeleteRegKey HKCU "${CAPS_KEY}"
  DeleteRegValue HKCU "Software\RegisteredApplications" "${APP_NAME}"
  ; The profile (<AppData>\Signet GPUI) is the user's data and is kept.
  RMDir /r "$INSTDIR"
  done:
SectionEnd
