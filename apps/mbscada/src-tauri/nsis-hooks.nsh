; Кастомные хуки NSIS-установщика mbSCADA (tauri.conf.json > bundle > windows > nsis > installerHooks).
;
; После обновления поверх старой версии Проводник продолжает показывать ярлыкам
; старую иконку: пути .lnk не меняются, и шелл отдаёт иконку из своего кеша,
; хотя сам exe уже с новой иконкой. SHCNE_ASSOCCHANGED (0x08000000) заставляет
; шелл сбросить кеш иконок и перечитать их из свежеустановленного exe.

!macro NSIS_HOOK_POSTINSTALL
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
!macroend
