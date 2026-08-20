; DeepSeek Harness 卸载钩子
; 卸载前：停止应用进程，删除首次启动下载的 runtime（不在 NSIS 安装清单内）

!macro NSIS_HOOK_PREUNINSTALL
  ; 结束应用主进程及其子进程树（含其拉起的 dsh 服务）
  nsExec::Exec 'taskkill /IM "dsh-desktop.exe" /T /F'
  Sleep 500
  ; 删除首次启动下载解压的 runtime 目录（旧版在安装目录，新版在 AppData）
  RMDir /r "$INSTDIR\runtime"
  RMDir /r "$LOCALAPPDATA\com.dsh.desktop\runtime"
  RMDir /r "$APPDATA\com.dsh.desktop\runtime"
!macroend
