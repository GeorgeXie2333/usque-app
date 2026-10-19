# Proton VPN chain exit / Proton VPN 链式出口

Open **Proxy → Chain proxy → Proton VPN**. This source prepares and saves a
Proton session for a WireGuard exit after the outer WARP connection. The order
is `Application → WARP → Proton VPN → Internet`. Explicit direct rules and
app exclusions keep their existing meaning. Preparing an account does not
select it, apply settings or establish a tunnel.

打开 **代理 → 链式代理 → Proton VPN**，在外层 WARP 之后准备并保存 Proton
WireGuard 出口。准备完成不会自动选用、应用或连接；显式直连和分应用例外继续生效。

This is a native implementation under validation. No usable account was supplied
for live testing. The earlier standalone guest experiment received HTTP 422 /
API 8004 before authentication; that result belongs to the historical Python
candidate, not this native implementation. Native guest/account authentication,
a real human puzzle, token renewal and the complete tunnel are **not_run**.
See the [native validation record](PROTON_VPN_NATIVE_VALIDATION.md) and
[historical PoC results](PROTON_VPN_POC_VALIDATION.md).

本原生实现仍在验证中。本轮没有可用于实测的账号；此前独立 Python 游客实验在
认证前收到 HTTP 422 / API 8004，不能把该历史结果当成本次原生实现的实测。
原生游客／账号认证、真人拼图、实网续期和完整隧道均为 **not_run**。

## Prepare an account / 准备账号

1. Select a configured WARP account and a MASQUE mode that supports the
   WireGuard UDP exit. L4 TCP proxy mode cannot provide this exit. An Engine
   without Proton support disables the source and explains why.
2. Choose **Continue as guest** or **Sign in to Proton**. Guest setup and account
   sign-in are separate flows; a guest rejection does not establish that account
   sign-in also fails. For sign-in, enter your Proton username and password in
   the application dialog. Passwords are used for authentication and are not
   saved. This implementation accepts Proton SRP v4 and passwords of 1–72 UTF-8
   bytes; other versions or longer passwords stop with an error.
3. If requested, enter the six-digit **Authenticator code**. The implemented
   second factor is TOTP. Other required sign-in methods are reported as
   unsupported rather than silently skipped.
4. Android opens the official verification window automatically when Proton
   requires it. On desktop, select **Complete verification** when
   **Verification required** appears. Finish the puzzle yourself. Closing that
   window or canceling setup cancels the waiting job. The app does not solve,
   drag or automate the puzzle. A challenge is usable once, for the current
   job/session/outer-connection generation, for at most five minutes.
5. Choose a server from **Choose a free server**. The list contains eligible
   free-tier directory candidates; visibility and load are remote observations,
   not evidence that a server is reachable. Setup checks the returned key,
   certificate and authorization before saving a configuration.
6. After **Ready. Select the saved configuration and apply changes.**, enable
   chain proxy, select that saved configuration, and use **Apply changes** or
   **Apply and reconnect**. The current connection section describes the
   running exit. A saved configuration or a WireGuard handshake alone does not
   establish final Proton authorization: the session-mode local agent also has
   to accept the connection.

先选择可用的 WARP 账号和支持 UDP 出口的 MASQUE 模式；L4 不支持该来源。
在 Proton 区域选择“以游客身份继续”或“登录 Proton”，按提示输入账号密码、
六位身份验证器验证码。Android 收到人机验证要求时自动打开官方窗口；桌面端点击
“完成人机验证”，由本人完成拼图。关闭窗口或取消设置会终止等待任务。挑战只绑定本次任务、会话和
外层连接代次，最多五分钟、使用一次。选择免费服务器，准备成功后还需在配置库
选用并应用。目录可见、配置已保存或 WireGuard 已握手，都不等于最终授权完成。

## Verification window and network path / 验证窗口与网络路径

The authentication, directory and certificate API requests use the explicitly
owned WARP underlay. A change to that underlay invalidates a waiting job;
control-plane failure does not switch those requests to the host network.

**The verification WebView uses the platform's normal system network path.**
It does not use the Rust WARP control-plane handle. Its egress may therefore
be different, depending on the system's routing and VPN state. Matching its
source IP to the WARP API requests has not been established, and Proton's
acceptance of this split path has not been tested. Do not assume the verification
page is protected by the selected chain exit.

认证、目录和证书 API 使用明确持有的 WARP underlay；外层连接变化会使等待任务
失效，失败不会把这些 API 请求改走宿主直连。**验证码 WebView 采用平台普通系统
网络路径，不使用 Rust 的 WARP 控制面句柄**。其出口受系统路由及 VPN 状态影响，
尚未证明与 API 的 WARP 出口一致，也未实测 Proton 是否接受这种分离路径。
不能假定验证码页面经过所选链式出口。

The window opens only `https://verify.proton.me` as the official top-level
challenge. Windows uses a fresh InPrivate profile and Linux an ephemeral
context. Android uses the app's default WebView profile and clears cookies and
WebStorage on normal open/close; this is not an isolated ephemeral profile or a
guarantee of cleanup after process death. The success bridge checks origin,
top frame, the current challenge digest, expiry and one-use state.
Downloads, new windows, permissions and TLS bypasses are denied. Windows needs
an already installed compatible WebView2 runtime; Usque does not install one.
Linux loads a compatible system WebKitGTK 4.1 or 4.0 library when available;
otherwise verification is unavailable. Android uses the platform WebView.
Windows currently offers CAPTCHA only: it binds the official embedded captcha
iframe's result, since the observed page's WebView2 branch does not emit a
top-level success message. A server offering no CAPTCHA is reported unavailable.
A missing runtime is an error, not a successful verification. The Linux debug
preview cannot open this native verification flow.

原生窗口限制官方来源、顶层页面、当前挑战摘要、期限和一次性回传，禁止下载、
新窗口、权限授权和绕过 TLS。Windows 使用独立 InPrivate 环境及已安装的兼容
WebView2 runtime，不自动安装浏览器；目前仅支持官方嵌入拼图 iframe 的 CAPTCHA
结果，服务端未提供 CAPTCHA 时显示不可用。Linux 使用临时 WebKit context，
按需加载系统 WebKitGTK 4.1／4.0，缺少兼容库即显示不可用。Android 使用应用默认
WebView profile，在正常打开／关闭时清理 cookies 和 WebStorage，并非独立临时
profile，也不保证进程死亡后已清理。Linux 界面预览不能启动该流程。

## Refresh, cancel and remove / 续期、取消与移除

Use **Refresh authorization or change server** on a saved Proton account to
prepare renewed authorization or another eligible server. Disconnect before
changing an account used by the running exit. While connected, the runtime
uses the saved account lease for renewal before certificate expiry. Refused,
expired, ambiguous or unverifiable authorization does not fall back to a
WARP-only exit. An interrupted token rotation marks the account **Sign in
again** rather than replaying its previous refresh token.

Use the saved configuration's remove action to sign out and delete it. A
running or saved chain selection must be disconnected and deselected first.
Removal attempts to revoke only the owned Proton session; a failed or unknown
remote revocation is reported as such. Canceling unfinished setup also attempts
to clean up its known sessions. It is not equivalent to signing out all Proton
sessions on other devices.

已保存账号的“刷新授权或更换服务器”可重新准备授权或服务器；正在使用的出口需先
断开。运行时通过该账号的独占 lease 在证书到期前续期。授权被拒、过期、结果未知或
无法校验时，不降级成仅 WARP 出口；轮转中断的账号要求重新登录，不重放旧刷新令牌。
移除配置会尝试撤销本机持有的会话并删除记录；需先断开并解除已保存的出口选择。
撤销失败或远端状态未知会保留错误，不宣称已退出其他设备的全部 Proton 会话。

If the saved token is no longer usable, or a previous rotation/revocation has an
unknown result, use **Forget on this device only** and confirm the local-only
removal. This deletes the managed profile and encrypted Proton record without
contacting Proton; it needs no WARP connection and **does not revoke the remote
session**. The same active/saved-selection protection applies: disconnect and
deselect the configuration first. Use online **Sign out and delete** when you
want to attempt remote revocation. A local forget must not be reported as a
successful remote sign-out.

保存的令牌失效，或轮转／撤销结果无法确认时，可选择**仅忘记本机**并明确确认。
此操作无需 WARP，只删除本机托管配置和加密 Proton 记录，**不撤销远端会话**。
仍须先断开并解除已保存的出口选择；希望尝试远端撤销时，使用联网的“退出登录并
删除”。仅本机遗忘不能记为远端退出成功。

## Storage and current limits / 存储与当前限制

Session tokens, the Ed25519 seed and certificates are stored in the platform's
encrypted private sidecar, separate from ordinary settings. Passwords, TOTP,
verification URLs/tokens, raw HTTP bodies, private keys and complete certificates
are excluded from normal status and diagnostics. Do not attach these values to
bug reports. Errors expose bounded stage/status/code information instead.

The implementation checks the pinned modulus signature, safe-group parameters
and SRP server proof; the leaf certificate must match the locally generated key
and API/X.509 times. A bounded chain of 1–8 distinct certificates is parsed, but
its issuer chain is not independently authenticated here: provenance relies on
the checked HTTPS API response. Token rotation is journaled before sending its
single request. Unix directory synchronization and Windows write-through
replacement are implemented; real power-loss recovery has not been tested.

This source currently selects free-tier WireGuard UDP candidates. Guest
eligibility, third-party client acceptance, device verification, other second
factors, the real local-agent protocol and platform cleanup still need live
validation. API 8004 means the client is not allowed to create a session; the
failed step and numeric codes are shown without opening a CAPTCHA window.
Only an actual human-verification response starts the puzzle flow. Android
uses a pinned Android APK compatibility profile, matching API/User-Agent
versions and actual device facts, as described in the protocol reference.

会话、Ed25519 seed 和证书保存在平台加密私有侧车中，与普通设置分离；密码不保存，
验证码、挑战 URL/token、HTTP 原文及私钥不进入普通状态和诊断。证书校验绑定本地
公钥和 API/X.509 时间；1–8 张不同 DER 的链只作严格解析，issuer 来源依赖经过
TLS 校验的 API，不另行认证。轮转前写持久化标记；Unix 目录同步和 Windows
write-through 替换已实现，但真实断电恢复未实测。免费游客资格、第三方身份接受性、
其他二次验证、真实 local-agent 和平台清理仍需实网验证。8004 表示不允许客户端
创建会话，界面显示失败步骤和数字代码，不会当作拼图。Android 按固定 APK 兼容
版本统一 API 标识和 User-Agent，设备字段取真实系统信息，细节见协议参考。

See the [chain guide](CHAIN_PROXY.md), [pinned protocol reference](PROTON_VPN_PROTOCOL.md)
and [native validation record](PROTON_VPN_NATIVE_VALIDATION.md). The standalone
[Python PoC](../tool/proton_poc/README.md) remains a separate historical tool.
The native SRP implementation retains the
[Proton go-srp MIT notice](../apps/usque_gui/assets/licenses/proton-srp.txt) in
source and in the application's license registry.
