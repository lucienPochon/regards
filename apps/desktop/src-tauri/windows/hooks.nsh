; Installer hooks (Tauri's NSIS template). Regards' daemon keeps running when
; the window closes (background mode), and Windows does not let the files of
; a running program be replaced or removed: it is stopped first.
; `regards daemon stop` returns once the daemon has let go of its data.

!include LogicLib.nsh

Var RegardsStopExit

!macro REGARDS_STOP_DAEMON
  ${If} ${FileExists} "$INSTDIR\regards.exe"
    nsExec::Exec '"$INSTDIR\regards.exe" daemon stop'
    Pop $RegardsStopExit
    ; The process exits right after it let go.
    Sleep 500
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro REGARDS_STOP_DAEMON
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro REGARDS_STOP_DAEMON
!macroend
