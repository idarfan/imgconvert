; ImgConvert 安裝程式（NSIS 3，可在 WSL2/Linux 用 makensis 編譯）
; 用法：makensis -DVERSION=1.0.0 -DEXE=../dist/ImgConvert.exe installer.nsi

Unicode true
!include "MUI2.nsh"

!ifndef VERSION
  !define VERSION "1.0.0"
!endif
!ifndef EXE
  !define EXE "../dist/ImgConvert.exe"
!endif

!define APPNAME "ImgConvert"
!define APPNAME_ZH "ImgConvert 圖片轉檔器"
; 捷徑檔名用 ASCII，避免少數環境（如 Wine / 非 Unicode 地區設定）建立失敗
!define LNK "ImgConvert.lnk"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}"

Name "${APPNAME_ZH}"
OutFile "../dist/ImgConvert-Setup-${VERSION}.exe"
; 安裝到使用者目錄，不需系統管理員權限，方便轉發給別人
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\Programs\${APPNAME}"
InstallDirRegKey HKCU "Software\${APPNAME}" "InstallDir"
SetCompressor /SOLID lzma
ManifestDPIAware true

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1028 "ProductName" "${APPNAME}"
VIAddVersionKey /LANG=1028 "FileDescription" "${APPNAME_ZH} 安裝程式"
VIAddVersionKey /LANG=1028 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1028 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=1028 "LegalCopyright" "MIT License"

!define MUI_ICON "../assets/icon.ico"
!define MUI_UNICON "../assets/icon.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\ImgConvert.exe"
!define MUI_FINISHPAGE_RUN_TEXT "立即啟動 ${APPNAME_ZH}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "TradChinese"

Section "主程式（必要）" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "/oname=ImgConvert.exe" "${EXE}"
  File "/oname=icon.ico" "../assets/icon.ico"
  File "/oname=THIRD-PARTY-LICENSES.txt" "../assets/icons/LICENSE-fluentui-emoji.txt"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\${LNK}" "$INSTDIR\ImgConvert.exe" "" "$INSTDIR\icon.ico"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\Uninstall.lnk" "$INSTDIR\Uninstall.exe"

  WriteRegStr HKCU "Software\${APPNAME}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APPNAME_ZH}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${APPNAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\icon.ico"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
SectionEnd

Section "桌面捷徑" SecDesktop
  CreateShortcut "$DESKTOP\${LNK}" "$INSTDIR\ImgConvert.exe" "" "$INSTDIR\icon.ico"
SectionEnd

Section "右鍵「傳送到」選單" SecSendTo
  ; 在檔案總管對資料夾／圖片按右鍵 → 傳送到 → ImgConvert，即可直接載入
  CreateShortcut "$SENDTO\${LNK}" "$INSTDIR\ImgConvert.exe" "" "$INSTDIR\icon.ico"
SectionEnd

LangString DESC_Main ${LANG_TRADCHINESE} "ImgConvert 主程式。"
LangString DESC_Desktop ${LANG_TRADCHINESE} "在桌面建立捷徑。"
LangString DESC_SendTo ${LANG_TRADCHINESE} "在右鍵選單「傳送到」加入 ImgConvert。"
!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} $(DESC_Main)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} $(DESC_Desktop)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecSendTo} $(DESC_SendTo)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  Delete "$INSTDIR\ImgConvert.exe"
  Delete "$INSTDIR\icon.ico"
  Delete "$INSTDIR\THIRD-PARTY-LICENSES.txt"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APPNAME}\${LNK}"
  Delete "$SMPROGRAMS\${APPNAME}\Uninstall.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  Delete "$DESKTOP\${LNK}"
  Delete "$SENDTO\${LNK}"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\${APPNAME}"
SectionEnd
