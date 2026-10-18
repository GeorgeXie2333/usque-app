<!--
Prepare v0.3.2 from source changes after v0.3.1; metadata targets 0.3.2+26.
Keep English first and immediately follow it with Simplified Chinese.
Recheck versioned schema facts against source. Rendering is not approval.
-->

## Usque {{release_tag}} official release / Usque {{release_tag}} 正式版发布

Usque {{release_tag}} is a feature and reliability release that adds Chrome 155 MASQUE TLS fingerprints and built-in IPv6 endpoint presets, supports live Ads rule updates, and improves registration, DNS recovery, Android lifecycle handling and update checks.

Usque {{release_tag}} 是一个功能与可靠性版本，新增 Chrome 155 MASQUE TLS 指纹与内置 IPv6 端点预设，支持 Ads 规则在线更新，并改进注册、DNS 恢复、Android 生命周期处理和更新检查。

## Highlights / 更新亮点 ✨

- **Chrome 155 MASQUE TLS** — Select Chrome 155 or the original handshake, now labeled unsafe, in advanced network settings. New installations and configurations without a saved selection use Chrome 155; explicit saved choices remain. The profile applies to H2, H3 and experimental L4 MASQUE, not complete browser traffic emulation. Authentication and endpoint pins remain mandatory.
  <br>**Chrome 155 MASQUE TLS** — 在高级网络设置选择 Chrome 155 或现标记为 unsafe 的原握手。新安装及未保存选择的配置使用 Chrome 155，已保存的显式选择保留。该配置作用于 H2、H3 与实验性 L4 MASQUE，不是完整模拟浏览器流量；认证及端点公钥校验始终保留。

- **Built-in IPv6 endpoint presets** — Choose United States, Germany or Finland; the Engine matches the authenticated account's tier. Each connection discovers current IPv6 candidates. IPv6-only stops on failure; Prefer IPv6 may try the retained IPv4 endpoint after retryable failures, possibly in another region. This selects an ingress, not a promised final exit location.
  <br>**内置 IPv6 端点预设** — 可选择美国、德国或芬兰，Engine 按已验证账号的类型匹配地址，每次连接重新发现 IPv6 候选。仅 IPv6 模式失败即停止；优先 IPv6 可在可重试故障后尝试保留的 IPv4，其地区可能不同。预设选择入口，不保证最终出口地区。

- **Live Ads rules and country data** — Enable Ads to download and verify its rules, inspect rule details, retry or update now. Complete verified replacements apply to new requests without reconnecting; failed updates keep the previous good revision. Country downloads verify the aggregate cache, and countries with GeoIP but no GeoSite category can still provide IP matching.
  <br>**Ads 规则在线更新与国家数据** — 启用 Ads 后下载并校验规则，可查看详情、重试或立即更新。完整有效的新规则无需重连即可用于新请求，更新失败保留上一有效版本。国家数据下载校验聚合缓存，只有 GeoIP 而无 GeoSite 分类的国家仍可按 IP 匹配。

- **Registration, DNS and direct routing recovery** — Registration uses a bounded fallback for eligible failures, with UI deadlines covering both requests. WARP DNS bootstrap and stale DoT connections recover more reliably. DIRECT traffic in a known unavailable physical address family fails locally instead of silently using WARP; available direct candidates remain eligible.
  <br>**注册、DNS 与直连恢复** — 注册对符合条件的故障使用有界备用请求，界面截止时间覆盖两次请求。改进 WARP DNS 引导与失效 DoT 连接恢复。已知物理网络不支持的地址族，其 DIRECT 流量在本地失败，不会静默改用 WARP；可用直连候选仍可继续尝试。

- **Android lifecycle and shared layouts** — First-run setup no longer requests VPN authorization; Android requests it when a connection needs VPN. Redundant connection confirmation is removed. After the task finishes, the UI process can exit once tracked preference writes are complete, without stopping the separate VPN service. Primary pages, chain summaries and launcher shortcut labels are aligned across layouts.
  <br>**Android 生命周期与统一布局** — 首次引导不再申请 VPN 授权，在连接确实需要 VPN 时由 Android 申请，并移除重复连接确认。任务结束后，界面进程在已跟踪的偏好写入完成后可退出，不停止独立 VPN 服务。统一主要页面、链式摘要及各布局中的启动器快捷标签。

- **Updates and Windows compatibility** — The updater queries the latest stable release directly and distinguishes unavailable, rate-limited and invalid responses. Windows packages support Windows 10 2004/build 19041 or later, retaining the x64-v2 CPU requirement. UI allocation changes and dependency refreshes are maintenance work, not measured performance or memory-saving guarantees.
  <br>**更新检查与 Windows 兼容性** — 更新器直接查询最新稳定版，区分服务不可用、限流与无效响应。Windows 软件包支持 Windows 10 2004／内部版本 19041 或更新版本，x64-v2 CPU 要求不变。界面分配优化与依赖刷新属于维护改动，不代表已证实性能或内存收益。

## Download / 下载 📥

> [!IMPORTANT]
> Download packages only from this release. Do not install Pull Request artifacts, local builds, or files redistributed elsewhere.
>
> 请仅从此 Release 下载软件包。不要安装 Pull Request 产物、本地构建或其他渠道转载的文件。

| OS / 系统 | Requirements / 版本要求 | Direct links / 点击直链下载 |
| :---: | --- | --- |
| ![Android](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android.svg?raw=true)<br>**Android** | **Android 8.0+ (API 26)**<br>Compatible with Android TV<br>支持 Android TV | [![APK ARMv8 (arm64-v8a)](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-arm64-v8a.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-arm64-v8a.apk) [![APK x64 (x86_64)](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-x86_64.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-x86_64.apk)<br>[![APK ARMv7 (armeabi-v7a)](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-armeabi-v7a.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-armeabi-v7a.apk) [![APK Universal](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-universal.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-universal.apk) |
| ![Windows](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/windows.svg?raw=true)<br>**Windows** | **Windows 10 2004+ (build 19041)**<br>Build 19041 or later<br>内部版本 19041 或更高 | [![EXE x64-v2](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/windows-x64-v2.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-windows-x64-v2.exe) [![EXE ARM64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/windows-arm64.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-windows-arm64.exe) |
| **Linux amd64** | Ubuntu 22.04+, Debian 12+, supported Fedora<br>systemd, GNOME/KDE, X11/Wayland<br>受支持的 Fedora；需要 systemd | [AppImage](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-amd64.AppImage) · [deb](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-amd64.deb) · [rpm](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-amd64.rpm) |
| **Linux arm64** | Ubuntu 22.04+, Debian 12+, supported Fedora<br>systemd, GNOME/KDE, X11/Wayland<br>受支持的 Fedora；需要 systemd | [AppImage](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-arm64.AppImage) · [deb](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-arm64.deb) · [rpm](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-arm64.rpm) |

For Windows, use the linked installer EXE. The similarly named MSI assets are
reserved for Usque's verified in-app update flow.

Windows 请下载上方的 EXE 安装程序。名称相近的 MSI 文件仅供应用内更新使用。

<details>
<summary>Package selection and installation guide / 软件包选择与安装指南</summary>

Use the package matching your device architecture. The universal APK contains all three Android ABIs and is larger; use it only when the device ABI is unknown.

请优先下载与设备架构匹配的软件包。Universal APK 包含三种 Android ABI，文件更大，仅在无法确定设备架构时使用。

For complete installation, upgrade, and uninstall guidance, see the [installation guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md).

完整的安装、升级和卸载说明请参阅[安装指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md)。

</details>

## Before upgrading / 升级须知

<details>
<summary>Upgrade behavior and compatibility / 升级行为与兼容性</summary>

- **Configuration advances from v0.3.1's schema 24 to 26.** Schema 25 adds the shared TLS fingerprint, defaulting missing settings to Chrome 155; schema 26 initializes older settings without an active IPv6 preset. Explicit saved fingerprint choices remain. Older engines reject migrated data. Recovery journal 6 reads compatible older records conservatively; an older Agent cannot read journal 6. Preserve recoverable pre-upgrade data, keep the newer-Agent-first upgrade sequence, and do not lower schema numbers or downgrade over migrated data.
  <br>**配置从 v0.3.1 的 schema 24 升至 26。** schema 25 新增共用 TLS 指纹，缺失设置默认 Chrome 155；schema 26 将旧设置初始化为未启用 IPv6 预设。已保存的显式指纹选择保留。旧 Engine 拒绝迁移后的数据。恢复日志 6 保守读取兼容的旧记录，旧 Agent 无法读取日志 6。请保留可恢复的升级前数据，不改变新版 Agent 优先安装的顺序，不要降低 schema 编号或覆盖已迁移数据降级。

- **Ads stays off by default.** Enabling it attempts a verified rule download. Without a valid catalog, connections remain allowed and the UI reports blocking inactive; failures retain the last complete catalog. Toggle changes and complete replacements affect new requests without reconnecting, not existing flows or DNS answers cached by applications. Custom routing and country edits retain their separate apply lifecycle. Apps' encrypted DNS and platform exclusions remain visibility boundaries.
  <br>**Ads 仍默认关闭。** 启用时尝试下载并验证规则。没有有效库时允许连接，界面提示拦截未生效；失败保留上一完整规则库。开关与完整规则替换无需重连即可作用于新请求，不追溯已有流量或应用缓存的 DNS 答案。自定义分流与国家设置保留各自应用流程。应用加密 DNS 与平台排除仍是可见性边界。

- **WARP DNS remains Plain DNS by default.** Select and apply DoH or DoT to use it; applying a DNS change to a connected session reconnects that session. The selector does not replace direct DNS, proxy DNS choices saved by older versions, or the final chain exit's resolver policy.
  <br>**WARP DNS 默认仍为普通 DNS。** 选择并应用 DoH 或 DoT 后才会启用；已连接时应用 DNS 更改会重新连接。该选项不替代直连 DNS、旧版本保存的代理 DNS 选择或最终链出口的解析策略。

- **Zero Trust remains experimental.** Manual endpoints are not restricted to Cloudflare address ranges. Use only trusted, authorized addresses after reading the risk notice; editing them does not add organization policy, posture checks or full Cloudflare One Client compatibility.
  <br>**Zero Trust 仍属实验性功能。** 手动端点不限制 Cloudflare 地址范围。请阅读风险提示，仅使用可信且获授权的地址；端点编辑不增加组织策略、设备状态检查或完整 Cloudflare One Client 兼容能力。

- **The chain proxy is off for new installations; saved selections are retained.** Select and apply one exit to use it. A terminal exit failure stops final traffic without a WARP-only fallback. HTTP/SOCKS VPN sessions retain blocking according to the applied Kill Switch and handoff policy; unconfirmed cleanup never releases protection. Android system Always-on VPN and Block connections without VPN are still required for blocking after the VPN process ends. Explicit direct rules still apply.
  <br>**新安装默认关闭链式代理，升级保留已保存的选择。** 选择并应用一个出口后才会使用。出口终止失败会停止最终流量，不会退回仅使用 WARP 隧道。HTTP/SOCKS VPN 会话按已生效的 Kill Switch 与交接策略保留阻断；清理未确认时不会解除保护。Android 若需要在 VPN 进程结束后继续阻断，仍须启用系统的“始终开启的 VPN”和“阻止未使用 VPN 的连接”。显式直连规则继续生效。

- **UDP-based exits require non-L4 mode.** OpenVPN over UDP, WireGuard and WARP via WireGuard cannot be enabled with experimental L4; the page prompts you to switch to non-L4 mode and apply.
  <br>**基于 UDP 的出口需要非 L4 模式。** OpenVPN UDP、WireGuard 和 WARP via WireGuard 不能在实验性 L4 下启用，页面会提示切换为非 L4 模式并应用。

- **The Windows virtual adapter can remain after disconnecting.** Usque restores the connection's network settings at disconnect, keeps the adapter for reuse, and attempts to remove it when you fully exit the app. Windows may take time to complete removal, which can affect an immediate restart.
  <br>**Windows 断开连接后可能仍显示虚拟网卡。** Usque 会恢复该连接修改的网络设置，保留网卡供下次连接复用，完全退出应用后再尝试移除。Windows 完成删除可能需要时间，因此立即重启应用仍可能受影响。

- **New installations use Automatic endpoint selection.** Existing configurations retain Custom and their saved addresses. CONNECT-IP with Auto transport and CUBIC remain the defaults. Disable QUIC remains off as a saved preference, but HTTP/SOCKS5 exits always block proxied UDP/443 to prefer TCP web traffic. Direct traffic and Usque's outer HTTP/3 connection are unaffected. L4 and BBRv3 remain experimental. New installations enable Allow local network; upgrades retain its saved value.
  <br>**新安装使用自动端点选择。** 升级保留自定义模式与已保存地址。默认仍为 CONNECT-IP、Auto 传输与 CUBIC。“禁用 QUIC”的保存偏好仍默认关闭，但 HTTP/SOCKS5 出口始终拦截经代理的 UDP/443，使网页流量优先使用 TCP。直连流量与 Usque 外层 HTTP/3 连接不受影响。L4、BBRv3 仍属实验性；新安装开启“允许访问局域网”，升级保留其保存值。

- **Android VPN consent is deferred to connections that need it.** First-run permission setup is notification-only, and notifications remain optional. Android's VPN authorization may disconnect another VPN; a local-proxy-only connection does not request it. Open Always-on VPN settings from Connection & protection; startup preferences remain under Application → System integration.
  <br>**Android VPN 授权延后到确实需要它的连接。** 首次权限引导只处理可选的通知权限。Android 的 VPN 授权可能断开其他 VPN；仅本地代理连接不申请该权限。“始终开启的 VPN”仍位于“连接与保护”，启动偏好仍位于“应用 → 系统集成”。

</details>

<details>
<summary>Technical changes / 技术改动详情</summary>

- VPN-protocol exits connect through the WARP tunnel, negotiate and authenticate, apply their final network configuration, and only then admit traffic. HTTP/SOCKS5 exits also connect through the WARP tunnel, with final TCP connections using CONNECT. Exit endpoint names resolve inside the WARP tunnel and protocol UDP uses the WARP service's private network stack, so Usque opens no physical socket to the exit server. OpenVPN moves to its next listed server only after DNS, dial, transport-close or timeout failures, within a 120-second candidate budget; authentication, certificate and configuration errors stop immediately. WireGuard accepts one peer and enforces partial AllowedIPs in both directions. With H3, UDP-based exits cap TCP MSS for the nested encapsulation.
  <br>VPN 协议出口先经 WARP 隧道连接、完成协商与认证并应用最终网络配置，之后才接收流量。HTTP/SOCKS5 出口同样经 WARP 隧道连接，最终 TCP 连接使用 CONNECT。出口服务器域名在 WARP 隧道内解析，协议 UDP 使用 WARP 私有网络栈，Usque 不会为出口服务器打开物理网络套接字。OpenVPN 仅在 DNS、拨号、传输关闭或超时失败时尝试下一个服务器，候选阶段总计最多 120 秒；认证、证书和配置错误会立即停止。WireGuard 支持单个 Peer，并在收发两个方向执行部分 AllowedIPs。使用 H3 时，基于 UDP 的出口会按嵌套封装开销限制 TCP MSS。

- Imported configurations are encrypted per record with current-user DPAPI on Windows and Android Keystore AES-256-GCM on Android, and are shared by all accounts on the device. WARP via WireGuard registers a separate identity through MASQUE and never converts the outer identity. Generation status is process-local; saved configurations and endpoint overrides remain encrypted. After MASQUE starts, WireGuard attempt limits are 3, 4, 5, 5, 5 and 5 seconds. Each failed session is cleaned up before another attempt; cancellation and the overall deadline still apply. Exhaustion stops the chain without a WARP-only fallback. Custom WireGuard exits retain their existing retry behavior.
  <br>导入的配置逐条加密保存：Windows 使用当前用户 DPAPI，Android 使用 Android Keystore AES-256-GCM，并由设备上的所有账号共用。WARP via WireGuard 经 MASQUE 注册独立身份，不会转换外层身份。生成状态仅保留在当前进程，已保存配置与端点覆盖仍加密保存。MASQUE 建立后，WireGuard 各次尝试的时限依次为 3、4、5、5、5、5 秒。每次失败会话清理后才开始下一次，取消与总截止时间仍然有效；尝试耗尽会停止整条链路，不会回退为仅使用 WARP 隧道。自定义 WireGuard 出口保留原有重试行为。

- OpenVPN and WireGuard retain batch file import, validation and filename-based names; an OpenVPN configuration accepts up to 16 servers. HTTP/SOCKS5 chain records retain encrypted storage, optional authentication and their Add proxy DNS choices. These are separate from the removed local-proxy DNS controls. Readiness confirms endpoint reachability; a real CONNECT is required for TCP-forwarding verification, and SOCKS5 UDP ASSOCIATE acceptance does not prove end-to-end UDP delivery.
  <br>OpenVPN 与 WireGuard 保留批量文件导入、校验与按文件名命名；OpenVPN 配置最多支持 16 个服务器。HTTP/SOCKS5 链式记录保留加密存储、可选认证及“添加代理”中的 DNS 选项，与已移除的本地代理 DNS 控件不同。就绪状态确认端点可达；TCP 转发验证需要实际 CONNECT，SOCKS5 UDP ASSOCIATE 被接受不代表端到端 UDP 转发已验证。

- VPN-protocol final DNS starts with UDP and adds alternatives after 250 ms under one four-second question deadline. HTTP/SOCKS5 Automatic DNS defaults to verified Cloudflare® DoH through the final proxy; custom or non-default inherited DNS retains TCP DNS. Application-selected UDP/53 queries are converted to TCP at that resolver, including DNS-only local SOCKS5 associations. A refused port-53 CONNECT fails explicitly. DoH failure never switches to plaintext, physical DNS or another exit. See the chain guide for explicit DNS choices and budgets.
  <br>VPN 协议最终 DNS 先使用 UDP，250 ms 后加入备用候选，每个问题共用 4 秒期限。HTTP/SOCKS5 自动 DNS 默认经最终代理使用校验 TLS 的 Cloudflare DoH；自定义或非默认继承 DNS 保留 TCP。应用指定的 UDP/53 查询转换为发往该解析器的 TCP，包括仅承载 DNS 的本地 SOCKS5 关联。端口 53 的 CONNECT 被拒绝时明确失败。DoH 失败不会转为明文、物理 DNS 或另一出口；显式 DNS 选择与预算见链式代理指南。

- The native established CONNECT-IP supervisor stops automatic attempts after authentication, identity, configuration, address-assignment and socket-protection failures. Android's service separately retains an established ordinary session after a typed protection failure and confirmed cleanup, waiting for a newer usable physical generation before one attempt; repeated protection failure waits for another generation. There is no timed protection retry on the same network. Ordinary network failures keep bounded backoff; offline observations pause attempts. An HTTP/2 PING without a reply ends the session after a 15 to 30 second final deadline.
  <br>原生 CONNECT-IP 已建立会话的监督器在认证、身份、配置、地址分配或套接字保护失败后停止自动尝试。Android 服务另行处理已建立的普通会话：明确的保护失败及清理确认后，等待更新的可用物理网络代次再尝试一次；再次保护失败则等待下一个代次，不会在同一网络上定时重试。普通网络故障保留有上限的退避重试；离线观测暂停尝试。HTTP/2 PING 无回复时，会在 15 到 30 秒的最终期限后结束会话。

- Configuration schema is 26, Agent protocol is 3, recovery journal is 6 and sanitized recovery export schema is 2. HTTP/SOCKS5 records remain version 5 and VPN records version 3. TLS, live Ads and preset/lease IPC additions are append-only; the retired pre-release Chrome131 enum number is not reused. Compatible journals 2–5 migrate conservatively and cannot claim newer preset authorization. Diagnostic exports retain their privacy filtering.
  <br>配置 schema 为 26、Agent 协议为 3、恢复日志为 6、脱敏恢复导出为 2。HTTP/SOCKS5 记录仍为 5，VPN 记录仍为 3。TLS、Ads 在线更新与预设／租约 IPC 仅追加；弃用的预发布 Chrome131 枚举编号不复用。兼容的日志 2–5 保守迁移，不能冒用新预设授权；诊断导出继续执行隐私过滤。

- IPv6 presets use bounded physical-network AAAA discovery and operation/generation-scoped DNS and MASQUE authorization. Network changes, cancellation and cleanup invalidate volatile candidates. Chrome 155 is a fixed TLS profile: ECH GREASE is not real ECH encryption, and advertising ML-DSA identifiers does not add certificate verification support; unsupported peer requirements fail closed. Neither feature bypasses socket protection, client authentication or endpoint pins.
  <br>IPv6 预设使用有界的物理网络 AAAA 发现，以及限定操作／网络代次的 DNS 与 MASQUE 授权。网络变化、取消和清理会使临时候选失效。Chrome 155 是固定 TLS 配置：ECH GREASE 不是真正的 ECH 加密，声明 ML-DSA 标识不增加对应证书验证能力，不支持的服务端要求会安全失败。两项功能均不绕过套接字保护、客户端认证或端点公钥校验。

- The multilingual Windows EXE installers introduced in v0.2.6 remain the user-facing packages; signed MSIs remain reserved for verified in-app updates. The newer-Agent-first upgrade bridge introduced in v0.2.5 remains in place for v0.2.4 upgrades. Release APKs compress native libraries, which Android extracts at installation, and Dart symbols are kept outside the installable packages. Download size is not installed disk usage.
  <br>v0.2.6 引入的多语言 Windows EXE 安装程序仍为用户安装入口；签名 MSI 仍仅供经过验证的应用内更新。v0.2.5 引入的新版 Agent 优先安装机制继续为 v0.2.4 升级提供兼容桥接。Release APK 压缩原生库，Android 会在安装时解压；Dart 符号不放入安装包。下载体积不等于安装后的磁盘占用。

Protected Windows, Android, leak-observer and performance validation is supplemental; missing runs remain `not_run` and do not establish upgrade, leak or performance results.

受保护的 Windows、Android、外部泄漏观测与性能验证属于补充检查；未运行的项目保留 `not_run`，不代表已经取得升级、泄漏或性能验证结果。

</details>

<details>
<summary>DNS privacy, chain exits and L4 behavior / DNS 隐私、链式出口与 L4 行为</summary>

### DNS privacy / DNS 隐私

Built-in IPv6 presets resolve their account-tier/region hostname through physical-network DNS before MASQUE is established. The physical DNS provider and directory operator may observe those discovery requests; this is separate from tunneled application DNS and does not guarantee anonymity, availability or final exit location.

内置 IPv6 预设在 MASQUE 建立前通过物理网络 DNS 解析账号等级／地区域名。物理 DNS 提供商与目录运营方可能看到这些发现请求；这与隧道内的应用 DNS 不同，不保证匿名性、可用性或最终出口地区。

GeoSite-matched country queries and custom DIRECT-domain queries use the selected direct DNS mode. System (the default) exposes them to the physical DNS provider; DoH or DoT uses the configured encrypted resolver with numeric bootstrap and strict TLS, with no plaintext fallback. REJECT names receive a local refusal without an upstream query. Other remote queries use the WARP tunnel or the selected final chain exit. WARP encrypted DNS may omit bootstrap IPs and resolve its server name through configured Plain DNS inside WARP; Direct DNS requires bootstrap IPs. WireGuard prefers its configured DNS; HTTP/SOCKS5 chain DNS defaults to verified DoH through the final proxy, retaining explicit TCP and older local choices. The local Proxy page has no DNS editor. Apps using their own encrypted DNS hide domains from Usque, so routing uses IP rules.

与 GeoSite 匹配的国家查询及自定义 DIRECT 域名查询使用所选直连 DNS 模式。System（默认）将查询发送给当前网络的 DNS 服务器；DoH 或 DoT 使用填写的 IP 连接加密解析器并严格校验 TLS，失败时不改用明文。REJECT 域名在本地拒绝，不查询上游。其他远端查询使用 WARP 隧道或所选最终链式出口。WARP 加密 DNS 可省略引导 IP，改由 WARP 内配置的普通 DNS 解析服务器域名；直连 DNS 仍要求引导 IP。WireGuard 优先使用配置中的 DNS；HTTP/SOCKS5 链式 DNS 默认经最终代理使用校验 TLS 的 DoH，保留显式 TCP 与旧本机选择。本地代理页没有 DNS 编辑器。应用自行使用加密 DNS 时域名不可见，路由按 IP 规则判断。

With the chain proxy enabled, the WARP provider carries the exit connection and the selected exit server provides final egress; its operator can observe traffic leaving that tunnel subject to application encryption. VPN Gate directory services also learn directory requests, and VPN Gate exits are public volunteer servers. Existing Geo, CIDR, LAN, system-proxy bypass and Android application exceptions retain their direct behavior. Public node scores and TCP observations are not local end-to-end measurements or promises of availability. No automatic telemetry or diagnostic upload is added. See the [chain proxy guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/CHAIN_PROXY.md) and the [VPN Gate guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/VPN_GATE.md) for the complete boundaries.

启用链式代理后，WARP 提供商承载出口连接，所选出口服务器提供最终出口，其运营方可以看到从该出口发出的流量，但 HTTPS 等应用层加密仍保护其加密内容。VPN Gate 目录服务还可见目录请求，VPN Gate 出口为公共志愿服务器。已有 Geo、CIDR、LAN、系统代理绕过及 Android 应用例外保留直连行为。公共节点评分与 TCP 观测不是本地端到端测量，也不保证可用性。不新增自动遥测或诊断上传。完整边界请参阅[链式代理指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/CHAIN_PROXY.md)和 [VPN Gate 指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/VPN_GATE.md)。

Without a chain exit, experimental L4 remains TCP-only: valid tunneled UDP/53 queries are converted to TCP DNS and preserve the application-selected resolver IP. EdgeResolved for L4 SOCKS5/HTTP sends hostnames to the CONNECT edge without a local lookup; it cannot recover names from TUN IP traffic. An OpenVPN-over-TCP exit, either a custom OpenVPN TCP configuration or a VPN Gate node, can carry application UDP as IP packets inside its OpenVPN TCP connection. Neither mode silently converts failed proxied traffic into direct traffic.

未启用链式出口时，实验性 L4 仍仅支持 TCP：有效的隧道 UDP/53 查询转换为 TCP DNS，并保留应用指定的解析器 IP。L4 SOCKS5/HTTP 的 EdgeResolved 会将域名发送至 CONNECT 边缘节点而不进行本地查询，不能从 TUN IP 流量还原域名。基于 OpenVPN TCP 的出口（自定义 OpenVPN TCP 配置或 VPN Gate 节点）可将应用的 UDP 流量作为 IP 数据包承载于其 OpenVPN TCP 连接。两种模式都不会静默将失败的代理流量转为直连。

</details>

## Verify before installing / 安装前验证 🔐

<details>
<summary>Signature checks and release evidence / 签名校验与发布验证材料</summary>

1. Compare the package SHA-256 with both [SHA256SUMS](https://github.com/{{repository}}/releases/download/{{release_tag}}/SHA256SUMS) and the digest displayed by GitHub.
   <br>将软件包 SHA-256 同时与 [SHA256SUMS](https://github.com/{{repository}}/releases/download/{{release_tag}}/SHA256SUMS) 及 GitHub 显示的摘要进行比对。

2. Verify that the package signer matches the fingerprint below. The [installation guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md#verify-before-installing) has commands and expected fields.
   <br>确认软件包签名者与下方指纹一致。具体命令和需要比较的字段见[安装指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md#verify-before-installing)。

3. Stop if the filename, hash, signature, architecture, or version differs.
   <br>如文件名、哈希、签名、架构或版本有任何不一致，请停止安装。

- Windows Authenticode certificate SHA-256 / Windows Authenticode 证书 SHA-256: `{{windows_signer_sha256}}`
- Android release certificate SHA-256 / Android Release 证书 SHA-256: `{{android_signer_sha256}}`
- Linux Ed25519 public key / Linux Ed25519 公钥: `{{linux_ed25519_public_key}}`

For Linux, verify the architecture-specific `.manifest.json` and `.sig` against this fixed public key before installing. AppImage service installation needs administrator approval; credentials require an unlocked Secret Service keyring. See the [Linux guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/LINUX_DESKTOP.md).

Linux 安装前须使用此固定公钥验证对应架构的 `.manifest.json` 和 `.sig`。AppImage 后台服务安装需要管理员授权，凭据需要已解锁的 Secret Service 钥匙串。详见 [Linux 指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/LINUX_DESKTOP.md)。

> [!NOTE]
> Before v1.0, Windows packages use a fixed self-signed identity and may show an unknown-publisher warning. Android packages use a fixed project-controlled certificate and are not distributed through Google Play.
>
> v1.0 之前的 Windows 软件包使用固定的自签名身份，系统可能显示“未知发布者”警告。Android 软件包使用由项目管理的固定证书，且不通过 Google Play 分发。

Release evidence: [manifest](https://github.com/{{repository}}/releases/download/{{release_tag}}/release-manifest.json) · [SHA-256 checksums](https://github.com/{{repository}}/releases/download/{{release_tag}}/SHA256SUMS) · per-package SPDX SBOMs attached to this release

发布验证材料：[清单](https://github.com/{{repository}}/releases/download/{{release_tag}}/release-manifest.json) · [SHA-256 校验和](https://github.com/{{repository}}/releases/download/{{release_tag}}/SHA256SUMS) · 此 Release 附带的逐包 SPDX SBOM

</details>

## Feedback / 问题反馈 💬

<details>
<summary>Reporting guidelines and links / 反馈指南与入口</summary>

Detailed, reproducible reports are prioritized. Include the exact version, platform, expected result, actual result, and minimal reproduction steps. Remove credentials, tokens, device identifiers, endpoint pins, and personal addresses from logs and attachments.

信息完整且可复现的报告会被优先处理。请提供准确版本、平台、预期结果、实际结果和最小复现步骤，并从日志与附件中移除凭据、令牌、设备标识符、端点 Pin 和个人地址。

- Bug report / 错误反馈: [Open the bug form / 打开错误反馈表单](https://github.com/{{repository}}/issues/new?template=bug.yml)
- Feature request / 功能建议: [Open the feature form / 打开功能建议表单](https://github.com/{{repository}}/issues/new?template=feature.yml)
- Security issue / 安全问题: [Report privately / 私密报告](https://github.com/{{repository}}/security/advisories/new)

</details>

---

Cloudflare and WARP are trademarks and/or registered trademarks of Cloudflare, Inc. in the United States and other jurisdictions.

Cloudflare 和 WARP 是 Cloudflare, Inc. 在美国及其他司法管辖区的商标和/或注册商标。
