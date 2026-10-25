# Proton VPN chain exit / Proton VPN 链式出口

Open **Proxy → Chain proxy → Proton VPN**. This source prepares and saves a
Proton session for a WireGuard exit after the outer WARP connection. The order
is `Application → WARP → Proton VPN → Internet`. Explicit direct rules and
app exclusions keep their existing meaning. Preparing an account does not
select the Proton account, apply settings or activate the system VPN.

打开 **代理 → 链式代理 → Proton VPN**，准备并保存 Proton 账号，用作 WARP
之后的 WireGuard 出口。准备完成不会自动选用 Proton 账号、应用设置或启动系统 VPN；显式直连和分应用例外继续生效。

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

1. Open the Proton source in an Engine with Proton support. Account setup does
   not require a WARP identity or an active WARP connection.
2. Choose **Continue as guest** or **Sign in to Proton** under **Proton
   accounts**. Once an account is saved, both are under **Add account**. Guest
   setup and account sign-in are separate flows; a guest rejection does not establish that account
   sign-in also fails. For sign-in, enter your Proton username and password in
   the application dialog. Passwords are used for authentication and are not
   saved. This implementation accepts Proton SRP v4 and passwords of 1–72 UTF-8
   bytes; other versions or longer passwords stop with an error.
3. If requested, enter the six-digit **Authenticator code**. The implemented
   second factor is TOTP. Other required sign-in methods are reported as
   unsupported rather than silently skipped.
4. Android opens the official verification page automatically in a fixed
   full-screen window with an **X** at the top right when Proton requires it.
   On desktop, select **Complete verification** when
   **Verification required** appears. Finish the puzzle yourself; the window
   closes and setup resumes when its result is accepted. Use the **X** or
   Android Back to abandon verification on Android, or **Cancel** on desktop.
   Android reports window failures as errors, separately from cancellation,
   and shows a verification error code when available. The app does not solve,
   drag or automate the puzzle. A challenge is usable once, for the current
   job/session/challenge, for at most five minutes.
5. Setup saves the account, its local key and the complete eligible logical
   server list. It does not issue a connection certificate yet. Repeating sign-in
   to the same Proton user updates the existing account. Legacy duplicates appear
   as one account, rather than several copies of the same server.
6. Select the **Server** row under the account to search by server name, country
   code or city. **Automatic** chooses an available server when connecting.
   **Refresh list** in the account's menu updates the catalogue. Each logical
   server appears once; its multiple physical entrances do not create duplicate
   rows. The list includes available, compatible WireGuard UDP servers within
   the account's authorized tier, without the former 200-candidate truncation.
7. To connect, select a configured WARP account and a MASQUE mode that supports
   the WireGuard UDP exit; L4 TCP proxy mode cannot provide this exit. Select the
   Proton account, which also turns on chain proxy in the draft, and use **Apply
   changes** or **Apply and reconnect**. At connection time Usque checks the
   session, retrieves a fresh directory and resolves the selected logical
   server's current entrance, peer key and agent hostname. It obtains a session
   certificate only when missing or due, reusing a valid certificate bound to the
   saved local key. An unavailable manual choice reports an error; choose another
   server or Automatic. WireGuard must still complete local-agent authorization
   before business traffic is admitted.

准备 Proton 账号不要求 WARP 身份或已连接的 WARP。
在“Proton 账号”下选择“以游客身份继续”或“登录 Proton”（已有账号后两者位于
“添加账号”中），按提示输入密码、六位身份验证器验证码，
由本人完成官方窗口的拼图。Android 自动打开固定全屏验证窗口，右上角 **X** 或
返回键会终止任务；桌面点击“完成人机验证”，可用“取消”退出。窗口失败单独显示错误。
挑战绑定本次任务、会话和当前挑战，最多五分钟、使用一次。12087 表示验证结果被拒，应用清除该结果，
最多重新获取一次挑战，新的拼图仍需本人完成；再次失败会显示错误。

On Windows and Linux, **Continue as guest** automatically uses a fixed synthetic
Android compatibility context. No additional setting or device details are
required. It sends an Android 15 / Pixel 8 profile with English, US region and
UTC, without reading the host's hardware identifiers. Ordinary account sign-in
keeps its independent client identity; Android uses its actual platform facts.
The guest's API identity remains with its encrypted session for later requests,
including after restarting Usque. Existing accounts retain their saved identity.
Proton can still reject a session or require manual verification; API 8004 alone
does not identify which field or eligibility condition caused the rejection.

Windows 和 Linux 的“以游客身份继续”自动使用固定的合成 Android 兼容配置，
无需开启额外选项或填写设备信息。模板为 Android 15 / Pixel 8、英语、美国区域
和 UTC，不读取本机硬件标识。普通账号登录保持独立客户端身份，Android 使用
真实系统信息。游客 API 身份随加密会话保存，重启后的请求继续沿用；现有账号
保留原身份。Proton 仍可能拒绝会话或要求本人完成验证，不能仅凭 8004 确定
具体被拒绝的字段或资格条件。

准备成功只保存账号、本地密钥和完整可用逻辑节点列表，不提前申请连接证书。
相同 Proton 用户再次登录会更新已有账号；旧版本重复记录合并显示。
点击账号下的“节点”行，可按名称、国家代码或城市搜索，或选择“自动”；
账号菜单中的“刷新列表”更新目录。账号权限内、兼容且在线的 WireGuard UDP 逻辑节点完整保留，
不再截断为 200 个候选，也不把同一节点的多个入口显示成多行。
实际连接仍需配置 WARP 账号及支持 UDP 出口的 MASQUE 模式，L4 不支持此出口。
选择账号（同时在草稿中启用链式代理）并应用后，连接时重新解析节点入口、公钥和 agent hostname，
检查会话并按需申请证书，有效证书继续复用同一密钥。手动节点不可用会明确报错。
目录可见、账号已保存或 WireGuard 已握手，都不等于最终授权完成。

## Verification window and network path / 验证窗口与网络路径

Guest registration, account sign-in, manual directory refresh and sign-out first
use system HTTPS and DNS. They follow the current OS routes, an existing VPN,
and the HTTP client's supported environment proxy settings. A network failure
can switch the entire task once to WARP, including subsequent API requests,
DNS, verification and session cleanup. Business rejections such as a wrong
password, 8004, 10200 or rate limiting do not trigger this switch.

Fallback borrows a healthy WARP underlay or starts a temporary headless
connection using the currently selected, already enrolled WARP account.
It never creates a new WARP identity. Temporary connections keep OS routing,
including another VPN, and respect retained network guards. They do not install
a TUN, change system proxy/DNS/routes or replace the current connection.
If no usable account/path exists, the task reports an error. Once switched,
the task cannot return to system sockets; an external WARP generation change
invalidates its lease.

Requests that might already have changed a session are not blindly replayed.
Automatic replay is limited to reads, SRP information, or a transport failure
proven to have occurred before submission. The existing rotation journal and
remote-state-unknown reporting remain authoritative. If verification fails
before a rejected operation is submitted, the old window closes, WARP is
acquired, and one fresh challenge is requested. Complete the replacement puzzle
manually; a desktop window already opened by the user reopens automatically.

On WARP, the native browser uses an authenticated, short-lived loopback CONNECT
proxy scoped to this task. HTTP plus CONNECT preflight, with a completion
proof derived from the random proxy password, must succeed before the
challenge URL is loaded. Only `verify.proton.me`, `verify-api.proton.me` and
`account.proton.me` on port 443 are allowed. DNS and TCP use the same WARP
handle; HTTPS retains original origins and normal certificate checks. Android
requires WebView proxy override support and serializes process-wide overrides;
Windows uses a fresh WebView2 environment with scoped proxy arguments; Linux
requires WebKitGTK proxy/authentication APIs. Unsupported or failed routing
stops verification instead of silently using system traffic.

Connection preparation and ongoing Proton certificate/session renewal keep
their separate WARP-only client. Ordinary application traffic still waits for
final Proton authorization; this account-control channel is not an application
exit. OS route changes can still change the source address of the initial
system path. Live external egress consistency requires the isolated network
observer; local tests alone do not establish it.

游客注册、登录、手动刷新目录和退出首先使用系统 HTTPS/DNS，遵循当前 VPN 与支持的
环境代理。网络失败后，整个任务可单向切换一次到 WARP，涵盖后续 API、DNS、
验证码和会话清理。密码错误、8004、10200、限流等业务拒绝不会触发切换。
优先复用健康的 WARP，否则用当前已选、已有身份的 WARP 账户启动临时无界面连接；
不创建账户，不修改系统 VPN、路由、DNS 或代理，也不绕过其他 VPN 或保留的网络保护。
WARP 不可用会报错；切换后不会重新直连。

可能已提交的会话变更不会自动重放。验证码网络失败时，只有原操作已明确被拒绝，
才会关闭旧窗口、获取 WARP 并重新请求一次挑战；新拼图仍由本人完成。
WARP 验证窗口必须先通过本地代理的 HTTP/CONNECT 预检，所有允许的页面请求及
域名解析再经由同一 WARP。运行时不支持代理能力或预检失败时明确停止验证。
本地测试不能代替隔离环境对实际出口、DNS 和断线泄漏的观测。

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

Use **Refresh list** in the account's menu to update available servers.
Disconnect before changing a server on the account used by the running exit;
its server row and menu actions stay disabled until then. The account row shows
the running session's state, and a failed connection is never shown as
connected. The server preference takes effect
on the next connection; selecting a server does not independently reconnect.

If the reply starting guest setup or sign-in is lost, Usque checks the current
task once without repeating the login request. Until its status is confirmed,
new setup and account changes are disabled. If the status check also fails,
select **Refresh status** to find the existing task, then continue its verification
or cancel it. Once the status confirms that no task is running, setup can start
again.

Valid access tokens are reused. Only a protected request returning HTTP 401
triggers one journaled token recovery per operation. A known replacement is saved
before later requests, so a directory or certificate error does not leave a
successful rotation marked **Sign in again**. An unknown refresh result requires
explicit sign-in and never replays the old refresh token. While connected, the
runtime renews the same key's certificate when due. Authorization failure does not
fall back to a WARP-only exit.

Connection errors name Proton separately from WARP. A temporarily restricted
server stays in connecting with business traffic blocked; if it does not recover
within the connection budget, choose another server or retry. Expired client
certificates are renewed automatically. Revoked certificates or invalid
signatures trigger one new tunnel key and a complete inner WireGuard restart.
Repeated key recovery within five minutes stops with the original Proton error.
Session limits, reused keys and account policy restrictions have distinct
messages. Numeric API/HTTP or agent codes may appear below the message.

A temporary scheduled-refresh failure retains a still-valid certificate and
retries after 30 seconds, backing off to at most five minutes. Certificate expiry,
unknown token rotation and loss of agent authorization still block traffic.

Choose **Remove account** from the account's menu, then **Sign out and remove**
to revoke sessions
owned by this app, then remove the local account and its legacy duplicate
profiles. **Forget locally** removes only the local records without contacting
Proton or promising remote revocation. It needs no WARP connection. The dialog
clears a saved chain selection before removal; if that save fails, removal stops.
A running account must first be disconnected. Failed remote cleanup remains
visible and retains the local handles for an explicit retry or local removal.
Canceling unfinished setup also attempts to clean up its known sessions. These
actions do not sign out other devices' Proton sessions.

账号菜单中的“刷新列表”更新节点目录。正在使用的账号需先断开，再更换节点，
在此之前节点行和菜单操作不可用；账号行显示运行中会话的状态，连接失败不会显示
为已连接。偏好在下次连接
时生效，选节点本身不会单独触发重连。

游客设置或登录的启动回复丢失时，Usque 会查询一次当前任务，不重复提交登录请求。
状态确认前暂停新的设置和账号变更；补查也失败时，点击“刷新状态”找回任务，
再继续验证或取消。确认没有正在运行的任务后，才允许重新开始。

有效 access token 不强制轮换；一次操作中，
只有受保护接口返回 401 时才尝试一次带持久化日志的恢复。已知的新令牌先保存，
后续目录或证书失败不会把成功刷新误标为“重新登录”；刷新结果未知才要求明确
重新认证，绝不重放旧 refresh token。运行时按期限续签同一密钥的证书，授权失败
不会降级为仅 WARP 出口。

连接错误明确标明 Proton，不会引导重置 WARP 账号。节点暂时受限时保持连接中并
阻断业务流量，超出连接预算仍未解除则提示重试或换节点。客户端证书过期会自动
续签；吊销或签名错误会更换隧道密钥并完整重建内层 WireGuard，五分钟内重复
触发则保留 Proton 原因并停止。连接数上限、密钥重复使用和账号政策限制分别
显示，附带允许公开的 API／HTTP 或 agent 数字代码。定时续证暂时失败时继续使用
尚未过期的证书，30 秒后重试并逐步退避至最多五分钟；证书过期、令牌轮换结果
未知或 agent 不再授权时仍会阻断流量。

在账号菜单中点击“移除账号”，选择“退出并移除”会先尝试撤销本应用拥有的会话，再删除本机
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

The encrypted account record retains the complete eligible server catalogue,
up to 20,000 logical servers. Its serialized plaintext is limited to 8 MiB and
its encrypted envelope to 12 MiB on every supported platform. Linux and Android
encryption accept this account-record budget; ordinary imported chain profiles
retain their separate 192 KiB plaintext and 256 KiB ciphertext limits. Records
over their applicable limits fail without truncating the catalogue or replacing
an existing valid record.

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
New Windows/Linux guests use the separate fixed synthetic Android context
described above; the full device frame is never persisted or exported.

会话、Ed25519 seed 和证书保存在平台加密私有侧车中，与普通设置分离；密码不保存，
验证码、挑战 URL/token、HTTP 原文及私钥不进入普通状态和诊断。证书校验绑定本地
公钥和 API/X.509 时间；1–8 张不同 DER 的链只作严格解析，issuer 来源依赖经过
TLS 校验的 API，不另行认证。轮转前写持久化标记；Unix 目录同步和 Windows
write-through 替换已实现，但真实断电恢复未实测。免费游客资格、第三方身份接受性、
其他二次验证、真实 local-agent 和平台清理仍需实网验证。8004 表示不允许客户端
创建会话，界面显示失败步骤和数字代码，不会当作拼图。Android 按固定 APK 兼容
版本统一 API 标识和 User-Agent，设备字段取真实系统信息，细节见协议参考。
Windows/Linux 新游客采用上述独立的固定合成 Android 配置，完整设备帧不落盘、
不导出。

加密账号记录保留最多 20,000 个可用逻辑节点，所有支持平台的序列化明文上限为
8 MiB，加密封装上限为 12 MiB。Linux 和 Android 加密器支持此账号容量；普通
导入链配置仍受独立的 192 KiB 明文、256 KiB 密文限制。记录超限时明确失败，
不截断节点目录，也不替换已有有效记录。

See the [chain guide](CHAIN_PROXY.md), [pinned protocol reference](PROTON_VPN_PROTOCOL.md)
and [native validation record](PROTON_VPN_NATIVE_VALIDATION.md). The standalone
[Python PoC](../tool/proton_poc/README.md) remains a separate historical tool.
The native SRP implementation retains the
[Proton go-srp MIT notice](../apps/usque_gui/assets/licenses/proton-srp.txt) in
source and in the application's license registry.
