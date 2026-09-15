<!--
Before each release, replace the highlights with the user-visible changes in
that release. Keep English first and put the Simplified Chinese translation
immediately below the matching English text.
-->

## Usque {{release_tag}} official release / Usque {{release_tag}} 正式版发布

Usque {{release_tag}} is a feature and reliability release that adds optional WARP-chained VPN Gate exits, improves Windows TUN device lifecycle and recovery handling, and refines the Windows and Android experience.

Usque {{release_tag}} 是一个功能与可靠性版本，新增可选的 WARP 串联 VPN Gate 出口，改进 Windows TUN 设备生命周期与恢复处理，并优化 Windows 与 Android 使用体验。

## Highlights / 更新亮点 ✨

- **Optional VPN Gate exits** — Send proxied traffic through a selected volunteer exit over WARP, with a shared final session for system VPN, SOCKS5 and HTTP.
  <br>**可选 VPN Gate 出口** — 经 WARP 将代理流量发送至选定的志愿出口，系统 VPN、SOCKS5 和 HTTP 共用最终会话。
- **Server pool and local favorites** — Browse by country, keep local configuration snapshots, and distinguish the connected server from a pending selection.
  <br>**服务器池与本地收藏** — 按国家浏览、保存本地配置快照，并区分当前连接节点与待应用的选择。
- **Windows device reuse and recovery** — Retain the managed TUN device between connections and strengthen ownership, cleanup and recovery-record persistence.
  <br>**Windows 设备复用与恢复** — 在连接之间保留受管理的 TUN 设备，并强化所有权、清理与恢复记录持久化。
- **Android lifecycle fixes** — Restore status subscriptions after background resume and coordinate final-tunnel handoff, stop confirmation and failure cleanup.
  <br>**Android 生命周期修复** — 后台恢复后重新订阅状态，并协调最终隧道交接、停止确认与失败清理。
- **Clearer controls and feedback** — Refine proxy navigation, connection retry, form validation and screen-reader labels, with bundled offline country flags.
  <br>**更清晰的操作与反馈** — 优化代理导航、连接重试、表单验证和屏幕阅读器标签，并内置离线国家旗帜。

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

Windows 请使用上方链接的安装程序 EXE。同名 MSI 资产仅供 Usque 经过验证的应用内更新流程使用。

Use the package matching your device architecture. The universal APK contains all three Android ABIs and is larger; use it only when the device ABI is unknown.

请优先下载与设备架构匹配的软件包。Universal APK 包含三种 Android ABI，文件更大，仅在无法确定设备 ABI 时使用。

For complete installation, upgrade, and uninstall guidance, see the [installation guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md).

完整的安装、升级和卸载说明请参阅[安装指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md)。

## Before upgrading / 升级须知

- **VPN Gate is off by default.** Select and save a server explicitly. A terminal Gate failure disconnects the whole chain; it does not silently use WARP or physical egress as a final fallback. Existing explicit direct exceptions remain direct.
  <br>**VPN Gate 默认关闭。** 请显式选择并保存服务器。Gate 终止失败会断开整条连接，不会静默将 WARP 或物理出口作为最终回退；已有的显式直连例外仍保持直连。
- **An idle Windows device is not an active connection.** Ordinary disconnect restores the connection's network state while retaining the application-owned device. Full application exit starts final device retirement; native deletion delays can still affect an immediate restart.
  <br>**Windows 闲置设备不代表仍在连接。** 普通断开会恢复连接的网络状态，同时保留应用持有的设备。完整退出应用后才进行最终设备退役；原生删除延迟仍可能影响立即重启。
- **Defaults and validation limits remain explicit.** CONNECT-IP with Auto and CUBIC remain the defaults; L4 and BBRv3 remain experimental. Volunteer availability, measured performance, real upgrades, VPN recovery and leak behavior are not established by deterministic tests or package inspection.
  <br>**默认值与验证边界保持明确。** 默认仍为 CONNECT-IP、Auto 和 CUBIC；L4 与 BBRv3 仍为实验性功能。确定性测试和软件包检查不能证明志愿节点可用性、实测性能、真实升级、VPN 恢复或泄漏行为。

<details>
<summary>Technical changes / 技术改动详情</summary>

- VPN Gate embeds an OpenVPN TCP session carried by the private WARP dialer, without a second OS VPN interface or an external OpenVPN process. The final tunnel supplies addresses, DNS and MTU; unsupported proxied IPv6 is blocked. One internal startup authentication retry reuses the same saved node only after the rejected worker stops. Cancellation closes admission, and an unconfirmed bounded stop remains pending rather than being reported as complete.
  <br>VPN Gate 内嵌由私有 WARP 拨号器承载的 OpenVPN TCP 会话，不创建第二个系统 VPN 接口或外部 OpenVPN 进程。地址、DNS 与 MTU 来自最终隧道；不支持的代理 IPv6 会被阻止。启动认证仅内部重试一次，并且必须在被拒绝的工作线程停止后才复用同一已保存节点。取消会关闭流量准入，有界停止未确认时仍记为待完成，不冒充停止成功。
- The cumulative directory validates pinned snapshots and configuration hashes before use. Local favorites, saved selections, drafts and in-flight preparations retain independent references. Refresh failures or disappearing nodes do not silently replace saved configurations. Traffic samples describe the final Gate channel, while RTT, loss and congestion observations describe the WARP underlay, not the full volunteer-exit path.
  <br>累积目录在使用前验证固定快照和配置哈希。本地收藏、已保存选择、草稿与进行中的配置准备分别持有独立引用。刷新失败或节点消失不会静默替换已保存配置。流量采样描述最终 Gate 通道，而 RTT、丢包与拥塞观测描述 WARP 底层连接，并非经过志愿出口的完整路径。
- Windows separates application-lifetime device leases from connection packet leases, joins packet waiters before replacement, and rejects stale ownership and generation results. Recovery journal schema 3 conservatively reads schema 2; failed retirement-record writes retry without repeating native deletion or restarting the connection recovery budget. Configuration schema stays at 15, Agent protocol is 3, and sanitized recovery exports remain schema 2. Sensitive identity and configuration data are excluded from diagnostic exports.
  <br>Windows 将应用生命周期设备租约与连接数据包租约分离，在替换前等待数据包等待线程结束，并拒绝过期所有权及代次结果。恢复日志 schema 3 保守读取 schema 2；设备退役记录写入失败会重试，但不会重复原生删除或重置连接恢复预算。配置 schema 保持 15，Agent 协议为 3，脱敏恢复导出仍为 schema 2。诊断导出排除敏感身份与配置数据。
- Android restores status subscriptions after resume, binds exit probes to the final tunnel, and coordinates interface handoff and terminal cleanup. Ending a failed VPN restores ordinary network access outside Android Lockdown; in-process traffic admission is not an independent OS Kill Switch. UI changes improve proxy status cards, country filtering, local proxy controls, validation priority and appearance-picker accessibility.
  <br>Android 在恢复后重新订阅状态，将出口探测绑定至最终隧道，并协调接口交接与终止清理。在 Android Lockdown 之外，结束失败的 VPN 会恢复普通网络访问；进程内流量准入不等于独立的系统 Kill Switch。界面改进涵盖代理状态卡、国家筛选、本地代理控制、验证反馈优先级和外观选择器无障碍标签。
- The multilingual Windows EXE installers introduced in v0.2.6 remain the user-facing packages; signed MSIs remain reserved for verified in-app updates. The newer-Agent-first upgrade bridge introduced in v0.2.5 remains in place for v0.2.4 upgrades. Dependency maintenance includes reviewed Rust, Flutter, Kotlin and Actions updates, with locked native source and license inventories included in release SBOMs. No measured performance improvement or real-machine upgrade result is claimed.
  <br>v0.2.6 引入的多语言 Windows EXE 安装程序仍为用户安装入口；签名 MSI 仍仅供经过验证的应用内更新。v0.2.5 引入的新版 Agent 优先安装机制继续为 v0.2.4 升级提供兼容桥接。依赖维护包含经审查的 Rust、Flutter、Kotlin 和 Actions 更新，发布 SBOM 纳入锁定的原生源码与许可证清单。不宣称已测得性能提升或已完成真实机器升级验证。

</details>

<details>
<summary>DNS privacy, VPN Gate and L4 behavior / DNS 隐私、VPN Gate 与 L4 行为</summary>

### DNS privacy / DNS 隐私

GeoSite-matched direct-country queries use the selected direct DNS mode. System (the default) exposes them to the physical DNS provider; DoH or DoT exposes them to the configured encrypted resolver using numeric bootstrap and strict TLS, with no plaintext fallback. Other remote queries use the final tunnel's DNS: WARP normally, or VPN Gate when enabled. Explicit local/direct DNS choices remain in effect. Apps using their own encrypted DNS hide domains from Usque, so routing falls back to GeoIP classification.

与 GeoSite 匹配的直连国家规则查询会使用所选直连 DNS 模式。System（默认）会将查询暴露给物理 DNS 提供商；DoH 或 DoT 使用数字 IP 引导和严格 TLS，将查询发送给配置的加密解析器，且不回退至明文。其他远端查询使用最终隧道的 DNS：通常为 WARP，启用 VPN Gate 后则为 VPN Gate。显式本地或直连 DNS 选择仍然生效。应用自行使用加密 DNS 时，Usque 无法获知域名，路由会回退至 GeoIP 分类。

VPN Gate directory services learn directory requests, the WARP provider carries the OpenVPN connection, and the selected volunteer provides final egress and can observe traffic leaving that tunnel subject to application encryption. Existing Geo, CIDR, LAN, system-proxy bypass and Android application exceptions retain their direct behavior. Public node scores and TCP observations are not local end-to-end measurements or promises of availability. No automatic telemetry or diagnostic upload is added. See the [VPN Gate guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/VPN_GATE.md) for the complete boundaries.

VPN Gate 目录服务可见目录请求，WARP 提供商承载 OpenVPN 连接，所选志愿节点提供最终出口，并可在应用加密的边界内观察离开该隧道的流量。已有 Geo、CIDR、LAN、系统代理绕过及 Android 应用例外保留直连行为。公共节点评分与 TCP 观测不是本地端到端测量，也不保证可用性。不新增自动遥测或诊断上传。完整边界请参阅 [VPN Gate 指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/VPN_GATE.md)。

Without VPN Gate, experimental L4 remains TCP-only: valid tunneled UDP/53 queries are converted to TCP DNS and preserve the application-selected resolver IP. EdgeResolved for L4 SOCKS5/HTTP sends hostnames to the CONNECT edge without a local lookup; it cannot recover names from TUN IP traffic. VPN Gate can carry business UDP as IP packets inside its OpenVPN TCP channel. Neither mode silently converts failed proxied traffic into direct traffic.

未启用 VPN Gate 时，实验性 L4 仍仅支持 TCP：有效的隧道 UDP/53 查询转换为 TCP DNS，并保留应用指定的解析器 IP。L4 SOCKS5/HTTP 的 EdgeResolved 会将域名发送至 CONNECT 边缘节点而不进行本地查询，不能从 TUN IP 流量还原域名。VPN Gate 可将业务 UDP 作为 IP 数据包承载于其 OpenVPN TCP 通道。两种模式都不会静默将失败的代理流量转为直连。

</details>

## Verify before installing / 安装前验证 🔐

1. Compare the package SHA-256 with both [SHA256SUMS](https://github.com/{{repository}}/releases/download/{{release_tag}}/SHA256SUMS) and the digest displayed by GitHub.
   <br>将软件包 SHA-256 同时与 [SHA256SUMS](https://github.com/{{repository}}/releases/download/{{release_tag}}/SHA256SUMS) 及 GitHub 显示的摘要进行比对。
2. Verify that the package signer matches the fingerprint below.
   <br>确认软件包签名者与下方指纹一致。
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
