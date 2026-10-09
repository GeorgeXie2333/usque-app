<!--
Before each release, replace the version summary and highlights with the
user-visible changes in that release. Also recheck every versioned fact in the
Technical changes block, such as the configuration schema, Agent protocol,
recovery journal and recovery export schema numbers, against the source. Keep
English first and put the Simplified Chinese translation immediately below the
matching English text.
This template prepares v0.3.1 from source changes after the v0.3.0 tag.
The workflow and application version surfaces target v0.3.1 / 0.3.1+25.
Validate the exact candidate before tagging; rendering is not release approval.
-->

## Usque {{release_tag}} official release / Usque {{release_tag}} 正式版发布

Usque {{release_tag}} is a feature and reliability release that adds DIRECT/REJECT/PROXY routing rules and optional Ads filtering, fixes local-network access and HTTP/3 startup during MTU discovery, and simplifies DNS and chain-proxy settings. It also improves Android Quick Settings recovery, background UI polling and adaptive icons.

Usque {{release_tag}} 是一个功能与可靠性版本，新增 DIRECT／REJECT／PROXY 分流规则和可选 Ads 拦截，修复局域网访问及 MTU 探测期间的 HTTP/3 启动问题，并简化 DNS 与链式代理设置。同时改进 Android 快捷设置恢复、后台界面轮询和自适应图标。

## Highlights / 更新亮点 ✨

- **Routing rules and Ads** — In Settings → Bypass settings, add or paste domains, IPs and CIDRs with DIRECT, REJECT or PROXY actions. More-specific rules win, and conflicting actions for the same target prevent saving. Ads is off by default and uses the downloaded GeoSite advertising/tracking category. Apply changes and reconnect when prompted; see the [routing guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/ROUTING.md) for visibility and catalog limits.
  <br>**分流规则与 Ads** — 在“设置 → 分流设置”添加或粘贴域名、IP、CIDR，并指定 DIRECT、REJECT 或 PROXY。更具体的规则优先，同一目标的冲突动作会阻止保存。Ads 默认关闭，使用下载的 GeoSite 广告与跟踪分类。应用修改并按提示重连；可见性与规则库限制见[分流指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/ROUTING.md)。

- **Local-network access across outputs** — Allow local network now applies to VPN/TUN, HTTP forwarding and CONNECT, and SOCKS5 TCP/UDP where supported. Explicit routing rules and Ads retain priority for traffic entering the Engine. Proxy listener exposure and platform exclusions retain their own boundaries.
  <br>**各输出的局域网访问** — “允许访问局域网”现对 VPN/TUN、HTTP 转发与 CONNECT、以及受支持的 SOCKS5 TCP/UDP 生效。进入 Engine 的流量仍优先遵循显式分流规则与 Ads；代理监听暴露范围和平台排除规则保留各自边界。

- **HTTP/3 startup fixes** — MTU discovery no longer blocks ordinary CONNECT-IP traffic while waiting for a probe acknowledgement. Oversized probe send errors reduce the discovery bound without discarding ordinary queued traffic; errors outside discovery retain the existing recovery policy. These fixes do not establish a measured throughput gain.
  <br>**HTTP/3 启动修复** — MTU 探测等待探测包确认时不再阻塞普通 CONNECT-IP 流量。探测包过大导致的发送错误会降低探测上限，不会丢弃普通待发流量；探测范围外的错误仍执行原有恢复策略。这些修复不代表已证实吞吐提升。

- **Simpler DNS editing** — WARP and Direct DNS use one complete DoH URL field. New encrypted drafts prefill editable Cloudflare values; saved custom values are preserved. WARP bootstrap IPs are optional, while Direct DNS requires them. Local proxy DNS controls are removed; existing DNS settings remain active internally and listener edits preserve them.
  <br>**简化 DNS 编辑** — WARP DNS 与直连 DNS 使用完整 DoH 地址输入框。新加密 DNS 草稿预填可编辑的 Cloudflare 配置，已有自定义值保留。WARP 引导 IP 可选，直连 DNS 仍要求填写。本地代理 DNS 控件已移除；已有 DNS 设置继续在底层生效，修改监听地址会保留这些设置。

- **Settings and VPN Gate navigation** — Advanced network settings are grouped by routing/protection, WARP DNS, Direct DNS, endpoint and transport. Rows, hints and dropdowns are aligned across layouts. VPN Gate is available inside Proxy → Chain proxy, with refresh, filters and favorites in the shared editor; the separate VPN Gate page is removed.
  <br>**设置与 VPN Gate 导航** — 高级网络设置按路由与保护、WARP DNS、直连 DNS、端点和传输分组，不同布局的行、提示与下拉框统一对齐。VPN Gate 位于“代理 → 链式代理”，在共用编辑器内刷新、筛选和管理收藏；独立 VPN Gate 页面已移除。

- **Android tile, observation and icons** — Quick Settings can recover connection intent after process or control-channel loss. Hidden UI pauses quality and diagnostics polling and refreshes when visible again; it does not stop the VPN service. Full-colour adaptive icons use an opaque brand-orange background so launchers do not expose a black tile; themed icons remain available.
  <br>**Android 磁贴、观测与图标** — 进程或控制通道丢失后，快捷设置可恢复连接意图。界面隐藏时暂停网络质量与诊断轮询，重新可见时刷新，不会因此停止 VPN 服务。全彩自适应图标采用不透明品牌橙色背景，避免启动器显示黑底；主题图标继续保留。

## Download / 下载 📥

> [!IMPORTANT]
> Download packages only from this release. Do not install Pull Request artifacts, local builds, or files redistributed elsewhere.
>
> 请仅从此 Release 下载软件包。不要安装 Pull Request 产物、本地构建或其他渠道转载的文件。

| OS / 系统 | Requirements / 版本要求 | Direct links / 点击直链下载 |
| :---: | --- | --- |
| ![Android](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android.svg?raw=true)<br>**Android** | **Android 8.0+ (API 26)**<br>Compatible with Android TV<br>支持 Android TV | [![APK ARMv8 (arm64-v8a)](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-arm64-v8a.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-arm64-v8a.apk) [![APK x64 (x86_64)](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-x86_64.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-x86_64.apk)<br>[![APK ARMv7 (armeabi-v7a)](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-armeabi-v7a.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-armeabi-v7a.apk) [![APK Universal](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-universal.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-universal.apk) |
| ![Windows](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/windows.svg?raw=true)<br>**Windows** | **Windows 10 22H2+ (build 19045)**<br>Build 19045 or later<br>内部版本 19045 或更高 | [![EXE x64-v2](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/windows-x64-v2.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-windows-x64-v2.exe) [![EXE ARM64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/windows-arm64.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-windows-arm64.exe) |

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

- **Configuration schema advances from v0.3.0's 23 to 24.** Legacy custom domains and CIDRs migrate to DIRECT rules, preserving country selections. Custom CIDRs now route in the application data plane instead of installing physical bypass routes. The v0.3.0 engine, and older engines such as v0.2.9, reject schema 24. Arrange any recoverable pre-upgrade backup before upgrading; do not downgrade over migrated data or manually change schema numbers. Agent protocol 3 and recovery journal 5 remaining unchanged do not establish downgrade compatibility.
  <br>**配置 schema 从 v0.3.0 的 23 升至 24。** 旧自定义域名与 CIDR 迁移为 DIRECT 规则，国家选择保留。自定义 CIDR 改由应用数据平面分流，不再安装物理网络绕过路由。v0.3.0 及 v0.2.9 等旧 Engine 会拒绝 schema 24。请在升级前安排可恢复的备份；不要在已迁移数据上直接降级或手动修改 schema 编号。Agent 协议仍为 3、恢复日志仍为 5，并不代表支持降级。

- **Ads requires a valid downloaded catalog and is off by default.** Failed updates keep the last complete catalog. With no valid catalog, Usque reports Ads unavailable but allows connections; custom rules remain active. Reconnect to load updated data. Apps' own encrypted DNS, Android excluded apps and platform exclusions limit what the rules can inspect or control. This is not a system-wide firewall.
  <br>**Ads 需要有效的已下载规则库，默认关闭。** 更新失败保留上一个完整规则库；完全没有有效库时，应用提示 Ads 不可用，但允许连接，自定义规则继续生效。更新数据后需重连加载。应用自行加密的 DNS、Android 排除应用及平台排除规则限制可检查和控制的范围；该功能不是全系统防火墙。

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

- **Android first-run VPN consent remains required.** This applies even if you later choose only local proxies; granting it may disconnect another VPN. It does not start Usque's connection. Notifications are optional. Open Always-on VPN settings from Settings → Connection & protection; startup preferences remain under Application → System integration.
  <br>**Android 首次引导仍要求 VPN 授权。** 即使之后只使用本地代理，也需完成授权；授权可能断开其他 VPN，但不会启动 Usque 连接。通知权限可选。“始终开启的 VPN”入口位于“设置 → 连接与保护”，启动偏好仍位于“应用 → 系统集成”。

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

- Configuration schema is 24: schema 22 initializes Plain WARP DNS for older settings, schema 23 initializes Zero Trust endpoint overrides as unset, preserving registered addresses, and schema 24 migrates custom bypass targets to DIRECT routing rules. Agent protocol remains 3, recovery journal remains 5 and sanitized recovery exports remain schema 2. HTTP/SOCKS5 records remain version 5 with DNS transport metadata; VPN records remain version 3. IPC fields are appended only. Diagnostic exports omit custom DNS names, paths, bootstrap addresses, query contents and sensitive identity/configuration data.
  <br>配置 schema 为 24：schema 22 将旧设置初始化为普通 WARP DNS，schema 23 将 Zero Trust 端点覆盖初始化为未设置，保留注册地址，schema 24 将自定义绕过目标迁移为 DIRECT 分流规则。Agent 协议仍为 3，恢复日志仍为 5，脱敏恢复导出仍为 schema 2。HTTP/SOCKS5 对象保留版本 5 的 DNS 传输元数据，VPN 对象仍为版本 3。IPC 字段仅追加；诊断导出排除自定义 DNS 域名、路径、引导地址、查询内容及敏感身份／配置数据。

- The multilingual Windows EXE installers introduced in v0.2.6 remain the user-facing packages; signed MSIs remain reserved for verified in-app updates. The newer-Agent-first upgrade bridge introduced in v0.2.5 remains in place for v0.2.4 upgrades. Release APKs compress native libraries, which Android extracts at installation, and Dart symbols are kept outside the installable packages. Download size is not installed disk usage.
  <br>v0.2.6 引入的多语言 Windows EXE 安装程序仍为用户安装入口；签名 MSI 仍仅供经过验证的应用内更新。v0.2.5 引入的新版 Agent 优先安装机制继续为 v0.2.4 升级提供兼容桥接。Release APK 压缩原生库，Android 会在安装时解压；Dart 符号不放入安装包。下载体积不等于安装后的磁盘占用。

Protected Windows, Android, leak-observer and performance validation is supplemental; missing runs remain `not_run` and do not establish upgrade, leak or performance results.

受保护的 Windows、Android、外部泄漏观测与性能验证属于补充检查；未运行的项目保留 `not_run`，不代表已经取得升级、泄漏或性能验证结果。

</details>

<details>
<summary>DNS privacy, chain exits and L4 behavior / DNS 隐私、链式出口与 L4 行为</summary>

### DNS privacy / DNS 隐私

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
