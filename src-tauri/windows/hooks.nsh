; This file is included before the installer pages are declared.

; The finish page offers a desktop shortcut; the box starts unticked.
; (A silent or passive install has no finish page and creates the shortcut
; unless it is run with /NS.)
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED

; ClickIt keeps scenarios in %AppData%\ClickIt, not in the identifier-based folder
; that the stock uninstaller knows about. Remove it only when the user ticked
; "Delete the application data" and this is a real uninstall, not an update.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RMDir /r "$APPDATA\ClickIt"
  ${EndIf}
!macroend
