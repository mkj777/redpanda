; Removes what redpanda registers at start to be offered as a mail app (src/default_mail.rs).
!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey HKCU "Software\Classes\redpanda.mailto"
  DeleteRegKey HKCU "Software\Clients\Mail\redpanda"
  DeleteRegValue HKCU "Software\RegisteredApplications" "redpanda"
!macroend
