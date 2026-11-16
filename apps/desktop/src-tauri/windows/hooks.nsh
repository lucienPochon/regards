; Installer hooks (Tauri's NSIS template). Regards' daemon keeps running when
; the window closes (background mode), and Windows does not let the files of
; a running program be replaced or removed: it is stopped first.
;
; The order matters. These hooks run before the installer's own check for a
; running app, and an app left open starts the daemon again as soon as it is
; gone. So the app is closed first, then the daemon is stopped.
;
; `regards.exe` can still be held after that, by an agent's `regards mcp` for
; one. A silent install would then skip that one file without a word and
; leave a new app beside an old daemon, so nothing is installed until the
; file is free. Those programs are not closed from here: closing a console
; program through Windows' Restart Manager also ends the terminal it runs in.

!include LogicLib.nsh

; What is said when `regards.exe` stays in use. The installer speaks English
; and French (`bundle.windows.nsis.languages`); 1036 is French. Accents are
; written as ${U+xxxx} so the text does not depend on this file's encoding.
!define REGARDS_IN_USE_EN "${PRODUCTNAME} is still in use by another program, so its files cannot be replaced.$\n$\nClose what is connected to it (an AI agent, a terminal running regards), then try again."
!define REGARDS_IN_USE_FR "${PRODUCTNAME} est encore utilis${U+00E9} par un autre programme : ses fichiers ne peuvent pas ${U+00EA}tre remplac${U+00E9}s.$\n$\nFermez ce qui y est connect${U+00E9} (un agent IA, un terminal o${U+00F9} tourne regards), puis r${U+00E9}essayez."

Var RegardsStopExit
Var RegardsTry
Var RegardsFile
Var RegardsFree
Var RegardsInUse


!macro REGARDS_STOP_DAEMON
  ${If} ${FileExists} "$INSTDIR\regards.exe"
    ; Asks before closing the app, unless the install is silent or an update.
    !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
    ${Do}
      ; Returns once the daemon has let go of its data.
      nsExec::Exec '"$INSTDIR\regards.exe" daemon stop'
      Pop $RegardsStopExit
      ; The process exits right after it let go: its file is free within moments.
      StrCpy $RegardsFree 0
      ${For} $RegardsTry 1 10
        ClearErrors
        FileOpen $RegardsFile "$INSTDIR\regards.exe" a
        ${IfNot} ${Errors}
          FileClose $RegardsFile
          StrCpy $RegardsFree 1
          ${ExitFor}
        ${EndIf}
        Sleep 300
      ${Next}
      ${If} $RegardsFree = 1
        ${ExitDo}
      ${EndIf}
      ${If} $LANGUAGE = 1036
        StrCpy $RegardsInUse "${REGARDS_IN_USE_FR}"
      ${Else}
        StrCpy $RegardsInUse "${REGARDS_IN_USE_EN}"
      ${EndIf}
      ; A silent install takes the default, and stops here.
      ${If} ${Cmd} `MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$RegardsInUse" /SD IDCANCEL IDCANCEL`
        Abort "$RegardsInUse"
      ${EndIf}
    ${Loop}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro REGARDS_STOP_DAEMON
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro REGARDS_STOP_DAEMON
  ; Taken off the user's Path when Regards goes, not when it is updated.
  ${If} $UpdateMode <> 1
  ${AndIf} ${FileExists} "$INSTDIR\regards.exe"
    nsExec::Exec '"$INSTDIR\regards.exe" path remove'
    Pop $RegardsStopExit
  ${EndIf}
  ; No longer offered as a mail app in Default apps (mail_app.rs) once it is
  ; gone; an update keeps the person's choice.
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\RegisteredApplications" "Regards"
    DeleteRegKey HKCU "Software\Clients\Mail\Regards"
    DeleteRegKey HKCU "Software\Classes\Regards.Url.mailto"
  ${EndIf}
  ; "Delete the application data" was ticked: the mail, the settings, the logs
  ; and the passwords in the credential store go too, before regards.exe does.
  ; With no daemon running: this is not something a connected program can ask.
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
  ${AndIf} ${FileExists} "$INSTDIR\regards.exe"
    nsExec::Exec '"$INSTDIR\regards.exe" erase --confirm "erase everything"'
    Pop $RegardsStopExit
  ${EndIf}
!macroend

; Up to 0.3.0 the installer had no publisher name, and kept its notes (where
; Regards is installed, the installer's language) under the key below. They
; are now under the publisher's name: the old ones go.
!macro NSIS_HOOK_POSTINSTALL
  DeleteRegKey HKCU "Software\regards\Regards"
  DeleteRegKey /ifempty HKCU "Software\regards"
!macroend
