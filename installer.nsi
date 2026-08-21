; Livetop インストーラー
; makensis /DVERSION=x.y.z installer.nsi でビルドする

!ifndef VERSION
  !define VERSION "0.0.0"
!endif

Name "Livetop"
OutFile "Livetop-${VERSION}-setup.exe"
; レジストリ変更不要なユーザー権限インストールにする
InstallDir "$LOCALAPPDATA\Programs\Livetop"
RequestExecutionLevel user

Page directory
Page instfiles

Section "install"
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "target\release\livetop.exe"
  File "target\release\libmpv-2.dll"
  File "LICENSE.LGPL"
  File "NOTICE.txt"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$SMPROGRAMS\Livetop"
  CreateShortcut "$SMPROGRAMS\Livetop\Livetop.lnk" "$INSTDIR\livetop.exe"
  CreateShortcut "$SMPROGRAMS\Livetop\Uninstall.lnk" "$INSTDIR\uninstall.exe"
SectionEnd

Section "uninstall"
  Delete "$INSTDIR\livetop.exe"
  Delete "$INSTDIR\libmpv-2.dll"
  Delete "$INSTDIR\LICENSE.LGPL"
  Delete "$INSTDIR\NOTICE.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\Livetop\Livetop.lnk"
  Delete "$SMPROGRAMS\Livetop\Uninstall.lnk"
  RMDir "$SMPROGRAMS\Livetop"
SectionEnd
