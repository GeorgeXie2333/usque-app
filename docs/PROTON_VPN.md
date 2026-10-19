# Proton VPN chain exit / Proton VPN 链式出口

Open **Proxy → Chain proxy → Proton VPN**. This source prepares and saves a
Proton session for a WireGuard exit after the outer WARP connection. The order
is `Application → WARP → Proton VPN → Internet`. Explicit direct rules and
app exclusions keep their existing meaning. Preparing an account does not
select it, apply settings or establish a tunnel.

打开 **代理 → 链式代理 → Proton VPN**，在外层 WARP 之后准备并保存 Proton
WireGuard 出口。准备完成不会自动选用、应用或连接；显式直连和分应用例外继续生效。

This native implementation remains under validation. A bounded, synthetic
workstation HTTPS probe completed guest creation, VPN permissions, certificate
issuance, token refresh and owned-session revocation. It did not run through the
native WARP stack or a human puzzle. Native Android account/guest setup and the
complete WireGuard/local-agent tunnel have not been validated on this candidate.
See the [native validation record](PROTON_VPN_NATIVE_VALIDATION.md).

本原生实现仍在验证中。一次有限的开发机 HTTPS 实验完成了合成游客注册、VPN 权限、
证书签发、令牌刷新及本机所建会话撤销；它没有使用原生 WARP 栈或真人拼图。
本次候选的 Android 认证与完整 WireGuard/local-agent 隧道仍未实测，不能以离线检查代替。

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
   **Verification required** appears. Finish the puzzle yourself; the window
   closes and setup resumes when its result is accepted. Use **Cancel** or
   Android Back to abandon verification. Android reports window failures as
   errors, separately from cancellation, and shows a verification error code
   when available. The app does not solve,
   drag or automate the puzzle. A challenge is usable once, for the current
   job/session/outer-connection generation, for at most five minutes.
5. Setup saves the account, its local key and the complete eligible logical
   server list. It does not issue a connection certificate yet. Repeating sign-in
   to the same Proton user updates the existing account. Legacy duplicates appear
   as one account, rather than several copies of the same server.
6. Under the account, use **Change server** to search by server name, country
   code or city. **Automatic** chooses an available server when connecting.
   **Refresh list** updates the catalogue. Each logical server appears once;
   its multiple physical entrances do not create duplicate rows. The list includes
   available, compatible WireGuard UDP servers within the account's authorized
   tier, without the former 200-candidate truncation.
7. Enable chain proxy, select the account and use **Apply changes** or **Apply
   and reconnect**. At connection time Usque checks the session, retrieves a fresh
   directory and resolves the selected logical server's current entrance, peer key
   and agent hostname. It obtains a session certificate only when missing or due,
   reusing a valid certificate bound to the saved local key. An unavailable manual
   choice reports an error; choose another server or Automatic. WireGuard must
   still complete local-agent authorization before business traffic is admitted.

先选择可用的 WARP 账号及支持 UDP 出口的 MASQUE 模式；L4 不支持此来源。
选择“以游客身份继续”或“登录 Proton”，按提示输入密码、六位身份验证器验证码，
由本人完成官方窗口的拼图。Android 自动打开验证窗口，桌面点击“完成人机验证”。
取消或 Android 返回键会终止任务；窗口失败单独显示错误。挑战绑定本次任务、会话
和外层连接代次，最多五分钟、使用一次。12087 表示验证结果被拒，应用清除该结果，
最多重新获取一次挑战，新的拼图仍需本人完成；再次失败会显示错误。

准备成功只保存账号、本地密钥和完整可用逻辑节点列表，不提前申请连接证书。
相同 Proton 用户再次登录会更新已有账号；旧版本重复记录合并显示。
在账号下点击“更换节点”，可按名称、国家代码或城市搜索，或选择“自动”；
“刷新列表”更新目录。账号权限内、兼容且在线的 WireGuard UDP 逻辑节点完整保留，
不再截断为 200 个候选，也不把同一节点的多个入口显示成多行。
启用链式代理、选择账号并应用后，连接时重新解析节点入口、公钥和 agent hostname，
检查会话并按需申请证书，有效证书继续复用同一密钥。手动节点不可用会明确报错。
目录可见、账号已保存或 WireGuard 已握手，都不等于最终授权完成。

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
Android ignores taps outside the dialog so a puzzle drag cannot cancel setup.
It accepts the bound CAPTCHA iframe result directly as well as the official
top-level success message; either path can finish the challenge only once.
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
profile，也不保证进程死亡后已清理。Android 不因触碰弹窗外部而取消设置；
同时接收绑定本次挑战的 CAPTCHA iframe 结果和官方顶层成功消息，只有首次有效
结果可以完成验证。Linux 界面预览不能启动该流程。

## Refresh, cancel and remove / 续期、取消与移除

Use **Refresh list** to update available servers. Disconnect before changing a
server on the account used by the running exit. The server preference takes effect
on the next connection; selecting a server does not independently reconnect.
Valid access tokens are reused. Only a protected request returning HTTP 401
triggers one journaled token recovery per operation. A known replacement is saved
before later requests, so a directory or certificate error does not leave a
successful rotation marked **Sign in again**. An unknown refresh result requires
explicit sign-in and never replays the old refresh token. While connected, the
runtime renews the same key's certificate when due. Authorization failure does not
fall back to a WARP-only exit.

Use **Remove account** and choose **Sign out and remove** to revoke sessions
owned by this app, then remove the local account and its legacy duplicate
profiles. **Forget locally** removes only the local records without contacting
Proton or promising remote revocation. It needs no WARP connection. The dialog
clears a saved chain selection before removal; if that save fails, removal stops.
A running account must first be disconnected. Failed remote cleanup remains
visible and retains the local handles for an explicit retry or local removal.
Canceling unfinished setup also attempts to clean up its known sessions. These
actions do not sign out other devices' Proton sessions.

“刷新列表”更新节点目录。正在使用的账号需先断开，再更换节点；偏好在下次连接
时生效，选节点本身不会单独触发重连。有效 access token 不强制轮换；一次操作中，
只有受保护接口返回 401 时才尝试一次带持久化日志的恢复。已知的新令牌先保存，
后续目录或证书失败不会把成功刷新误标为“重新登录”；刷新结果未知才要求明确
重新认证，绝不重放旧 refresh token。运行时按期限续签同一密钥的证书，授权失败
不会降级为仅 WARP 出口。

点击“移除账号”，选择“退出并移除”会先尝试撤销本应用拥有的会话，再删除本机
记录及旧版本重复配置。“仅本机遗忘”只删除本机记录，无需 WARP，**不撤销远端
会话**。移除前会清除已保存的链式选择；保存失败即停止删除。正在连接的账号必须
先断开。撤销失败会保留本机记录并明确提示，供重试或选择仅本机遗忘。
这些操作不表示已退出用户其他设备的全部 Proton 会话。

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

This source currently supports eligible WireGuard UDP logical servers. Guest
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
