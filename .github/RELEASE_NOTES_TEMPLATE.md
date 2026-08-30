<!--
Before each release, replace the highlights with the user-visible changes in
that release. Keep English first and put the Simplified Chinese translation
immediately below the matching English text.
-->

## Highlights / 更新亮点

- Verified in-app upgrades are now available on Windows and Android. Usque requires the architecture-matched package, release manifest, size, SHA-256, signer, package identity, and version to agree before handing the update to the platform installer.
  <br>Windows 与 Android 现已支持经过验证的应用内升级。Usque 仅在架构匹配的软件包、Release 清单、大小、SHA-256、签名者、软件包身份和版本全部一致后，才会将更新交给系统安装程序。
- Structured diagnostics now explain tunnel, transport, frontend, and platform failures and can export privacy-sanitized evidence. Official releases add an immutable manifest, checksums, and per-package SBOMs; isolated-runner reports remain optional supplemental validation.
  <br>结构化诊断现可说明隧道、传输层、前端和平台故障，并导出经过隐私清理的证据。正式 Release 还会附带不可变清单、校验和与逐包 SBOM；隔离 Runner 报告作为可选的补充验证。
- H3 and H2 transport paths now reduce packet copies, allocations, logging contention, and nested timers. Windows packet-ring batching and fairer Android tunnel scheduling improve sustained traffic handling.
  <br>H3 与 H2 传输路径现已减少数据包复制、内存分配、日志竞争和嵌套计时器；Windows 数据包环批处理与更公平的 Android 隧道调度可改善持续流量处理。
- Navigation, accessibility, touch behavior, platform-specific VPN labels, and translations have been refined. Zero Trust reauthentication now preserves registration-owned endpoint addresses while sharing device-wide port and SNI settings consistently.
  <br>导航、无障碍、触控行为、平台专用 VPN 标签和翻译均已改进；Zero Trust 重新认证现会保留注册方下发的端点地址，并一致共享设备级端口与 SNI 设置。

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
