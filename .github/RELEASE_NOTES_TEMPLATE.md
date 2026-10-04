<!--
Before each release, replace the version summary and highlights with the
user-visible changes in that release. Also recheck every versioned fact in the
Technical changes block, such as the configuration schema, Agent protocol,
recovery journal and recovery export schema numbers, against the source. Keep
English first and put the Simplified Chinese translation immediately below the
matching English text.
-->

## Usque {{release_tag}} official release / Usque {{release_tag}} 正式版发布

Usque {{release_tag}} is a feature and reliability release that adds HTTP and SOCKS5 chain exits, custom bypass targets and automatic endpoint selection, and improves connection recovery, first-run setup and local diagnostics.

Usque {{release_tag}} 是一个功能与可靠性版本，新增 HTTP 与 SOCKS5 链式出口、自定义绕过目标和自动端点选择，并改进连接恢复、首次引导与本地诊断。

## Highlights / 更新亮点 ✨

- **HTTP and SOCKS5 chain exits** — Add a proxy server in Proxy → Chain proxy alongside OpenVPN, WireGuard, WARP via WireGuard and VPN Gate. The final proxy connects through WARP and supports optional username/password authentication. Automatic chain DNS uses encrypted DoH by default; ordinary UDP depends on the exit's capabilities.
  <br>**HTTP 与 SOCKS5 链式出口** — 在“代理 → 链式代理”添加代理服务器，与 OpenVPN、WireGuard、WARP via WireGuard 和 VPN Gate 一起选用。最终代理通过 WARP 连接，可选用户名与密码认证。自动链 DNS 默认使用加密 DoH；普通 UDP 是否可用取决于出口能力。
- **Custom bypass targets** — Enter CIDRs, individual IP addresses or domains in Settings → bypass settings, then apply. Domains include their subdomains and work without downloading country rules. These are explicit direct-traffic exceptions, shared across accounts.
  <br>**自定义绕过目标** — 在“设置 → 绕过分流设置”填写 CIDR、单个 IP 或域名并应用。域名包含其子域名，无需下载国家规则即可使用。这些规则是明确的直连例外，由所有账号共用。
- **Automatic endpoint selection** — New installations race eligible Consumer endpoints and reuse the first authenticated, pinned connection. Existing settings keep Custom mode and their saved addresses. Choose Automatic or Custom in Advanced network settings; port and SNI remain editable.
  <br>**自动端点选择** — 新安装并发尝试个人版账号可用端点，复用最先完成认证与 Pin 校验的连接。升级保留自定义模式及已保存地址。在高级网络设置选择“自动选择”或“自定义”，端口与 SNI 仍可编辑。
- **Clearer Home and section navigation** — Home simplifies connection details and uses the shared 60-second traffic history. Proxy holds the TUN/VPN, SOCKS5, HTTP and Windows system-proxy switches; Settings groups protection, routing, tools and preferences. Subpages keep desktop section navigation and guard unapplied edits.
  <br>**首页与分区导航调整** — 首页简化连接详情并使用共用的最近 60 秒流量记录。“代理”集中提供虚拟网卡／VPN、SOCKS5、HTTP 与 Windows 系统代理开关；“设置”按保护、分流、工具与偏好分组。桌面子页面保留分区导航，离开前提示未应用的修改。
- **Recovery and protected proxy handoffs** — Android improves ordinary WARP and chain recovery after physical-network changes. HTTP/SOCKS VPN startup, settings replacement and account handoffs retain blocking until the replacement is ready; terminal-failure retention follows the applied Kill Switch policy. Windows retains a journaled guard through protected tunnel replacement.
  <br>**连接恢复与代理交接保护** — Android 改进物理网络变化后的普通 WARP 与链式恢复。HTTP/SOCKS VPN 启动、设置重建与账号交接在新连接就绪前保持阻断；终止失败按已生效的 Kill Switch 策略决定是否保留。Windows 在受保护的隧道替换中保留有日志记录的阻断保护。
- **Resumable first-run setup** — Interrupted account setup checks the saved result before registering again. Android now requires VPN consent during onboarding, without starting a connection; notification permission is optional.
  <br>**可恢复的首次引导** — 账号设置中断后，先检查已保存结果再重新注册。Android 现在在首次引导中要求 VPN 授权，但不会因此建立连接；通知权限可选。
- **More precise local diagnostics** — Results distinguish observed, inferred, unavailable and stale evidence. Retained timelines identify the last connection; bounded log owners and export barriers preserve complete records and strengthen redaction. No automatic upload is added.
  <br>**更准确的本地诊断** — 结果区分已观测、推断、不可用和过期证据。保留的时间线标明上次连接；有界日志管理与导出同步保留完整记录并加强脱敏。不新增自动上传。

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

- **The chain proxy is off for new installations; saved selections are retained.** Select and apply one exit to use it. A terminal exit failure stops final traffic without a WARP-only fallback. HTTP/SOCKS VPN sessions retain blocking according to the applied Kill Switch and handoff policy; unconfirmed cleanup never releases protection. Android system Always-on VPN and Block connections without VPN are still required for blocking after the VPN process ends. Explicit direct rules still apply.
  <br>**新安装默认关闭链式代理，升级保留已保存的选择。** 选择并应用一个出口后才会使用。出口终止失败会停止最终流量，不会退回仅使用 WARP。HTTP/SOCKS VPN 会话按已生效的 Kill Switch 与交接策略保留阻断；清理未确认时不会解除保护。Android 若需要在 VPN 进程结束后继续阻断，仍须启用系统的“始终开启的 VPN”和“阻止未使用 VPN 的连接”。显式直连规则继续生效。
- **UDP-based exits require non-L4 mode.** OpenVPN over UDP, WireGuard and WARP via WireGuard cannot be enabled with experimental L4; the page prompts you to switch to non-L4 mode and apply.
  <br>**基于 UDP 的出口需要非 L4 模式。** OpenVPN UDP、WireGuard 和 WARP via WireGuard 不能在实验性 L4 下启用，页面会提示切换为非 L4 模式并应用。
- **The Windows virtual adapter can remain after disconnecting.** Usque restores the connection's network settings at disconnect, keeps the adapter for reuse, and attempts to remove it when you fully exit the app. Windows may take time to complete removal, which can affect an immediate restart.
  <br>**Windows 断开连接后可能仍显示虚拟网卡。** Usque 会恢复该连接修改的网络设置，保留网卡供下次连接复用，完全退出应用后再尝试移除。Windows 完成删除可能需要时间，因此立即重启应用仍可能受影响。
- **New installations use Automatic endpoint selection.** Existing configurations retain Custom and their saved addresses. CONNECT-IP with Auto transport and CUBIC remain the defaults. Disable QUIC remains off as a saved preference, but HTTP/SOCKS5 exits always block proxied UDP/443 to prefer TCP web traffic. Direct traffic and Usque's outer HTTP/3 connection are unaffected. L4 and BBRv3 remain experimental. New installations enable Allow local network; upgrades retain its saved value.
  <br>**新安装使用自动端点选择。** 升级保留自定义模式与已保存地址。默认仍为 CONNECT-IP、Auto 传输与 CUBIC。“禁用 QUIC”的保存偏好仍默认关闭，但 HTTP/SOCKS5 出口始终拦截经代理的 UDP/443，使网页流量优先使用 TCP。直连流量与 Usque 外层 HTTP/3 连接不受影响。L4、BBRv3 仍属实验性；新安装开启“允许访问局域网”，升级保留其保存值。
- **Android first-run permissions changed.** VPN consent is required to finish onboarding, even if you later choose only local proxies; granting it may disconnect another VPN. It does not start Usque's connection. Notifications are optional. Open Always-on VPN settings from Settings → Connection & protection; startup preferences remain under Application → System integration.
  <br>**Android 首次引导权限已调整。** 完成引导需要 VPN 授权，即使之后只使用本地代理；授权可能断开其他 VPN，但不会启动 Usque 连接。通知权限可选。“始终开启的 VPN”入口位于“设置 → 连接与保护”，启动偏好仍位于“应用 → 系统集成”。

</details>

<details>
<summary>Technical changes / 技术改动详情</summary>

- VPN-protocol exits connect through WARP, negotiate and authenticate, apply their final network configuration, and only then admit traffic. HTTP/SOCKS5 exits also connect through WARP, with final TCP connections using CONNECT. Exit endpoint names resolve inside WARP and protocol UDP uses WARP's private network stack, so Usque opens no physical socket to the exit server. OpenVPN moves to its next listed server only after DNS, dial, transport-close or timeout failures, within a 120-second candidate budget; authentication, certificate and configuration errors stop immediately. WireGuard accepts one peer and enforces partial AllowedIPs in both directions. With H3, UDP-based exits cap TCP MSS for the nested encapsulation.
  <br>VPN 协议出口先经 WARP 连接、完成协商与认证并应用最终网络配置，之后才接收流量。HTTP/SOCKS5 出口同样经 WARP 连接，最终 TCP 连接使用 CONNECT。出口服务器域名在 WARP 内解析，协议 UDP 使用 WARP 私有网络栈，Usque 不会为出口服务器打开物理网络套接字。OpenVPN 仅在 DNS、拨号、传输关闭或超时失败时尝试下一个服务器，候选阶段总计最多 120 秒；认证、证书和配置错误会立即停止。WireGuard 支持单个 Peer，并在收发两个方向执行部分 AllowedIPs。使用 H3 时，基于 UDP 的出口会按嵌套封装开销限制 TCP MSS。
- Imported configurations are encrypted per record with current-user DPAPI on Windows and Android Keystore AES-256-GCM on Android, and are shared by all accounts on the device. WARP via WireGuard registers a separate identity through MASQUE and never converts the outer identity. Generation status is process-local; saved configurations and endpoint overrides remain encrypted. After MASQUE starts, WireGuard attempt limits are 3, 4, 5, 5, 5 and 5 seconds. Each failed session is cleaned up before another attempt; cancellation and the overall deadline still apply. Exhaustion stops the chain without a WARP-only fallback. Custom WireGuard exits retain their existing retry behavior.
  <br>导入的配置逐条加密保存：Windows 使用当前用户 DPAPI，Android 使用 Android Keystore AES-256-GCM，并由设备上的所有账号共用。WARP via WireGuard 经 MASQUE 注册独立身份，不会转换外层身份。生成状态仅保留在当前进程，已保存配置与端点覆盖仍加密保存。MASQUE 建立后，WireGuard 各次尝试的时限依次为 3、4、5、5、5、5 秒。每次失败会话清理后才开始下一次，取消与总截止时间仍然有效；尝试耗尽会停止整条链路，不会回退为仅使用 WARP。自定义 WireGuard 出口保留原有重试行为。
- OpenVPN and WireGuard retain batch file import, validation and filename-based names; an OpenVPN configuration accepts up to 16 servers. HTTP/SOCKS5 adds encrypted proxy records with optional authentication and a DNS transport choice. Their readiness confirms endpoint reachability; a real CONNECT is required for TCP-forwarding verification, and SOCKS5 UDP ASSOCIATE acceptance does not prove end-to-end UDP delivery.
  <br>OpenVPN 与 WireGuard 保留批量文件导入、校验与按文件名命名；OpenVPN 配置最多支持 16 个服务器。HTTP/SOCKS5 新增加密代理记录、可选认证与 DNS 传输选择。就绪状态确认端点可达；TCP 转发验证需要实际 CONNECT，SOCKS5 UDP ASSOCIATE 被接受不代表端到端 UDP 转发已验证。
- VPN-protocol final DNS starts with UDP and adds alternatives after 250 ms under one four-second question deadline. HTTP/SOCKS5 Automatic DNS defaults to verified Cloudflare DoH through the final proxy; custom or non-default inherited DNS retains TCP DNS. Application-selected UDP/53 queries are converted to TCP at that resolver, including DNS-only local SOCKS5 associations. A refused port-53 CONNECT fails explicitly. DoH failure never switches to plaintext, physical DNS or another exit. See the chain guide for explicit DNS choices and budgets.
  <br>VPN 协议最终 DNS 先使用 UDP，250 ms 后加入备用候选，每个问题共用 4 秒期限。HTTP/SOCKS5 自动 DNS 默认经最终代理使用校验 TLS 的 Cloudflare DoH；自定义或非默认继承 DNS 保留 TCP。应用指定的 UDP/53 查询转换为发往该解析器的 TCP，包括仅承载 DNS 的本地 SOCKS5 关联。端口 53 的 CONNECT 被拒绝时明确失败。DoH 失败不会转为明文、物理 DNS 或另一出口；显式 DNS 选择与预算见链式代理指南。
- Established CONNECT-IP sessions stop automatic reconnection after authentication, identity, configuration, address-assignment and socket-protection failures. Ordinary network failures keep bounded backoff; Android and Windows VPN network observations pause attempts while the device is confirmed offline and start one shortly after a usable network appears. An HTTP/2 PING without a reply ends the session after a 15 to 30 second final deadline.
  <br>已建立的 CONNECT-IP 会话遇到认证、身份、配置、地址分配或套接字保护失败时，停止自动重连。普通网络故障保留有上限的退避重试；Android 和 Windows VPN 的网络观测在确认设备离线时暂停尝试，并在出现可用网络后很快发起连接。HTTP/2 PING 长时间无回复时，会在 15 到 30 秒的最终期限后结束会话。
- Configuration schema is 21: schema 19 adds custom bypass domains, schema 20 preserves legacy endpoints in Custom mode, and schema 21 adds resumable initial-account operation state. Agent protocol remains 3; recovery journal schema 5 adds protected tunnel replacement after schema 4's automatic-endpoint receipts. Journal versions 2, 3 and 4 are read conservatively; older Agents cannot read schema 5. Sanitized recovery exports remain schema 2. HTTP/SOCKS5 records use version 5 with DNS transport metadata; VPN records remain version 3. IPC fields are appended only, and diagnostic exports exclude sensitive identity and configuration data.
  <br>配置 schema 为 21：schema 19 新增自定义绕过域名，schema 20 将旧端点保留在自定义模式，schema 21 新增可恢复的首次账号操作状态。Agent 协议仍为 3；恢复日志 schema 5 在 schema 4 自动端点记录基础上新增受保护的隧道替换。保守兼容读取日志版本 2、3、4；旧 Agent 无法读取 schema 5。脱敏恢复导出仍为 schema 2。HTTP/SOCKS5 对象使用版本 5 保存 DNS 传输元数据，VPN 对象仍为版本 3。IPC 字段仅追加，诊断导出排除敏感身份与配置数据。
- The multilingual Windows EXE installers introduced in v0.2.6 remain the user-facing packages; signed MSIs remain reserved for verified in-app updates. The newer-Agent-first upgrade bridge introduced in v0.2.5 remains in place for v0.2.4 upgrades. Release APKs compress native libraries, which Android extracts at installation, and Dart symbols are kept outside the installable packages. Download size is not installed disk usage.
  <br>v0.2.6 引入的多语言 Windows EXE 安装程序仍为用户安装入口；签名 MSI 仍仅供经过验证的应用内更新。v0.2.5 引入的新版 Agent 优先安装机制继续为 v0.2.4 升级提供兼容桥接。Release APK 压缩原生库，Android 会在安装时解压；Dart 符号不放入安装包。下载体积不等于安装后的磁盘占用。

Protected Windows, Android, leak-observer and performance validation is supplemental; missing runs remain `not_run` and do not establish upgrade, leak or performance results.

受保护的 Windows、Android、外部泄漏观测与性能验证属于补充检查；未运行的项目保留 `not_run`，不代表已经取得升级、泄漏或性能验证结果。

</details>

<details>
<summary>DNS privacy, chain exits and L4 behavior / DNS 隐私、链式出口与 L4 行为</summary>

### DNS privacy / DNS 隐私

GeoSite-matched country queries and custom bypass-domain queries use the selected direct DNS mode. System (the default) exposes them to the physical DNS provider; DoH or DoT uses the configured encrypted resolver with numeric bootstrap and strict TLS, with no plaintext fallback. Other remote queries use WARP or the selected final chain exit. WireGuard prefers its configured DNS; HTTP/SOCKS5 defaults to verified DoH through the final proxy, with explicit TCP and local DNS choices retaining their documented meaning. Apps using their own encrypted DNS hide domains from Usque, so routing uses IP rules.

与 GeoSite 匹配的国家查询及自定义绕过域名查询使用所选直连 DNS 模式。System（默认）将查询发送给当前网络的 DNS 服务器；DoH 或 DoT 使用填写的 IP 连接加密解析器并严格校验 TLS，失败时不改用明文。其他远端查询使用 WARP 或所选最终链式出口。WireGuard 优先使用配置中的 DNS；HTTP/SOCKS5 默认经最终代理使用校验 TLS 的 DoH，显式 TCP 与本地 DNS 选择保留其文档含义。应用自行使用加密 DNS 时域名不可见，路由按 IP 规则判断。

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
