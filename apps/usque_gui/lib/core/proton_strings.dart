import 'app_strings.dart';

const _english = <String, String>{
  'connection_api':
      'A Proton API request failed. Retry or review the Proton account in Chain proxy.',
  "connection_authentication":
      "Proton authorization failed. Open Chain proxy and sign in to Proton again.",
  "connection_certificate":
      "Proton rejected the connection certificate and recovery failed. Reconnect or sign in to Proton again.",
  "connection_restricted":
      "The Proton server is temporarily restricted. Try again or choose another server.",
  "connection_sessions":
      "Proton’s simultaneous connection limit was reached. Disconnect another Proton session and retry.",
  "connection_key":
      "Proton detected this connection key in multiple sessions. Disconnect the other session and retry.",
  "connection_policy":
      "Proton restricted this connection under its account or usage policy. Check the Proton account or choose another server.",
  "connection_server":
      "The Proton server could not authorize the connection. Try another server.",
  "connection_protocol":
      "The Proton connection response could not be validated. Retry or choose another server.",
  "connection_transport":
      "Could not reach Proton. Check your network or choose another server.",
  "connection_reconnect": "Proton authorization was renewed. Reconnecting…",
  "connection_review":
      "Open Chain proxy to review the Proton account and selected server.",
  "connection_retry": "Retry the Proton connection or choose another server.",

  'title': 'Proton VPN',
  'guest': 'Continue as guest',
  'login': 'Sign in to Proton',
  'username': 'Username',
  'password': 'Password',
  'totp': 'Authenticator code',
  'submit': 'Continue',
  'verify': 'Complete verification',
  'verification_hint':
      'Complete the puzzle in the official verification window.',
  'choose_node': 'Choose a free server',
  'working': 'Preparing Proton VPN…',
  'waiting_totp': 'Enter your authenticator code to continue.',
  'waiting_verification': 'Verification required',
  'waiting_node': 'Choose a server to finish setup.',
  'completed': 'Account saved. Select it and apply changes to use Proton VPN.',
  'cancelled': 'Setup cancelled',
  'failed': 'Proton setup failed. Retry when you are ready.',
  'result_unconfirmed':
      'The Proton setup result is not confirmed. Refresh status to continue or cancel the existing setup before starting again.',
  'unavailable': 'Proton VPN is unavailable in this Engine.',
  'bad_credentials':
      'Check your Proton username, password or authenticator code.',
  'rate_limited': 'Proton limited this request. Wait before trying again.',
  'network': 'Could not reach Proton. Check your network or VPN, then retry.',
  'verification_failed':
      'Verification could not be completed. Start setup again.',
  'verification_invalid':
      'Proton rejected the verification result. Start setup again for a new challenge.',
  'accounts': 'Proton accounts',
  'resolve_on_connect':
      'The server and authorization are checked when connecting.',
  'account': 'Proton account',
  'signed_in': 'Signed in',
  'session_requires_login': 'Session needs sign-in',
  'selected_server': 'Server',
  'automatic_server': 'Automatic',
  'automatic_hint': 'Choose an available server when connecting.',
  'available_servers': 'Available servers',
  'choose_server': 'Choose a server',
  'search_servers': 'Search by name, country or city',
  'load_servers': 'Load server list',
  'change_server': 'Change server',
  'refresh_servers': 'Refresh list',
  'remove_account': 'Remove account',
  'remove_account_confirm':
      'Remove this account and its saved server preference? Sign out also revokes this app’s saved sessions. Forget locally leaves remote sessions unchanged. A saved chain selection will be cleared.',
  'forget_local': 'Forget locally',
  'sign_out_remove': 'Sign out and remove',
  'current_account': 'Connected account',
  'saved_account': 'Saved selection',
  'next_connection': 'Server preference saved for the next connection.',
  'disconnect_to_change':
      'Disconnect this account to change its server or remove it.',
  'cleanup_pending':
      'An earlier session could not be confirmed revoked. Sign out again or explicitly forget the account locally.',
  'selected_server_unavailable':
      'The selected server is unavailable. Choose another server or Automatic.',
  'close': 'Close',
  'verification_error': 'Verification error',
  'refresh': 'Refresh authorization or change server',
  'logout': 'Sign out and delete',
  'forget': 'Forget on this device only',
  'forget_confirm':
      'Remove this Proton configuration and its saved session from this device? This does not revoke the remote Proton session.',
  'removed': 'The Proton account was removed from this device.',
  'needs_login': 'Sign in again',
  'expires': 'Authorization expires',
  'unsupported_second_factor':
      'This account requires a sign-in method that is not supported here.',
  'reauthentication_required': 'Sign in again to renew this Proton session.',
  'profile_in_use': 'Disconnect this account before changing or removing it.',
  'session_busy':
      'Another Proton operation is still running. Wait or cancel it first.',
  'api_rejected':
      'Proton rejected this request. The codes below identify the server response.',
  'session_not_allowed':
      'Proton does not allow this client to create a session.',
  'credentialless_disabled': 'Proton guest access is currently unavailable.',
  'no_free_nodes':
      'Proton returned no available WireGuard servers for this account.',
  'invalid_directory': 'Proton returned an unsupported server list.',
  'invalid_client_configuration': 'Proton returned no usable WireGuard ports.',
  'stage': 'Failed step',
  'stage_anonymous_session': 'Create an anonymous session',
  'stage_credentialless': 'Create a guest account',
  'stage_session_scopes': 'Check guest authorization',
  'stage_protected_user': 'Check the account',
  'stage_vpn_permissions': 'Check VPN access',
  'stage_session_refresh': 'Renew the session',
  'stage_srp_info': 'Start sign-in',
  'stage_srp_proof': 'Verify sign-in',
  'stage_totp': 'Verify the authenticator code',
  'stage_directory': 'Load servers',
  'stage_certificate': 'Request VPN authorization',
  'stage_secure_storage': 'Save the configuration',
  'stage_session_revoke': 'Revoke the session',
  'expired': 'The verification or input request expired. Start setup again.',
  'underlay_changed': 'The network context changed. Start setup again.',
  'remote_unknown': 'Remote session revocation could not be confirmed.',
  'cancelled_remote_unknown':
      'Cancelled on this device. Remote session revocation could not be confirmed.',
  'privacy':
      'Your password is used only to sign in. Sessions and keys are protected on this device.',
  'empty': 'Sign in or continue as guest to add a Proton account.',
};

const _chinese = <String, String>{
  'connection_api': 'Proton API 请求失败，请重试或在“链式代理”中检查 Proton 账号。',
  "connection_authentication": "Proton 授权失败，请打开“链式代理”重新登录 Proton。",
  "connection_certificate": "Proton 拒绝了连接证书且自动恢复失败，请重新连接或重新登录 Proton。",
  "connection_restricted": "Proton 节点暂时受限，请稍后重试或更换节点。",
  "connection_sessions": "已达到 Proton 同时连接数量上限，请断开其他 Proton 会话后重试。",
  "connection_key": "Proton 检测到同一连接密钥被多个会话使用，请断开另一会话后重试。",
  "connection_policy": "Proton 根据账号或使用政策限制了此次连接，请检查 Proton 账号或更换节点。",
  "connection_server": "Proton 节点未能完成连接授权，请尝试其他节点。",
  "connection_protocol": "无法验证 Proton 的连接响应，请重试或更换节点。",
  "connection_transport": "无法连接 Proton，请检查网络或更换节点。",
  "connection_reconnect": "Proton 授权已更新，正在重新连接…",
  "connection_review": "请打开“链式代理”，检查 Proton 账号和所选节点。",
  "connection_retry": "请重试 Proton 连接或更换节点。",

  'title': 'Proton VPN',
  'guest': '以游客身份继续',
  'login': '登录 Proton',
  'username': '用户名',
  'password': '密码',
  'totp': '身份验证器验证码',
  'submit': '继续',
  'verify': '完成人机验证',
  'verification_hint': '请在官方验证窗口中完成人工拼图。',
  'choose_node': '选择免费服务器',
  'working': '正在准备 Proton VPN…',
  'waiting_totp': '请输入身份验证器验证码以继续。',
  'waiting_verification': '需要人机验证',
  'waiting_node': '请选择服务器以完成设置。',
  'completed': '账号已保存。请选择该账号并应用更改，以使用 Proton VPN。',
  'cancelled': '已取消设置',
  'failed': 'Proton 设置失败，可稍后重试。',
  'result_unconfirmed': 'Proton 设置结果尚未确认。请刷新状态，继续或取消已有任务后再重新开始。',
  'unavailable': '当前引擎不支持 Proton VPN。',
  'bad_credentials': '请检查 Proton 用户名、密码或身份验证器验证码。',
  'rate_limited': 'Proton 限制了此请求，请稍后再试。',
  'network': '无法连接 Proton，请检查当前网络或 VPN 后重试。',
  'verification_failed': '未能完成验证，请重新开始设置。',
  'verification_invalid': 'Proton 拒绝了验证结果，请重新开始设置以获取新的挑战。',
  'accounts': 'Proton 账号',
  'resolve_on_connect': '连接时获取节点入口并检查授权。',
  'account': 'Proton 账号',
  'signed_in': '已登录',
  'session_requires_login': '会话需要重新登录',
  'selected_server': '节点',
  'automatic_server': '自动选择',
  'automatic_hint': '连接时选择可用节点。',
  'available_servers': '可用节点',
  'choose_server': '选择节点',
  'search_servers': '搜索名称、国家或城市',
  'load_servers': '加载节点列表',
  'change_server': '切换节点',
  'refresh_servers': '刷新列表',
  'remove_account': '移除账号',
  'remove_account_confirm':
      '要移除此账号及其节点选择吗？退出登录也会撤销本应用保存的会话；仅忘记本机不会改变远端会话。已保存的链式出口选择会一并清除。',
  'forget_local': '仅忘记本机',
  'sign_out_remove': '退出并移除',
  'current_account': '当前连接账号',
  'saved_account': '已保存选择',
  'next_connection': '节点选择已保存，将在下次连接时生效。',
  'disconnect_to_change': '请先断开此账号的连接，再切换节点或移除账号。',
  'cleanup_pending': '先前会话的撤销结果未知，请重试退出登录，或明确选择仅忘记本机账号。',
  'selected_server_unavailable': '所选节点不可用，请选择其他节点或自动选择。',
  'close': '关闭',
  'verification_error': '验证错误',
  'refresh': '刷新授权或更换服务器',
  'logout': '退出登录并删除',
  'forget': '仅忘记本机',
  'forget_confirm': '要从本机移除此 Proton 配置和保存的会话吗？此操作不会撤销远端 Proton 会话。',
  'removed': '已从本机移除 Proton 配置。',
  'needs_login': '需要重新登录',
  'expires': '授权到期时间',
  'unsupported_second_factor': '此账号需要的登录验证方式暂不受支持。',
  'reauthentication_required': '请重新登录以续期此 Proton 会话。',
  'profile_in_use': '请先断开此账号的连接，再修改或移除账号。',
  'session_busy': '另一个 Proton 操作仍在进行，请等待或先取消该操作。',
  'api_rejected': 'Proton 拒绝了此请求，下方代码标明服务端响应。',
  'session_not_allowed': 'Proton 不允许此客户端创建会话。',
  'credentialless_disabled': 'Proton 当前不提供游客访问。',
  'no_free_nodes': 'Proton 未返回此账号可用的 WireGuard 服务器。',
  'invalid_directory': 'Proton 返回的服务器列表格式不受支持。',
  'invalid_client_configuration': 'Proton 未返回可用的 WireGuard 端口。',
  'stage': '失败步骤',
  'stage_anonymous_session': '创建匿名会话',
  'stage_credentialless': '创建游客账号',
  'stage_session_scopes': '检查游客授权',
  'stage_protected_user': '检查账号',
  'stage_vpn_permissions': '检查 VPN 权限',
  'stage_session_refresh': '续期会话',
  'stage_srp_info': '开始登录',
  'stage_srp_proof': '验证登录',
  'stage_totp': '验证身份验证器验证码',
  'stage_directory': '加载服务器',
  'stage_certificate': '申请 VPN 授权',
  'stage_secure_storage': '保存配置',
  'stage_session_revoke': '撤销会话',
  'expired': '验证或输入请求已过期，请重新开始设置。',
  'underlay_changed': '网络环境已改变，请重新开始设置。',
  'remote_unknown': '远端会话撤销结果未知。',
  'cancelled_remote_unknown': '本机已取消，远端会话撤销结果未知。',
  'privacy': '密码仅用于登录，会话和密钥由本机安全存储保护。',
  'empty': '请登录或以游客身份继续，以添加 Proton 账号。',
};

extension ProtonStrings on AppStrings {
  String proton(String key) =>
      (languageCode == 'zh' ? _chinese[key] : null) ??
      _english[key] ??
      _english['failed']!;
}

String? protonConnectionFailure(AppStrings strings, String? code) {
  final key = switch (code) {
    'PROTON_AUTHENTICATION_FAILED' => 'connection_authentication',
    'PROTON_CERTIFICATE_FAILED' => 'connection_certificate',
    'PROTON_SERVER_RESTRICTED' => 'connection_restricted',
    'PROTON_SESSION_LIMIT' => 'connection_sessions',
    'PROTON_KEY_REUSED' => 'connection_key',
    'PROTON_POLICY_RESTRICTED' => 'connection_policy',
    'PROTON_SERVER_FAILED' => 'connection_server',
    'PROTON_RATE_LIMITED' => 'rate_limited',
    'PROTON_API_FAILED' => 'connection_api',
    'PROTON_PROTOCOL_FAILED' => 'connection_protocol',
    'PROTON_TRANSPORT_FAILED' => 'connection_transport',
    'PROTON_RECONNECT_REQUIRED' => 'connection_reconnect',
    _ => null,
  };
  return key == null ? null : strings.proton(key);
}

String? protonGateFailure(AppStrings strings, String? reason) =>
    protonConnectionFailure(strings, switch (reason) {
      'proton_authentication' => 'PROTON_AUTHENTICATION_FAILED',
      'proton_certificate' => 'PROTON_CERTIFICATE_FAILED',
      'proton_restricted' => 'PROTON_SERVER_RESTRICTED',
      'proton_max_sessions' => 'PROTON_SESSION_LIMIT',
      'proton_key_reused' => 'PROTON_KEY_REUSED',
      'proton_policy' => 'PROTON_POLICY_RESTRICTED',
      'proton_server' => 'PROTON_SERVER_FAILED',
      'proton_rate_limited' => 'PROTON_RATE_LIMITED',
      'proton_api' => 'PROTON_API_FAILED',
      'proton_protocol' => 'PROTON_PROTOCOL_FAILED',
      'proton_transport' => 'PROTON_TRANSPORT_FAILED',
      'proton_reconnect' => 'PROTON_RECONNECT_REQUIRED',
      _ => null,
    });
