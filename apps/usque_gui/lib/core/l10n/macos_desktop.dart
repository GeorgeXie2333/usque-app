const kMacDesktopEn = <String, String>{
  'macos_install_required':
      'Drag Usque to Applications, open that copy, and retry.',
  'macos_login_approval':
      'Allow Usque in System Settings → General → Login Items, then retry.',
  'macos_login_failed':
      'macOS could not change the login item. Review Login Items in System Settings and retry.',
  'macos_proxy_permission':
      'System proxy permission was not granted. Turn the proxy on again to retry authorization.',
  'macos_proxy_recovery':
      'System proxy recovery is pending. Retry Disconnect and Exit to finish cleanup; canceling permission keeps it available for another retry.',
  'macos_proxy_failed':
      'The system proxy could not be applied. Check the active network service and retry.',
  'macos_warp_connection_required':
      'VPN protection is still retained. Retry Disconnect, then refresh VPN Gate; its temporary WARP connection keeps your saved chain settings.',
  'macos_output_unavailable':
      'System VPN requires a signed Network Extension build; this development installation supports proxy mode.',
  'macos_proxy_unavailable':
      'System proxy is unavailable in this installation.',
  'macos_kill_switch_unavailable':
      'Saved for VPN use. This installation does not provide system VPN protection; proxy mode is not protected by Kill Switch.',
  'macos_quit_failed':
      'Disconnection cleanup is incomplete. Retry Disconnect and Exit; your window stays open until cleanup succeeds.',
};

const kMacDesktopZhCN = <String, String>{
  'macos_install_required': '请将 Usque 拖入“应用程序”，打开该副本后重试。',
  'macos_login_approval': '请在“系统设置 → 通用 → 登录项”中允许 Usque，然后重试。',
  'macos_login_failed': 'macOS 未能修改登录项，请检查系统设置中的登录项后重试。',
  'macos_proxy_permission': '尚未获得系统代理权限。请重新开启代理以重试授权。',
  'macos_proxy_recovery': '系统代理仍待恢复。请重试“断开并退出”以完成清理；取消授权后仍可再次重试。',
  'macos_proxy_failed': '未能应用系统代理，请检查当前网络服务后重试。',
  'macos_warp_connection_required':
      'VPN 保护仍被保留。请重试“断开”后刷新 VPN Gate；临时 WARP 连接会保留已保存的链式代理设置。',
  'macos_output_unavailable': '系统 VPN 需要签署的网络扩展版本；此开发版本支持代理模式。',
  'macos_proxy_unavailable': '此安装版本尚未提供系统代理。',
  'macos_kill_switch_unavailable': '此设置为 VPN 保留。此安装版本尚未提供系统 VPN 保护；代理模式不受断网保护。',
  'macos_quit_failed': '断开后的清理尚未完成。请重试“断开并退出”；清理成功前窗口会保持打开。',
};

const kMacDesktopZhHant = <String, String>{
  'macos_install_required': '請將 Usque 拖入「應用程式」，開啟該副本後重試。',
  'macos_login_approval': '請在「系統設定 → 一般 → 登入項目」中允許 Usque，然後重試。',
  'macos_login_failed': 'macOS 未能修改登入項目，請檢查系統設定中的登入項目後重試。',
  'macos_proxy_permission': '尚未取得系統代理權限。請重新開啟代理以重試授權。',
  'macos_proxy_recovery': '系統代理仍待恢復。請重試「中斷並結束」以完成清理；取消授權後仍可再次重試。',
  'macos_proxy_failed': '未能套用系統代理，請檢查目前網路服務後重試。',
  'macos_warp_connection_required':
      'VPN 保護仍被保留。請重試「中斷」後重新整理 VPN Gate；臨時 WARP 連線會保留已儲存的鏈式代理設定。',
  'macos_output_unavailable': '系統 VPN 需要經簽署的網路延伸功能版本；此開發版本支援代理模式。',
  'macos_proxy_unavailable': '此安裝版本尚未提供系統代理。',
  'macos_kill_switch_unavailable': '此設定供 VPN 使用。此安裝版本尚未提供系統 VPN 保護；代理模式不受斷網保護。',
  'macos_quit_failed': '中斷後的清理尚未完成。請重試「中斷並結束」；清理成功前視窗會保持開啟。',
};

const kMacDesktopCatalogs = <String, Map<String, String>>{
  'en': kMacDesktopEn,
  'zh_CN': kMacDesktopZhCN,
  'zh_HK': kMacDesktopZhHant,
  'zh_TW': kMacDesktopZhHant,
};
