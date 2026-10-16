!macro OPENLESS_IME_ABORT_IF_FAILED EXIT_CODE LABEL
  ${If} ${EXIT_CODE} != 0
    DetailPrint "OpenLess TSF IME ${LABEL} failed with exit code ${EXIT_CODE}"
    Abort
  ${EndIf}
!macroend

!macro OPENLESS_IME_REGISTER_X64
  ${If} ${RunningX64}
    DetailPrint "Registering OpenLess x64 TSF IME"
    ExecWait '"$WINDIR\Sysnative\regsvr32.exe" /s "$INSTDIR\windows-ime\x64\OpenLessIme-${VERSION}.dll"' $0
    ${If} $0 != 0
      ${DisableX64FSRedirection}
      ExecWait '"$WINDIR\System32\regsvr32.exe" /s "$INSTDIR\windows-ime\x64\OpenLessIme-${VERSION}.dll"' $0
      ${EnableX64FSRedirection}
    ${EndIf}
    !insertmacro OPENLESS_IME_ABORT_IF_FAILED $0 "x64 registration"
  ${EndIf}
!macroend

!macro OPENLESS_IME_UNREGISTER_X64
  ${If} ${RunningX64}
    DetailPrint "Unregistering OpenLess x64 TSF IME"
    ExecWait '"$WINDIR\Sysnative\regsvr32.exe" /s /u "$INSTDIR\windows-ime\x64\OpenLessIme-${VERSION}.dll"' $0
    ${If} $0 != 0
      ${DisableX64FSRedirection}
      ExecWait '"$WINDIR\System32\regsvr32.exe" /s /u "$INSTDIR\windows-ime\x64\OpenLessIme-${VERSION}.dll"' $0
      ${EnableX64FSRedirection}
    ${EndIf}
    DetailPrint "OpenLess x64 TSF IME unregister exit code $0"
  ${EndIf}
!macroend

!macro OPENLESS_IME_UNREGISTER_X86
  DetailPrint "Unregistering OpenLess x86 TSF IME"
  ${If} ${RunningX64}
    ExecWait '"$WINDIR\SysWOW64\regsvr32.exe" /s /u "$INSTDIR\windows-ime\x86\OpenLessIme-${VERSION}.dll"' $0
  ${Else}
    ExecWait '"$WINDIR\System32\regsvr32.exe" /s /u "$INSTDIR\windows-ime\x86\OpenLessIme-${VERSION}.dll"' $0
  ${EndIf}
  DetailPrint "OpenLess x86 TSF IME unregister exit code $0"
!macroend

!macro OPENLESS_IME_REGISTER_X86
  DetailPrint "Registering OpenLess x86 TSF IME"
  ${If} ${RunningX64}
    ExecWait '"$WINDIR\SysWOW64\regsvr32.exe" /s "$INSTDIR\windows-ime\x86\OpenLessIme-${VERSION}.dll"' $0
  ${Else}
    ExecWait '"$WINDIR\System32\regsvr32.exe" /s "$INSTDIR\windows-ime\x86\OpenLessIme-${VERSION}.dll"' $0
  ${EndIf}
  ${If} $0 != 0
    StrCpy $1 $0
    !insertmacro OPENLESS_IME_UNREGISTER_X64
    StrCpy $0 $1
  ${EndIf}
  !insertmacro OPENLESS_IME_ABORT_IF_FAILED $0 "x86 registration"
!macroend

!macro OPENLESS_IME_STAGE_AND_REPLACE PLATFORM_DIR ENV_VAR
  ; Install each release under a new name. TSF client processes can keep an old
  ; DLL mapped for their whole lifetime, so replacing a fixed path can leave the
  ; registry pointing at the old protocol until the next reboot.
  SetOutPath "$INSTDIR\windows-ime\${PLATFORM_DIR}"
  File /oname=OpenLessIme-${VERSION}.dll "$%${ENV_VAR}%"
!macroend

!macro NSIS_HOOK_PREINSTALL
  ; Upgrade path: release the COM/TSF registration so new client processes won't
  ; bind to the old DLL while we replace it. Fresh install: regsvr32 /u against
  ; a missing DLL is harmless — the existing UNREGISTER macros log the exit code
  ; and continue without aborting.
  !insertmacro OPENLESS_IME_UNREGISTER_X86
  !insertmacro OPENLESS_IME_UNREGISTER_X64

  !insertmacro OPENLESS_IME_STAGE_AND_REPLACE "x64" "OPENLESS_IME_DLL_X64"
  !insertmacro OPENLESS_IME_STAGE_AND_REPLACE "x86" "OPENLESS_IME_DLL_X86"

  SetOutPath "$INSTDIR"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro OPENLESS_IME_REGISTER_X64
  !insertmacro OPENLESS_IME_REGISTER_X86
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro OPENLESS_IME_UNREGISTER_X86
  !insertmacro OPENLESS_IME_UNREGISTER_X64
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\windows-ime\x64\OpenLessIme-*.dll"
  Delete "$INSTDIR\windows-ime\x64\OpenLessIme.dll"
  Delete "$INSTDIR\windows-ime\x86\OpenLessIme-*.dll"
  Delete "$INSTDIR\windows-ime\x86\OpenLessIme.dll"
  RMDir "$INSTDIR\windows-ime\x64"
  RMDir "$INSTDIR\windows-ime\x86"
  RMDir "$INSTDIR\windows-ime"
!macroend
