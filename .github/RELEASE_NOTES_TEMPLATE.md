<!--
Before each release, replace the highlights with the user-visible changes in
that release. Keep English first and put the Simplified Chinese translation
immediately below the matching English text.
-->

## Highlights / 更新亮点

- Usque v0.2.4 is a feature and reliability release that adds Network Quality, encrypted direct DNS, and more resilient transport and platform lifecycle handling.
  <br>Usque v0.2.4 是一个功能与可靠性版本，新增网络质量中心、加密直连 DNS，并强化传输与平台生命周期处理。
- Network Quality provides a local, non-persistent 60-second view of RTT, loss availability, queue pressure, PMTU, migration, and direct DNS. Network Doctor keeps Standard checks read-only and requires explicit authorization for Deep checks.
  <br>网络质量中心提供本地且不持久化的 60 秒视图，展示 RTT、丢包可用性、队列压力、PMTU、迁移和直连 DNS；Network Doctor 的 Standard 检查保持只读，Deep 检查则需要明确授权。
- HTTP/3 can now migrate across same-family physical paths and automatically discover the outer-path PMTU. Tuned HTTP/2 flow control, owned packet-buffer reuse, and bounded Android/Linux UDP batching improve resilience and throughput without unbounded queues.
  <br>HTTP/3 现在可在相同地址族的物理路径间迁移，并自动探测外层路径 PMTU；调优后的 HTTP/2 流控、自有数据包缓冲区复用以及 Android/Linux 有界 UDP 批处理，在不引入无界队列的前提下提升可靠性与吞吐。
- Direct-country DNS now supports explicit System, DoH, and DoT resolvers with numeric bootstrap and strict TLS. Encrypted resolver failures never silently downgrade to plaintext.
  <br>直连国家规则的 DNS 现在可明确选择 System、DoH 或 DoT，并使用数字 IP 引导和严格 TLS；加密解析器失败时绝不会静默降级为明文。
- Windows and Android now bind recovery and direct egress to exact network generations, clean up more consistently across shutdown and detach paths, and recover orphaned VPN state without starting a new tunnel on an unsafe or stale platform state.
  <br>Windows 与 Android 现在将恢复和直连出口绑定到准确的网络代次，在关机及分离路径中执行更一致的清理，并可恢复孤立的 VPN 状态，避免在不安全或过期的平台状态上启动新隧道。

### DNS privacy / DNS 隐私

GeoSite-matched direct-country queries use the selected direct DNS mode. System (the default) exposes them to the physical DNS provider; DoH or DoT exposes them to the configured encrypted resolver using numeric bootstrap and strict TLS, with no plaintext fallback. Other queries continue through WARP DNS. Apps that use their own encrypted DNS hide the domain from Usque, so those connections fall back to GeoIP routing.

与 GeoSite 匹配的直连国家规则查询会使用所选的直连 DNS 模式。System（默认）会将查询暴露给物理 DNS 提供商；DoH 或 DoT 则使用数字 IP 引导和严格 TLS，将查询发送给配置的加密解析器，且不会回退到明文。其他查询继续通过 WARP DNS。应用自行使用加密 DNS 时，Usque 无法获知域名，相应连接会回退到 GeoIP 路由。

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
| Windows | Windows 10 22H2 (build 19045) or later.<br>Windows 10 22H2（内部版本 19045）或更高版本。 | [x64-v2 MSI](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-windows-x64-v2.msi)<br>[ARM64 MSI](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-windows-arm64.msi) |
| Android / Android TV | Android 8.0 (API 26) or later. Android TV is supported.<br>Android 8.0（API 26）或更高版本，支持 Android TV。 | [ARM64-v8a APK](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-arm64-v8a.apk)<br>[x86_64 APK](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-x86_64.apk)<br>[ARMv7 APK](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-armeabi-v7a.apk)<br>[Universal APK](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-universal.apk) |

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
