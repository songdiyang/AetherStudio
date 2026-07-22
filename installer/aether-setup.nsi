; ============================================================
; Aether Studio 安装包脚本 (NSIS 3.x, Unicode)
;
; 本地编译:
;   makensis /DVERSION=0.1.0 installer\aether-setup.nsi
; CI 编译（release-main.yml 使用）:
;   makensis /DVERSION=<tag> /DSOURCE_EXE=<exe路径> /DOUTPUT_EXE=<输出路径> installer\aether-setup.nsi
;
; 设计说明:
;   - 每用户安装到 %LOCALAPPDATA%\Programs，无需 UAC，
;     为后续自动更新（直接替换 exe）铺路，更新时不会弹权限框。
;   - 如需改为全机安装：RequestExecutionLevel 改 admin，
;     InstallDir 改 $PROGRAMFILES64，SetShellVarContext 改 all。
; ============================================================

!define APP_NAME      "Aether Studio"
!define APP_EXE       "aether-app.exe"
!define APP_PUBLISHER "Aether Team"
!define APP_ID        "AetherEditor"          ; 注册表/快捷方式标识，勿随意改
!define APP_UNINST    "Uninstall.exe"

; ---- 可通过 makensis /D 覆盖的参数 ----
!ifndef VERSION
  !define VERSION "0.1.0"
!endif
!ifndef VERSION_QUAD
  !define VERSION_QUAD "0.1.0.0"              ; exe 文件属性里的四段版本号
!endif
!ifndef SOURCE_EXE
  !define SOURCE_EXE "..\target\x86_64-pc-windows-msvc\release\aether-app.exe"
!endif
!ifndef OUTPUT_EXE
  !define OUTPUT_EXE "aether-setup.exe"
!endif

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
Name "${APP_NAME}"
OutFile "${OUTPUT_EXE}"
Icon "..\crates\aether-win32\resources\app_icons\aether.ico"
UninstallIcon "..\crates\aether-win32\resources\app_icons\aether.ico"

; exe 文件属性信息
VIProductVersion "${VERSION_QUAD}"
VIAddVersionKey "ProductName"     "${APP_NAME}"
VIAddVersionKey "ProductVersion"  "${VERSION}"
VIAddVersionKey "CompanyName"     "${APP_PUBLISHER}"
VIAddVersionKey "FileDescription" "${APP_NAME} Setup"
VIAddVersionKey "FileVersion"     "${VERSION_QUAD}"
VIAddVersionKey "LegalCopyright"  "MIT License"

!include "MUI2.nsh"

!define MUI_ABORTWARNING
!define MUI_ICON   "..\crates\aether-win32\resources\app_icons\aether.ico"
!define MUI_UNICON "..\crates\aether-win32\resources\app_icons\aether.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Launch ${APP_NAME}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

; ---- 安装/升级前关闭正在运行的实例 ----
!macro CloseRunningApp
  ; 忽略不存在进程时的错误，静默强制结束
  nsExec::ExecToLog 'taskkill /F /IM ${APP_EXE} /T'
  Pop $0
  Sleep 500
!macroend

Section "!${APP_NAME}" SecMain
  SectionIn RO

  !insertmacro CloseRunningApp

  SetOutPath "$INSTDIR"
  File "${SOURCE_EXE}"

  ; 预置运行期图标目录（app 会把图标写到 exe 同级 resources/ 下，
  ; 见 crates/aether-win32/src/window/app_icon.rs）
  SetOutPath "$INSTDIR\resources\app_icons"
  File "..\crates\aether-win32\resources\app_icons\aether.ico"

  ; ============================================================
  ; TODO: 后续需要安装更多内容时，在这里追加，例如：
  ;   SetOutPath "$INSTDIR\plugins"
  ;   File /r "..\plugins\*.*"
  ; 或新增可选组件 Section（见下方桌面快捷方式写法）。
  ; ============================================================

  ; 注册表：版本信息 + Windows「应用和功能」卸载入口
  WriteRegStr HKCU "Software\${APP_ID}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\${APP_ID}" "Version" "${VERSION}"

  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" \
      "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" \
      "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" \
      "Publisher" "${APP_PUBLISHER}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" \
      "DisplayIcon" "$INSTDIR\${APP_EXE}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" \
      "UninstallString" "$INSTDIR\${APP_UNINST}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" \
      "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" \
      "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" \
      "NoRepair" 1

  WriteUninstaller "$INSTDIR\${APP_UNINST}"

  ; 开始菜单快捷方式
  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" \
      "$INSTDIR\${APP_EXE}" "" "$INSTDIR\resources\app_icons\aether.ico"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\Uninstall.lnk" "$INSTDIR\${APP_UNINST}"

  ; 静默安装（应用内自动更新场景）结束后自动重启应用
  IfSilent 0 +2
  Exec "$INSTDIR\${APP_EXE}"
SectionEnd

Section "Desktop Shortcut" SecDesktop
  CreateShortcut "$DESKTOP\${APP_NAME}.lnk" \
      "$INSTDIR\${APP_EXE}" "" "$INSTDIR\resources\app_icons\aether.ico"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain}    "Core application files (required)."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Create a shortcut on the desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  !insertmacro CloseRunningApp

  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\${APP_UNINST}"
  Delete "$INSTDIR\resources\app_icons\aether.ico"
  RMDir  "$INSTDIR\resources\app_icons"
  RMDir  "$INSTDIR\resources"
  RMDir  "$INSTDIR"

  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\Uninstall.lnk"
  RMDir  "$SMPROGRAMS\${APP_NAME}"
  Delete "$DESKTOP\${APP_NAME}.lnk"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}"
  DeleteRegKey HKCU "Software\${APP_ID}"

  ; 注意：不删除用户配置/数据目录，如需清理可在此追加
SectionEnd
