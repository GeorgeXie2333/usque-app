<!--
Before each release, replace the version summary and highlights with the
user-visible changes in that release. Also recheck every versioned fact in the
Technical changes block, such as the configuration schema, Agent protocol,
recovery journal and recovery export schema numbers, against the source. Keep
English first and put the Simplified Chinese translation immediately below the
matching English text.
-->

## Usque {{release_tag}} official release / Usque {{release_tag}} 正式版发布

Usque {{release_tag}} adds custom OpenVPN and WireGuard exits and a WARP via WireGuard exit to the chain proxy, adds an optional Disable QUIC setting, and improves reconnection after network changes.

Usque {{release_tag}} 为链式代理新增自定义 OpenVPN、WireGuard 出口和 WARP via WireGuard 出口，新增可选的“禁用 QUIC”设置，并改进网络变化后的重新连接。

## Highlights / 更新亮点 ✨

- **Custom OpenVPN and WireGuard exits** — Import or paste your own configuration in Proxy → Chain proxy and use it as the exit for your VPN, SOCKS5 and HTTP traffic, with the connection carried over WARP. An OpenVPN configuration can list up to 16 servers to try while connecting.
  <br>**自定义 OpenVPN 与 WireGuard 出口** — 在“代理 → 链式代理”中导入或粘贴自己的配置，作为 VPN、SOCKS5 和 HTTP 流量的出口，并通过 WARP 连接该服务器。OpenVPN 配置最多可列出 16 个服务器，建立连接时依次尝试。
- **WARP via WireGuard exit** — Generate a separate WARP WireGuard configuration or import one, edit its endpoint, and look for working endpoints with a scan you can pause and resume.
  <br>**WARP via WireGuard 出口** — 生成独立的 WARP WireGuard 配置或导入已有配置，可编辑端点，并通过可暂停、可继续的扫描查找可用端点。
- **One page for every chain exit** — OpenVPN, WireGuard, WARP via WireGuard and VPN Gate share one page with the current connection and an apply bar. Home shows the active chain as WARP → exit name.
  <br>**统一的链式出口页面** — OpenVPN、WireGuard、WARP via WireGuard 和 VPN Gate 共用同一页面，显示当前连接并提供应用栏。首页以“WARP → 出口名称”显示正在使用的链路。
- **Optional Disable QUIC** — Block UDP/443 through the proxy or tunnel from Settings → Advanced network settings → Routing & protection. GEO direct traffic and Usque's own HTTP/3 connection are unaffected, and the change applies without reconnecting.
  <br>**可选的禁用 QUIC** — 在“设置 → 高级网络设置 → 路由与保护”中拦截经代理或隧道转发的 UDP/443。GEO 直连和 Usque 自身的 HTTP/3 连接不受影响，应用后无需重连。
- **Reconnection after network changes** — On Android and in Windows VPN mode, an established connection waits while the device is offline and reconnects once a usable network returns. A stalled HTTP/2 connection now ends within a bounded time so recovery can start.
  <br>**网络变化后重新连接** — 在 Android 和 Windows VPN 模式下，已建立的连接会在设备离线时等待，并在可用网络恢复后重新连接。停滞的 HTTP/2 连接会在限定时间内结束，以便开始恢复。
- **Smaller downloads** — Release APKs compress their native libraries, and the apps no longer bundle unused icon font variants.
  <br>**更小的下载体积** — Release APK 压缩原生库，应用也不再附带未使用的图标字体变体。

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

Use the package matching your device architecture. The universal APK contains all three Android ABIs and is larger; use it only when the device ABI is unknown.

请优先下载与设备架构匹配的软件包。Universal APK 包含三种 Android ABI，文件更大，仅在无法确定设备架构时使用。

For complete installation, upgrade, and uninstall guidance, see the [installation guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md).

完整的安装、升级和卸载说明请参阅[安装指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md)。

## Before upgrading / 升级须知

- **The chain proxy is off by default.** An existing VPN Gate choice carries over to the VPN Gate source. Select and apply one exit to use it. A terminal exit failure disconnects the whole chain; there is no automatic WARP-only fallback. On Android, enable system Always-on VPN and Block connections without VPN if apps must stay blocked after the VPN ends. Your explicit direct rules still apply.
  <br>**链式代理默认关闭。** 已有的 VPN Gate 选择会保留为 VPN Gate 来源。选择并应用一个出口后才会使用。出口无法继续连接时，整条链路都会断开，不会自动退回仅使用 WARP。Android 用户若需要在 VPN 结束后继续阻止应用联网，请开启系统的“始终开启的 VPN”和“阻止未使用 VPN 的连接”。手动设置的直连规则仍然生效。
- **UDP-based exits require CONNECT-IP.** OpenVPN over UDP, WireGuard and WARP via WireGuard cannot be enabled with experimental L4; the page offers Switch to CONNECT-IP and apply instead.
  <br>**基于 UDP 的出口需要 CONNECT-IP。** OpenVPN UDP、WireGuard 和 WARP via WireGuard 不能在实验性 L4 下启用，页面会改为提供“切换为 CONNECT-IP 并应用”。
- **The Windows virtual adapter can remain after disconnecting.** Usque restores the connection's network settings at disconnect, keeps the adapter for reuse, and attempts to remove it when you fully exit the app. Windows may take time to complete removal, which can affect an immediate restart.
  <br>**Windows 断开连接后可能仍显示虚拟网卡。** Usque 会恢复该连接修改的网络设置，保留网卡供下次连接复用，完全退出应用后再尝试移除。Windows 完成删除可能需要时间，因此立即重启应用仍可能受影响。
- **Default connection settings are unchanged.** CONNECT-IP with Auto and CUBIC remain the defaults, and Disable QUIC is off. L4 and BBRv3 are experimental; keep that in mind when choosing them. New installations turn on Allow local network; existing saved settings keep their value.
  <br>**默认连接设置保持不变。** 默认仍使用 CONNECT-IP、Auto 和 CUBIC，“禁用 QUIC”默认关闭。L4 与 BBRv3 仍为实验性选项，请按需选择。新安装默认开启“允许访问局域网”；已保存的设置保持原值。

<details>
<summary>Technical changes / 技术改动详情</summary>

- Every chain exit connects through WARP, negotiates and authenticates, applies its final network configuration, and only then admits traffic. Exit endpoint names resolve inside WARP and protocol UDP uses WARP's private network stack, so Usque opens no physical socket to the exit server. OpenVPN moves to its next listed server only after DNS, dial, transport-close or timeout failures, within a 120-second candidate budget; authentication, certificate and configuration errors stop immediately. WireGuard accepts one peer and enforces partial AllowedIPs in both directions. With H3, UDP-based exits cap TCP MSS for the nested encapsulation.
  <br>所有链式出口都先经 WARP 连接、完成协商与认证并应用最终网络配置，之后才接收流量。出口服务器域名在 WARP 内解析，协议 UDP 使用 WARP 私有网络栈，Usque 不会为出口服务器打开物理网络套接字。OpenVPN 仅在 DNS、拨号、传输关闭或超时失败时尝试下一个服务器，候选阶段总计最多 120 秒；认证、证书和配置错误会立即停止。WireGuard 支持单个 Peer，并在收发两个方向执行部分 AllowedIPs。使用 H3 时，基于 UDP 的出口会按嵌套封装开销限制 TCP MSS。
- Imported configurations are encrypted per record with current-user DPAPI on Windows and Android Keystore AES-256-GCM on Android, and are shared by all accounts on the device. WARP via WireGuard registers a separate identity through the MASQUE tunnel and never converts the outer MASQUE identity. Endpoint scans use their own scan identity, test one candidate at a time, never probe over the physical network, and store encrypted progress and results.
  <br>导入的配置逐条加密保存：Windows 使用当前用户 DPAPI，Android 使用 Android Keystore AES-256-GCM，并由设备上的所有账号共用。WARP via WireGuard 经 MASQUE 隧道注册独立身份，不会转换外层 MASQUE 身份。端点扫描使用独立的扫描身份，每次测试一个候选，不通过物理网络探测，进度与结果加密保存。
- Final-exit DNS starts with UDP, adds an alternative after 250 ms, uses TCP when needed, and shares a four-second deadline per question. Remote DNS never falls back to physical DNS. Disable QUIC matches UDP destination port 443 after GEO direct routing and updates a running connection without reconnecting.
  <br>最终出口的 DNS 查询先使用 UDP，250 ms 后启用备用候选，必要时改用 TCP，每个问题共用 4 秒期限。远端 DNS 不会回退到物理 DNS。“禁用 QUIC”在 GEO 直连判断之后按 UDP 目标端口 443 匹配，可在连接中更新而无需重连。
- Established CONNECT-IP sessions stop automatic reconnection after authentication, identity, configuration, address-assignment and socket-protection failures. Ordinary network failures keep bounded backoff; Android and Windows VPN network observations pause attempts while the device is confirmed offline and start one shortly after a usable network appears. An HTTP/2 PING without a reply ends the session after a 15 to 30 second final deadline.
  <br>已建立的 CONNECT-IP 会话遇到认证、身份、配置、地址分配或套接字保护失败时，停止自动重连。普通网络故障保留有上限的退避重试；Android 和 Windows VPN 的网络观测在确认设备离线时暂停尝试，并在出现可用网络后很快发起连接。HTTP/2 PING 长时间无回复时，会在 15 到 30 秒的最终期限后结束会话。
- Configuration schema is 18: schema 16 adds Disable QUIC, schema 17 moves the VPN Gate switch into the chain exit setting, and schema 18 adds the optional WARP via WireGuard endpoint override. Agent protocol remains 3, the recovery journal remains schema 3, and sanitized recovery exports remain schema 2. IPC fields are appended only. Sensitive identity and configuration data are excluded from diagnostic exports.
  <br>配置 schema 为 18：schema 16 新增“禁用 QUIC”，schema 17 将 VPN Gate 开关迁移到链式出口设置，schema 18 新增 WARP via WireGuard 可选端点覆盖。Agent 协议仍为 3，恢复日志仍为 schema 3，脱敏恢复导出仍为 schema 2。IPC 字段仅追加。诊断导出排除敏感身份与配置数据。
- The multilingual Windows EXE installers introduced in v0.2.6 remain the user-facing packages; signed MSIs remain reserved for verified in-app updates. The newer-Agent-first upgrade bridge introduced in v0.2.5 remains in place for v0.2.4 upgrades. Release APKs compress native libraries, which Android extracts at installation, and Dart symbols are kept outside the installable packages. Dependency maintenance includes reviewed Rust, Flutter and Actions updates. No measured performance improvement or real-machine upgrade result is claimed.
  <br>v0.2.6 引入的多语言 Windows EXE 安装程序仍为用户安装入口；签名 MSI 仍仅供经过验证的应用内更新。v0.2.5 引入的新版 Agent 优先安装机制继续为 v0.2.4 升级提供兼容桥接。Release APK 压缩原生库，Android 会在安装时解压；Dart 符号不放入安装包。依赖维护包含经审查的 Rust、Flutter 和 Actions 更新。不宣称已测得性能提升或已完成真实机器升级验证。

</details>

<details>
<summary>DNS privacy, chain exits and L4 behavior / DNS 隐私、链式出口与 L4 行为</summary>

### DNS privacy / DNS 隐私

GeoSite-matched direct-country queries use the selected direct DNS mode. System (the default) exposes them to the physical DNS provider; DoH or DoT exposes them to the configured encrypted resolver using numeric bootstrap and strict TLS, with no plaintext fallback. Other remote queries use the final tunnel's DNS: WARP, or the selected chain exit (OpenVPN, WireGuard, WARP via WireGuard, or VPN Gate) when the chain proxy is enabled. A WireGuard exit prefers the DNS servers in its configuration. Explicit local/direct DNS choices remain in effect. Apps using their own encrypted DNS hide domains from Usque, so routing falls back to GeoIP classification.

与 GeoSite 匹配的直连国家规则查询会使用所选直连 DNS 模式。System（默认）会将查询发送给当前网络使用的 DNS 服务器。DoH 或 DoT 使用填写的 IP 地址连接加密 DNS 服务器，并校验其 TLS 身份；失败时不改用明文 DNS。其他远端查询使用最终隧道的 DNS：通常为 WARP，启用链式代理后则为所选出口（OpenVPN、WireGuard、WARP via WireGuard 或 VPN Gate）。WireGuard 出口优先使用其配置中的 DNS 服务器。显式本地或直连 DNS 选择仍然生效。应用自行使用加密 DNS 时，Usque 无法获知域名，路由会回退至 GeoIP 分类。

With the chain proxy enabled, the WARP provider carries the exit connection and the selected exit server provides final egress; its operator can observe traffic leaving that tunnel subject to application encryption. VPN Gate directory services also learn directory requests, and VPN Gate exits are public volunteer servers. Existing Geo, CIDR, LAN, system-proxy bypass and Android application exceptions retain their direct behavior. Public node scores, endpoint scan results and TCP observations are not local end-to-end measurements or promises of availability. No automatic telemetry or diagnostic upload is added. See the [chain proxy guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/CHAIN_PROXY.md) and the [VPN Gate guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/VPN_GATE.md) for the complete boundaries.

启用链式代理后，WARP 提供商承载出口连接，所选出口服务器提供最终出口，其运营方可以看到从该出口发出的流量，但 HTTPS 等应用层加密仍保护其加密内容。VPN Gate 目录服务还可见目录请求，VPN Gate 出口为公共志愿服务器。已有 Geo、CIDR、LAN、系统代理绕过及 Android 应用例外保留直连行为。公共节点评分、端点扫描结果与 TCP 观测不是本地端到端测量，也不保证可用性。不新增自动遥测或诊断上传。完整边界请参阅[链式代理指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/CHAIN_PROXY.md)和 [VPN Gate 指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/VPN_GATE.md)。

Without a chain exit, experimental L4 remains TCP-only: valid tunneled UDP/53 queries are converted to TCP DNS and preserve the application-selected resolver IP. EdgeResolved for L4 SOCKS5/HTTP sends hostnames to the CONNECT edge without a local lookup; it cannot recover names from TUN IP traffic. An OpenVPN-over-TCP exit, either a custom OpenVPN TCP configuration or a VPN Gate node, can carry application UDP as IP packets inside its OpenVPN TCP connection. Neither mode silently converts failed proxied traffic into direct traffic.

未启用链式出口时，实验性 L4 仍仅支持 TCP：有效的隧道 UDP/53 查询转换为 TCP DNS，并保留应用指定的解析器 IP。L4 SOCKS5/HTTP 的 EdgeResolved 会将域名发送至 CONNECT 边缘节点而不进行本地查询，不能从 TUN IP 流量还原域名。基于 OpenVPN TCP 的出口（自定义 OpenVPN TCP 配置或 VPN Gate 节点）可将应用的 UDP 流量作为 IP 数据包承载于其 OpenVPN TCP 连接。两种模式都不会静默将失败的代理流量转为直连。

</details>

## Verify before installing / 安装前验证 🔐

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

## Feedback / 问题反馈 💬

Detailed, reproducible reports are prioritized. Include the exact version, platform, expected result, actual result, and minimal reproduction steps. Remove credentials, tokens, device identifiers, endpoint pins, and personal addresses from logs and attachments.

信息完整且可复现的报告会被优先处理。请提供准确版本、平台、预期结果、实际结果和最小复现步骤，并从日志与附件中移除凭据、令牌、设备标识符、端点 Pin 和个人地址。

- Bug report / 错误反馈: [Open the bug form / 打开错误反馈表单](https://github.com/{{repository}}/issues/new?template=bug.yml)
- Feature request / 功能建议: [Open the feature form / 打开功能建议表单](https://github.com/{{repository}}/issues/new?template=feature.yml)
- Security issue / 安全问题: [Report privately / 私密报告](https://github.com/{{repository}}/security/advisories/new)
