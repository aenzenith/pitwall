; Pitwall's hooks into Tauri's NSIS installer (`bundle.windows.nsis.installerHooks` in
; tauri.conf.json).
;
; On Windows, Claude Code and Codex run pitwall.exe itself as Pitwall's hook, so an uninstall
; takes those hooks out first (and the team radio's entry), while the app is still there to do it:
; `pitwall.exe --uninstall-cleanup` (src-tauri/src/core/uninstall.rs). It prints nothing, always
; ends well and gives up after a minute.
;
; Only for an uninstall the user asked for. Two runs of the uninstaller keep the app where it is,
; and so the hooks:
; - an update (the updater, or an installer run with /UPDATE) runs the old uninstaller with
;   /UPDATE ($UpdateMode is 1);
; - an installer that removes the version already there before installing ("Uninstall before
;   installing") runs it in place with `_?=`, where an uninstall from Windows' Apps runs it from
;   a copy in the temporary folder: $EXEDIR is then not $INSTDIR.
; The template asks to close a running Pitwall only after this hook, and a Cancel there keeps the
; app installed: so that one is asked first here, and a Cancel leaves the hooks in place too. The
; template's own question then finds nothing running.
; Known gap: "Uninstall" picked in an installer of the version already there runs in place too,
; and keeps the hooks.

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
  ${AndIf} $EXEDIR != $INSTDIR
    !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --uninstall-cleanup'
    ClearErrors
  ${EndIf}
!macroend
