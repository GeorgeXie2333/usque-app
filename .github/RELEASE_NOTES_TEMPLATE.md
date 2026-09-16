<!--
Before each release, replace the highlights with the user-visible changes in
that release. Keep English first and put the Simplified Chinese translation
immediately below the matching English text.
-->

## Usque {{release_tag}} official release / Usque {{release_tag}} 正式版发布

Usque {{release_tag}} adds optional VPN Gate exits over WARP and improves connection recovery, server selection, and Windows and Android controls.

Usque {{release_tag}} 新增可选的 VPN Gate 出口，经 WARP 连接志愿服务器，并改进连接恢复、节点选择和 Windows、Android 上的操作体验。

## Highlights / 更新亮点 ✨

- **Optional VPN Gate exits** — Choose a volunteer server as the exit for your VPN, SOCKS5 and HTTP traffic, with the connection carried over WARP.
  <br>**可选的 VPN Gate 出口** — 选择志愿服务器作为 VPN、SOCKS5 和 HTTP 流量的出口，通过 WARP 连接该服务器。
- **Server selection and favorites** — Browse by country, save servers locally, and see which server is connected and which selection is waiting to be applied.
  <br>**节点选择与收藏** — 按国家浏览并收藏服务器，区分当前连接的节点和等待应用的新选择。
- **Windows disconnect and recovery** — Reuse Usque's virtual adapter between connections and improve cleanup and recovery after interruptions.
  <br>**Windows 断开与恢复** — 重连时复用 Usque 的虚拟网卡，并改进连接中断后的清理与恢复流程。
- **Android status and connection cleanup** — Restore status updates after returning from the background and improve cleanup when switching, stopping or failing to connect.
  <br>**Android 状态更新与连接清理** — 修复返回前台后状态订阅未恢复的问题，并完善切换、停止 VPN 和连接失败时的资源清理。
- **Clearer controls and feedback** — Improve proxy navigation, retry controls, input validation and screen-reader labels. Country flags are bundled for offline use.
  <br>**更清晰的操作与反馈** — 改进代理导航、重试按钮、输入校验和屏幕阅读器标签，国家旗帜也可离线显示。

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

请优先下载与设备架构匹配的软件包。Universal APK 包含三种 Android ABI，文件更大，仅在无法确定设备架构 时使用。

For complete installation, upgrade, and uninstall guidance, see the [installation guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md).

完整的安装、升级和卸载说明请参阅[安装指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md)。

## Before upgrading / 升级须知

- **VPN Gate is off by default.** Select and save a server to use it. A terminal failure disconnects the whole chain. On Android, enable system Always-on VPN and Block connections without VPN if apps must stay blocked after the VPN ends. Your explicit direct rules still apply.
  <br>**VPN Gate 默认关闭。** 选择并保存服务器后才能使用。VPN Gate 无法继续连接时，整条连接都会断开。Android 用户若需要在 VPN 结束后继续阻止应用联网，请开启系统的“始终开启的 VPN”和“阻止未使用 VPN 的连接”。手动设置的直连规则仍然生效。
- **The Windows virtual adapter can remain after disconnecting.** Usque restores the connection's network settings at disconnect, keeps the adapter for reuse, and attempts to remove it when you fully exit the app. Windows may take time to complete removal, which can affect an immediate restart.
  <br>**Windows 断开连接后可能仍显示虚拟网卡。** Usque 会恢复该连接修改的网络设置，保留网卡供下次连接复用，完全退出应用后再尝试移除。Windows 完成删除可能需要时间，因此立即重启应用仍可能受影响。
- **Default connection settings are unchanged.** CONNECT-IP with Auto and CUBIC remain the defaults. L4 and BBRv3 are experimental; keep that in mind when choosing them.
  <br>**默认连接设置保持不变。** 默认仍使用 CONNECT-IP、Auto 和 CUBIC。L4 与 BBRv3 仍为实验性选项，请按需选择。

<details>
<summary>Technical changes / 技术改动详情</summary>

- VPN Gate embeds an OpenVPN TCP session carried by the private WARP dialer, without a second OS VPN interface or an external OpenVPN process. The final tunnel supplies addresses, DNS and MTU; unsupported proxied IPv6 is blocked. One internal startup authentication retry reuses the same saved node only after the rejected worker stops. Cancellation closes admission, and an unconfirmed bounded stop remains pending rather than being reported as complete.
  <br>VPN Gate 内嵌由私有 WARP 拨号器承载的 OpenVPN TCP 会话，不创建第二个系统 VPN 接口或外部 OpenVPN 进程。地址、DNS 与 MTU 来自最终隧道；不支持的代理 IPv6 会被阻止。启动认证仅内部重试一次，并且必须在被拒绝的工作线程停止后才复用同一已保存节点。取消后不再接收新流量；若未能在超时前确认工作线程已停止，状态仍记为待清理。
- The cumulative directory validates pinned snapshots and configuration hashes before use. Local favorites, saved selections, drafts and in-flight preparations retain independent references. Refresh failures or disappearing nodes do not silently replace saved configurations. Traffic samples describe the final Gate channel, while RTT, loss and congestion observations describe the WARP underlay, not the full volunteer-exit path.
  <br>累积目录在使用前验证固定快照和配置哈希。本地收藏、已保存选择、草稿与进行中的配置准备分别持有独立引用。刷新失败或节点消失不会静默替换已保存配置。流量采样描述最终 Gate 通道，而 RTT、丢包与拥塞观测描述 WARP 底层连接，并非经过志愿出口的完整路径。
- Windows separates application-lifetime device leases from connection packet leases, joins packet waiters before replacement, and rejects stale ownership and generation results. Recovery journal schema 3 conservatively reads schema 2; failed retirement-record writes retry without repeating native deletion or restarting the connection recovery budget. Configuration schema stays at 15, Agent protocol is 3, and sanitized recovery exports remain schema 2. Sensitive identity and configuration data are excluded from diagnostic exports.
  <br>Windows 分开管理应用持有的网卡和每次连接的数据包处理权限。替换连接前会等待旧的数据包线程结束，并拒绝旧连接返回的结果。恢复日志 schema 3 兼容读取 schema 2，并保留恢复检查；网卡移除记录写入失败会重试，但不会重复原生删除或重置连接恢复预算。配置 schema 保持 15，Agent 协议为 3，脱敏恢复导出仍为 schema 2。诊断导出排除敏感身份与配置数据。
- Android restores status subscriptions after resume, binds exit probes to the final tunnel, and coordinates interface handoff and terminal cleanup. Ending a failed VPN restores ordinary network access outside Android Lockdown; in-process traffic admission is not an independent OS Kill Switch. UI changes improve proxy status cards, country filtering, local proxy controls, validation priority and appearance-picker accessibility.
  <br>Android 在恢复后重新订阅状态，将出口探测绑定至最终隧道，并在更换 VPN 接口或结束连接时清理资源。未启用 Android 系统阻断设置时，VPN 结束后会恢复普通联网；应用进程内的流量检查无法替代系统断网保护。界面改进涵盖代理状态卡、国家筛选、本地代理控制、验证反馈优先级和外观选择器无障碍标签。
- The multilingual Windows EXE installers introduced in v0.2.6 remain the user-facing packages; signed MSIs remain reserved for verified in-app updates. The newer-Agent-first upgrade bridge introduced in v0.2.5 remains in place for v0.2.4 upgrades. Dependency maintenance includes reviewed Rust, Flutter, Kotlin and Actions updates, with locked native source and license inventories included in release SBOMs. No measured performance improvement or real-machine upgrade result is claimed.
  <br>v0.2.6 引入的多语言 Windows EXE 安装程序仍为用户安装入口；签名 MSI 仍仅供经过验证的应用内更新。v0.2.5 引入的新版 Agent 优先安装机制继续为 v0.2.4 升级提供兼容桥接。依赖维护包含经审查的 Rust、Flutter、Kotlin 和 Actions 更新，发布 SBOM 纳入锁定的原生源码与许可证清单。不宣称已测得性能提升或已完成真实机器升级验证。

</details>

<details>
<summary>DNS privacy, VPN Gate and L4 behavior / DNS 隐私、VPN Gate 与 L4 行为</summary>

### DNS privacy / DNS 隐私

GeoSite-matched direct-country queries use the selected direct DNS mode. System (the default) exposes them to the physical DNS provider; DoH or DoT exposes them to the configured encrypted resolver using numeric bootstrap and strict TLS, with no plaintext fallback. Other remote queries use the final tunnel's DNS: WARP normally, or VPN Gate when enabled. Explicit local/direct DNS choices remain in effect. Apps using their own encrypted DNS hide domains from Usque, so routing falls back to GeoIP classification.

与 GeoSite 匹配的直连国家规则查询会使用所选直连 DNS 模式。System（默认）会将查询发送给当前网络使用的 DNS 服务器。DoH 或 DoT 使用填写的 IP 地址连接加密 DNS 服务器，并校验其 TLS 身份；失败时不改用明文 DNS。其他远端查询使用最终隧道的 DNS：通常为 WARP，启用 VPN Gate 后则为 VPN Gate。显式本地或直连 DNS 选择仍然生效。应用自行使用加密 DNS 时，Usque 无法获知域名，路由会回退至 GeoIP 分类。

VPN Gate directory services learn directory requests, the WARP provider carries the OpenVPN connection, and the selected volunteer provides final egress and can observe traffic leaving that tunnel subject to application encryption. Existing Geo, CIDR, LAN, system-proxy bypass and Android application exceptions retain their direct behavior. Public node scores and TCP observations are not local end-to-end measurements or promises of availability. No automatic telemetry or diagnostic upload is added. See the [VPN Gate guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/VPN_GATE.md) for the complete boundaries.

VPN Gate 目录服务可见目录请求，WARP 提供商承载 OpenVPN 连接，所选志愿节点提供最终出口，并可以看到从该出口发出的流量，但 HTTPS 等应用层加密仍保护其加密内容。已有 Geo、CIDR、LAN、系统代理绕过及 Android 应用例外保留直连行为。公共节点评分与 TCP 观测不是本地端到端测量，也不保证可用性。不新增自动遥测或诊断上传。完整边界请参阅 [VPN Gate 指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/VPN_GATE.md)。

Without VPN Gate, experimental L4 remains TCP-only: valid tunneled UDP/53 queries are converted to TCP DNS and preserve the application-selected resolver IP. EdgeResolved for L4 SOCKS5/HTTP sends hostnames to the CONNECT edge without a local lookup; it cannot recover names from TUN IP traffic. VPN Gate can carry business UDP as IP packets inside its OpenVPN TCP channel. Neither mode silently converts failed proxied traffic into direct traffic.

未启用 VPN Gate 时，实验性 L4 仍仅支持 TCP：有效的隧道 UDP/53 查询转换为 TCP DNS，并保留应用指定的解析器 IP。L4 SOCKS5/HTTP 的 EdgeResolved 会将域名发送至 CONNECT 边缘节点而不进行本地查询，不能从 TUN IP 流量还原域名。VPN Gate 可将应用的 UDP 流量 作为 IP 数据包承载于其 OpenVPN TCP 通道。两种模式都不会静默将失败的代理流量转为直连。

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
