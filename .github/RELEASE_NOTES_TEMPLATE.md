<!--
Before each release, replace the highlights with the user-visible changes in
that release. Keep English first and put the Simplified Chinese translation
immediately below the matching English text.
-->

## Highlights / 更新亮点

- Usque v0.2.3 is a major bug-fix release focused on eliminating traffic stalls and premature packet loss when sustained traffic fills MASQUE or platform packet queues.
  <br>Usque v0.2.3 是一个重大 Bug 修复版本，重点解决持续流量填满 MASQUE 或平台数据包队列时出现的流量停滞和过早丢包问题。
- H3 and H2 now retain bounded packet batches and apply backpressure until capacity returns instead of dropping or failing sends while the transport is still healthy. Closing or cancelling a path also releases blocked senders cleanly.
  <br>H3 与 H2 现在会保留有界数据包批次，并持续施加背压直至容量恢复，而不会在传输仍然健康时丢弃数据包或令发送失败；关闭或取消路径时也会干净地释放被阻塞的发送方。
- Windows packet-ring publication and Android tunnel scheduling now process bounded, ordered batches and resume in order after congestion, improving sustained traffic handling without introducing unbounded work.
  <br>Windows 数据包环发布和 Android 隧道调度现在会处理有界、有序的批次，并在拥塞后按顺序恢复，从而改善持续流量处理且不会引入无界工作量。
- Drop accounting and transport telemetry now distinguish real bounded-queue overflow from recoverable queue pressure. Pinned Rust dependencies and CI, CodeQL, Java, and SBOM tooling have also been refreshed.
  <br>丢包计数与传输遥测现在能够区分真实的有界队列溢出和可恢复的队列压力；固定的 Rust 依赖以及 CI、CodeQL、Java 与 SBOM 工具也已更新。

### DNS privacy / DNS 隐私

GeoSite-matched DNS queries use the current physical network's DNS and may be visible to that network. Apps that use their own DoH or DoT hide the domain from Usque, so those connections fall back to GeoIP routing.

与 GeoSite 匹配的 DNS 查询会使用当前物理网络的 DNS，因此该网络可能看到这些查询。应用自行使用 DoH 或 DoT 时，Usque 无法获知域名，相应连接会回退到 GeoIP 路由。

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
