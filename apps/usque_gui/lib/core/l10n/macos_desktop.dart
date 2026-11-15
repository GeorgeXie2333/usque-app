const kMacDesktopEn = <String, String>{
  'macos_keychain_locked': 'Connect to unlock this account in Keychain.',
  'macos_keychain_permission':
      'Access to this account is pending. Click Connect to retry and allow Usque to read the account from your login keychain.',
  'macos_service_title': 'macOS background service',
  'macos_service_admin_hint':
      'Administrator authorization is required to install, repair, or remove the service.',
  'macos_service_missing': 'Service is not installed.',
  'macos_service_incompatible': 'Service is unavailable or incompatible.',
  'macos_service_installed': 'Service is installed.',
  'macos_service_version': 'Version',
  'macos_service_protected': 'Network protection is retained.',
  'macos_service_recovery': 'Network recovery is incomplete.',
  'macos_service_ready': 'System VPN is ready.',
  'macos_service_gated': 'System VPN awaits platform acceptance in this build.',
  'macos_service_preflight': 'Network preflight has not passed.',
  'macos_service_failed': 'The operation did not complete. You can retry.',
  'macos_service_restart':
      'Restart Usque to use the installed service version.',
  'macos_service_disconnect': 'Disconnect before maintaining the service.',
  'macos_service_repair': 'Repair / update',
  'macos_service_install': 'Install service',
  'macos_service_restore': 'Restore network',
  'macos_service_uninstall': 'Uninstall service',
  'macos_install_required':
      'Drag Usque to Applications, open that copy, and retry.',
  'macos_login_approval':
      'Allow Usque in System Settings → General → Login Items, then retry.',
  'macos_login_failed':
      'macOS could not change the login item. Review Login Items in System Settings and retry.',
  'macos_proxy_agent_required':
      'In Settings → macOS background service, install or update the service, then reopen Usque. Daily system proxy connections use this service.',
  'macos_proxy_permission':
      'System proxy permission was not granted. Turn the proxy on again to retry authorization.',
  'macos_proxy_recovery':
      'System proxy recovery is pending. Retry Disconnect and Exit to finish cleanup; canceling permission keeps it available for another retry.',
  'macos_proxy_failed':
      'The system proxy could not be applied. Check the active network service and retry.',
  'macos_warp_connection_required':
      'VPN protection is still retained. Retry Disconnect, then refresh VPN Gate; its temporary WARP connection keeps your saved chain settings.',
  'macos_output_unavailable':
      'Check TUN availability in Settings → macOS background service.',
  'macos_proxy_unavailable':
      'Install or repair the macOS background service in Settings, then reopen Usque to use the system proxy.',
  'macos_kill_switch_unavailable':
      'Kill Switch requires system TUN. Check its status in Settings → macOS background service.',
  'macos_quit_failed':
      'Disconnection cleanup is incomplete. Retry Disconnect and Exit; your window stays open until cleanup succeeds.',
  'macos_agent_quit_failed':
      'Cleanup could not be confirmed. Check Settings → macOS background service. If the service is unavailable, repair it; if recovery is pending, restore the network. Then retry Disconnect and Exit.',
};

const kMacDesktopZhCN = <String, String>{
  'macos_keychain_locked': '点击“连接”即可授权访问钥匙串中的账户。',
  'macos_keychain_permission': '尚未获得账户访问权限。请点击“连接”重试，在登录钥匙串弹窗中允许 Usque 读取此账户。',
  'macos_service_title': 'macOS 后台服务',
  'macos_service_admin_hint': '安装、修复或卸载服务需要管理员授权。',
  'macos_service_missing': '尚未安装服务。',
  'macos_service_incompatible': '服务不可用或版本不兼容。',
  'macos_service_installed': '服务已安装。',
  'macos_service_version': '版本',
  'macos_service_protected': '网络保护仍被保留。',
  'macos_service_recovery': '网络恢复尚未完成。',
  'macos_service_ready': '系统 VPN 已就绪。',
  'macos_service_gated': '此版本的系统 VPN 仍待平台验收。',
  'macos_service_preflight': '网络预检尚未通过。',
  'macos_service_failed': '操作未完成，可再次重试。',
  'macos_service_restart': '请重启 Usque 以使用已安装的服务版本。',
  'macos_service_disconnect': '请先断开连接，再维护后台服务。',
  'macos_service_repair': '修复／更新',
  'macos_service_install': '安装服务',
  'macos_service_restore': '恢复网络',
  'macos_service_uninstall': '卸载服务',
  'macos_install_required': '请将 Usque 拖入“应用程序”，打开该副本后重试。',
  'macos_login_approval': '请在“系统设置 → 通用 → 登录项”中允许 Usque，然后重试。',
  'macos_login_failed': 'macOS 未能修改登录项，请检查系统设置中的登录项后重试。',
  'macos_proxy_agent_required':
      '请在“设置 → macOS 后台服务”安装或更新服务，然后重新打开 Usque。日常连接系统代理由该服务处理。',
  'macos_proxy_permission': '尚未获得系统代理权限。请重新开启代理以重试授权。',
  'macos_proxy_recovery': '系统代理仍待恢复。请重试“断开并退出”以完成清理；取消授权后仍可再次重试。',
  'macos_proxy_failed': '未能应用系统代理，请检查当前网络服务后重试。',
  'macos_warp_connection_required':
      'VPN 保护仍被保留。请重试“断开”后刷新 VPN Gate；临时 WARP 连接会保留已保存的链式代理设置。',
  'macos_output_unavailable': '请在“设置 → macOS 后台服务”查看 TUN 的可用状态。',
  'macos_proxy_unavailable': '请在设置中安装或修复 macOS 后台服务，重新打开 Usque 后即可使用系统代理。',
  'macos_kill_switch_unavailable': '断网保护需要系统 TUN。请在“设置 → macOS 后台服务”查看状态。',
  'macos_quit_failed': '断开后的清理尚未完成。请重试“断开并退出”；清理成功前窗口会保持打开。',
  'macos_agent_quit_failed':
      '未能确认断开后的清理。请查看“设置 → macOS 后台服务”：若服务不可用，请修复服务；若显示网络恢复尚未完成，请点“恢复网络”。然后重试“断开并退出”。',
};

const kMacDesktopZhHant = <String, String>{
  'macos_keychain_locked': '按下「連線」即可授權存取鑰匙圈中的帳戶。',
  'macos_keychain_permission': '尚未取得帳戶存取權限。請按下「連線」重試，在登入鑰匙圈提示中允許 Usque 讀取此帳戶。',
  'macos_service_title': 'macOS 背景服務',
  'macos_service_admin_hint': '安裝、修復或解除安裝服務需要管理員授權。',
  'macos_service_missing': '尚未安裝服務。',
  'macos_service_incompatible': '服務無法使用或版本不相容。',
  'macos_service_installed': '服務已安裝。',
  'macos_service_version': '版本',
  'macos_service_protected': '網路保護仍被保留。',
  'macos_service_recovery': '網路恢復尚未完成。',
  'macos_service_ready': '系統 VPN 已就緒。',
  'macos_service_gated': '此版本的系統 VPN 仍待平台驗收。',
  'macos_service_preflight': '網路預檢尚未通過。',
  'macos_service_failed': '操作未完成，可再次重試。',
  'macos_service_restart': '請重新啟動 Usque 以使用已安裝的服務版本。',
  'macos_service_disconnect': '請先中斷連線，再維護背景服務。',
  'macos_service_repair': '修復／更新',
  'macos_service_install': '安裝服務',
  'macos_service_restore': '恢復網路',
  'macos_service_uninstall': '解除安裝服務',
  'macos_install_required': '請將 Usque 拖入「應用程式」，開啟該副本後重試。',
  'macos_login_approval': '請在「系統設定 → 一般 → 登入項目」中允許 Usque，然後重試。',
  'macos_login_failed': 'macOS 未能修改登入項目，請檢查系統設定中的登入項目後重試。',
  'macos_proxy_agent_required':
      '請在「設定 → macOS 背景服務」安裝或更新服務，然後重新開啟 Usque。日常連線的系統代理由此服務處理。',
  'macos_proxy_permission': '尚未取得系統代理權限。請重新開啟代理以重試授權。',
  'macos_proxy_recovery': '系統代理仍待恢復。請重試「中斷並結束」以完成清理；取消授權後仍可再次重試。',
  'macos_proxy_failed': '未能套用系統代理，請檢查目前網路服務後重試。',
  'macos_warp_connection_required':
      'VPN 保護仍被保留。請重試「中斷」後重新整理 VPN Gate；臨時 WARP 連線會保留已儲存的鏈式代理設定。',
  'macos_output_unavailable': '請在「設定 → macOS 背景服務」查看 TUN 的可用狀態。',
  'macos_proxy_unavailable': '請在設定中安裝或修復 macOS 背景服務，重新開啟 Usque 後即可使用系統代理。',
  'macos_kill_switch_unavailable': '斷網保護需要系統 TUN。請在「設定 → macOS 背景服務」查看狀態。',
  'macos_quit_failed': '中斷後的清理尚未完成。請重試「中斷並結束」；清理成功前視窗會保持開啟。',
  'macos_agent_quit_failed':
      '無法確認中斷後的清理。請查看「設定 → macOS 背景服務」：若服務無法使用，請修復服務；若顯示網路復原尚未完成，請點「還原網路」。然後重試「中斷並結束」。',
};

const kMacDesktopCatalogs = <String, Map<String, String>>{
  'en': kMacDesktopEn,
  'zh_CN': kMacDesktopZhCN,
  'zh_HK': kMacDesktopZhHant,
  'zh_TW': kMacDesktopZhHant,
};
