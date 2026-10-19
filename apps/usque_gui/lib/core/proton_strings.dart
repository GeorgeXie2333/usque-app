import 'app_strings.dart';

const _english = <String, String>{
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
  'completed': 'Ready. Select the saved configuration and apply changes.',
  'cancelled': 'Setup cancelled',
  'failed': 'Proton setup failed. Retry when you are ready.',
  'unavailable': 'Proton VPN is unavailable in this Engine.',
  'bad_credentials':
      'Check your Proton username, password or authenticator code.',
  'rate_limited': 'Proton limited this request. Wait before trying again.',
  'underlay': 'Check your WARP account and MASQUE connection, then retry.',
  'verification_failed':
      'Verification could not be completed. Start setup again.',
  'refresh': 'Refresh authorization or change server',
  'logout': 'Sign out and delete',
  'forget': 'Forget on this device only',
  'forget_confirm':
      'Remove this Proton configuration and its saved session from this device? This does not revoke the remote Proton session.',
  'removed': 'The Proton configuration was removed from this device.',
  'needs_login': 'Sign in again',
  'expires': 'Authorization expires',
  'unsupported_second_factor':
      'This account requires a sign-in method that is not supported here.',
  'reauthentication_required': 'Sign in again to renew this Proton session.',
  'profile_in_use':
      'Disconnect and remove this configuration from the saved chain selection first.',
  'session_busy':
      'Another Proton operation is still running. Wait or cancel it first.',
  'api_rejected':
      'Proton rejected this request. The codes below identify the server response.',
  'expired': 'The verification or input request expired. Start setup again.',
  'underlay_changed': 'The outer WARP connection changed. Start setup again.',
  'remote_unknown': 'Remote session revocation could not be confirmed.',
  'cancelled_remote_unknown':
      'Cancelled on this device. Remote session revocation could not be confirmed.',
  'privacy':
      'Your password is used only to sign in. Sessions and keys are protected on this device.',
  'empty': 'Sign in or continue as guest to add a Proton configuration.',
};

const _chinese = <String, String>{
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
  'completed': '已就绪。请选择保存的配置并应用更改。',
  'cancelled': '已取消设置',
  'failed': 'Proton 设置失败，可稍后重试。',
  'unavailable': '当前引擎不支持 Proton VPN。',
  'bad_credentials': '请检查 Proton 用户名、密码或身份验证器验证码。',
  'rate_limited': 'Proton 限制了此请求，请稍后再试。',
  'underlay': '请检查 WARP 账号和 MASQUE 连接后重试。',
  'verification_failed': '未能完成验证，请重新开始设置。',
  'refresh': '刷新授权或更换服务器',
  'logout': '退出登录并删除',
  'forget': '仅忘记本机',
  'forget_confirm': '要从本机移除此 Proton 配置和保存的会话吗？此操作不会撤销远端 Proton 会话。',
  'removed': '已从本机移除 Proton 配置。',
  'needs_login': '需要重新登录',
  'expires': '授权到期时间',
  'unsupported_second_factor': '此账号需要的登录验证方式暂不受支持。',
  'reauthentication_required': '请重新登录以续期此 Proton 会话。',
  'profile_in_use': '请先断开连接，并从已保存的链式出口选择中解除此配置。',
  'session_busy': '另一个 Proton 操作仍在进行，请等待或先取消该操作。',
  'api_rejected': 'Proton 拒绝了此请求，下方代码标明服务端响应。',
  'expired': '验证或输入请求已过期，请重新开始设置。',
  'underlay_changed': '外层 WARP 连接已改变，请重新开始设置。',
  'remote_unknown': '远端会话撤销结果未知。',
  'cancelled_remote_unknown': '本机已取消，远端会话撤销结果未知。',
  'privacy': '密码仅用于登录，会话和密钥由本机安全存储保护。',
  'empty': '请登录或以游客身份继续，以添加 Proton 配置。',
};

extension ProtonStrings on AppStrings {
  String proton(String key) =>
      (languageCode == 'zh' ? _chinese[key] : null) ??
      _english[key] ??
      _english['failed']!;
}
