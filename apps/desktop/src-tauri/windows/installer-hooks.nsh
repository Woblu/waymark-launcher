; The app was called Waymark until 0.3.7, when it became Waymark Launcher. Tauri's
; installer names its folder, its entry in Installed apps and its shortcuts after the
; product, so on its own an update would install a second copy next to the old one.
; It would also leave the old Start menu shortcut opening the old version, which
; would then update itself again on every start.
;
; These hooks take over an install made under the old name instead: the new version
; goes into the same folder, and the entry and shortcuts move to the new name. Data
; isn't touched. It lives under %APPDATA%\Waymark, which keeps its name.
;
; This file is included before Tauri defines MANUFACTURER, PRODUCTNAME and
; MAINBINARYNAME, so those are only used inside the macros, which expand later.

!ifndef WAYMARK_OLD_NAME
  !define WAYMARK_OLD_NAME "Waymark"
!endif

Var WaymarkAdopted

!macro NSIS_HOOK_PREINSTALL
  StrCpy $WaymarkAdopted 0
  ; Only an install this app's publisher made under the old name, that's still on disk,
  ; and only when nobody picked a different folder for this one.
  ReadRegStr $R0 SHCTX "Software\${MANUFACTURER}\${WAYMARK_OLD_NAME}" ""
  ReadRegStr $R1 SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${WAYMARK_OLD_NAME}" "Publisher"
  ${If} $R0 != ""
  ${AndIf} $R1 == "${MANUFACTURER}"
  ${AndIf} ${FileExists} "$R0\${MAINBINARYNAME}.exe"
  ${AndIf} $INSTDIR == "$LOCALAPPDATA\${PRODUCTNAME}"
    StrCpy $WaymarkAdopted 1
    StrCpy $INSTDIR $R0
    SetOutPath $INSTDIR
    ; The installer made the new folder just before this. It's still empty, and RMDir
    ; without /r only removes an empty folder.
    RMDir "$LOCALAPPDATA\${PRODUCTNAME}"
  ${EndIf}
!macroend

; An old shortcut opens the same program, so it only needs the new name, which is
; also the one the uninstaller looks for. If this install already made one under the
; new name, the old one goes.
!macro WaymarkRenameShortcut FOLDER
  !insertmacro IsShortcutTarget "${FOLDER}\${WAYMARK_OLD_NAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  Pop $0
  ${If} $0 = 1
    ${If} ${FileExists} "${FOLDER}\${PRODUCTNAME}.lnk"
      Delete "${FOLDER}\${WAYMARK_OLD_NAME}.lnk"
    ${Else}
      Rename "${FOLDER}\${WAYMARK_OLD_NAME}.lnk" "${FOLDER}\${PRODUCTNAME}.lnk"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${If} $WaymarkAdopted = 1
    !insertmacro WaymarkRenameShortcut "$SMPROGRAMS"
    !insertmacro WaymarkRenameShortcut "$DESKTOP"
    ; The new name's entries were written above, so the old ones would only show the
    ; app twice in Installed apps.
    DeleteRegKey SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${WAYMARK_OLD_NAME}"
    DeleteRegKey SHCTX "Software\${MANUFACTURER}\${WAYMARK_OLD_NAME}"
  ${EndIf}
!macroend
