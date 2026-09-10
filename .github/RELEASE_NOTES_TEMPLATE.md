<!--
Before each release, replace the highlights with the user-visible changes in
that release. Keep English first and put the Simplified Chinese translation
immediately below the matching English text.
-->

## Highlights / 更新亮点

- Usque v0.2.6 is a feature and reliability release that adds experimental L4 proxying, selectable HTTP/3 congestion control, and multilingual Windows installers, while improving network settings and cross-platform lifecycle handling.
  <br>Usque v0.2.6 是一个功能与可靠性版本，新增实验性 L4 代理、可选 HTTP/3 拥塞控制及多语言 Windows 安装程序，并改进网络设置与跨平台生命周期处理。
- Experimental L4 carries TCP over HTTP/3 CONNECT for SOCKS5, HTTP, and the bounded VPN/TUN bridge. It is explicitly selected, excludes general UDP, and never silently falls back to CONNECT-IP/H2 or replays established TCP connections. Backpressure fixes preserve accepted data and reverse-direction progress; download paths reduce copies and reuse receive buffers. CONNECT-IP with Auto remains the default.
  <br>实验性 L4 通过 HTTP/3 CONNECT 承载 SOCKS5、HTTP 及有界 VPN/TUN 桥接中的 TCP 流量。它必须显式选择，不支持通用 UDP，不会静默回退至 CONNECT-IP/H2，也不会重放已建立的 TCP 连接。背压修复保留已接收数据及反向传输进度；下载路径减少复制并复用接收缓冲。默认仍为 CONNECT-IP 与 Auto。
- HTTP/3 now offers CUBIC, Reno, BBRv2, and an independent experimental BBRv3, with fixes for stalled BBRv2 sending. CUBIC remains the default; a saved algorithm applies to the next manually started session, not an already running connection. Windows and Android QUIC sockets make a best-effort 2 MiB receive-buffer request and report actual OS values. These implementation changes are not measured throughput or fairness claims.
  <br>HTTP/3 现在提供 CUBIC、Reno、BBRv2 和独立的实验性 BBRv3，并修复 BBRv2 发送停滞。默认仍为 CUBIC；保存的算法会在下次手动启动会话时生效，不会改变正在运行的连接。Windows 与 Android 的 QUIC 套接字会尽力请求 2 MiB 接收缓冲，并报告操作系统实际值。这些实现改动不代表已测得吞吐或公平性提升。
- Network settings distinguish durable saves from runtime application and track each uncertain operation independently. Late replies cannot confirm a different save or overwrite newer state. Android application tokens, session generations, and supported profile-store file locking preserve ownership; native stop completion must be confirmed before replacing an unfinished runtime.
  <br>网络设置区分持久化保存与运行时生效，并独立跟踪每个状态未确认的操作。迟到的回复不能确认另一次保存或覆盖较新状态。Android 通过应用令牌、会话代次及受支持的配置文件锁保留操作归属；未完成的原生运行实例必须确认停止后才能被替换。
- v0.2.6 introduces signed Windows installer EXEs with 21 interface languages, while retaining signed MSI assets for verified in-app updates. Localized uninstall and quiet-launcher fixes preserve command quoting, wait for the final result, and clean up the hidden bundle registration. The newer-Agent-first upgrade bridge introduced in v0.2.5 remains in place for upgrades from v0.2.4.
  <br>v0.2.6 新增支持 21 种界面语言的签名 Windows EXE 安装程序，同时保留签名 MSI 资产供经过验证的应用内更新使用。本地化卸载及静默启动器修复保留命令引号、等待最终结果，并清理隐藏的安装包注册信息。v0.2.5 引入的新版 Agent 优先安装机制继续为 v0.2.4 升级提供兼容桥接。
- Feature translations, account terminology, Windows tray/uninstall text, and Android notifications, Quick Settings, and shortcuts are completed across the 21 supported languages. Updated guides explain L4 limitations, congestion settings, and diagnostic evidence. Real installation, upgrade, VPN recovery, leak, and performance validation requires isolated environments; deterministic tests and package inspection are not those runtime results.
  <br>补全 21 种支持语言中的功能翻译、账户术语、Windows 托盘与卸载文案，以及 Android 通知、快捷设置和快捷方式。更新的指南说明 L4 限制、拥塞设置及诊断证据。真实安装、升级、VPN 恢复、泄漏及性能验证需要隔离环境；确定性测试和软件包检查不等同于这些运行时验证结果。

### DNS privacy / DNS 隐私

GeoSite-matched direct-country queries use the selected direct DNS mode. System (the default) exposes them to the physical DNS provider; DoH or DoT exposes them to the configured encrypted resolver using numeric bootstrap and strict TLS, with no plaintext fallback. Other queries continue through WARP DNS. Apps that use their own encrypted DNS hide the domain from Usque, so those connections fall back to GeoIP routing.

与 GeoSite 匹配的直连国家规则查询会使用所选的直连 DNS 模式。System（默认）会将查询暴露给物理 DNS 提供商；DoH 或 DoT 则使用数字 IP 引导和严格 TLS，将查询发送给配置的加密解析器，且不会回退到明文。其他查询继续通过 WARP DNS。应用自行使用加密 DNS 时，Usque 无法获知域名，相应连接会回退到 GeoIP 路由。

In experimental L4, valid tunneled UDP/53 queries are converted to TCP DNS; an application-selected resolver IP is preserved. Remote proxy DNS uses L4 TCP DNS, while explicitly selected LocalConfigured/System modes retain their existing resolver exposure. L4-only EdgeResolved for SOCKS5/HTTP sends the hostname to the CONNECT edge without a local lookup; it does not recover hostnames from TUN IP traffic. General UDP remains unsupported, and failed traffic does not silently become direct traffic.

在实验性 L4 中，有效的隧道 UDP/53 查询会转换为 TCP DNS，并保留应用指定的解析器 IP。远端代理 DNS 使用 L4 TCP DNS；显式选择的 LocalConfigured/System 模式仍保留原有的解析器可见性。仅适用于 L4 SOCKS5/HTTP 的 EdgeResolved 会将域名发送至 CONNECT 边缘节点而不进行本地查询，不能从 TUN IP 流量还原域名。通用 UDP 仍不受支持，失败流量不会静默转为直连。

## Usque {{release_tag}} official release / Usque {{release_tag}} 正式版发布

The packages below are the only official installers for this release.

以下安装包是此版本唯一的官方安装程序。

## Download / 下载

> [!IMPORTANT]
> Download packages only from this release. Do not install Pull Request artifacts, local builds, or files redistributed elsewhere.
>
> 请仅从此 Release 下载软件包。不要安装 Pull Request 产物、本地构建或其他渠道转载的文件。

| OS / 系统 | Requirements / 版本要求 | Direct downloads / 直接下载 |
| --- | --- | --- |
| Windows | Windows 10 22H2 (build 19045) or later.<br>Windows 10 22H2（内部版本 19045）或更高版本。 | [x64-v2 installer](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-windows-x64-v2.exe)<br>[ARM64 installer](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-windows-arm64.exe) |
| Android / Android TV | Android 8.0 (API 26) or later. Android TV is supported.<br>Android 8.0（API 26）或更高版本，支持 Android TV。 | [ARM64-v8a APK](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-arm64-v8a.apk)<br>[x86_64 APK](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-x86_64.apk)<br>[ARMv7 APK](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-armeabi-v7a.apk)<br>[Universal APK](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-universal.apk) |

For Windows, use the linked installer EXE. The similarly named MSI assets are
reserved for Usque's verified in-app update flow.

Windows 请使用上方链接的安装程序 EXE。同名 MSI 资产仅供 Usque 经过验证的应用内更新流程使用。

Use the package matching your device architecture. The universal APK contains all three Android ABIs and is larger; use it only when the device ABI is unknown.

请优先下载与设备架构匹配的软件包。Universal APK 包含三种 Android ABI，文件更大，仅在无法确定设备 ABI 时使用。

For complete installation, upgrade, and uninstall guidance, see the [installation guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md).

完整的安装、升级和卸载说明请参阅[安装指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md)。

## Verify before installing / 安装前验证

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

## Feedback / 问题反馈

> [!NOTE]
> Detailed, reproducible reports are prioritized. Include the exact version, platform, expected result, actual result, and minimal reproduction steps. Remove credentials, tokens, device identifiers, endpoint pins, and personal addresses from logs and attachments.
>
> 信息完整且可复现的报告会被优先处理。请提供准确版本、平台、预期结果、实际结果和最小复现步骤，并从日志与附件中移除凭据、令牌、设备标识符、端点 Pin 和个人地址。

- Bug report / 错误反馈: [Open the bug form / 打开错误反馈表单](https://github.com/{{repository}}/issues/new?template=bug.yml)
- Feature request / 功能建议: [Open the feature form / 打开功能建议表单](https://github.com/{{repository}}/issues/new?template=feature.yml)
- Security issue / 安全问题: [Report privately / 私密报告](https://github.com/{{repository}}/security/advisories/new)
