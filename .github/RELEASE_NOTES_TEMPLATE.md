<!--
Before each release, replace the highlights with the user-visible changes in
that release. Keep English first and put the Simplified Chinese translation
immediately below the matching English text.
-->

## Highlights / 更新亮点

- Usque v0.2.5 is a feature and reliability release that improves Windows upgrades and recovery, adds Zero Trust onboarding, and refines the Windows and Android experience.
  <br>Usque v0.2.5 是一个功能与可靠性版本，改进 Windows 升级与恢复，新增 Zero Trust 首次配置流程，并优化 Windows 与 Android 使用体验。
- Windows upgrades now install the versioned, recovery-compatible Agent before removing the older product, providing a compatibility bridge from v0.2.4. Complete payload replacement avoids mixing old and new application files, while asynchronous Wintun-removal confirmation and idempotent adapter recovery address cleanup failures.
  <br>Windows 升级现在先安装带版本信息且兼容恢复流程的新版 Agent，再移除旧产品，为 v0.2.4 提供升级兼容桥接。完整载荷替换避免新旧应用文件混用，异步 Wintun 移除确认和幂等适配器恢复则修复清理失败问题。
- Windows adds bounded, operation- and generation-checked automatic recovery, corrects physical DNS discovery, and authorizes bootstrap egress before committing the Kill Switch. Unrestored or conflicting platform state remains a hard stop for a new tunnel.
  <br>Windows 新增次数受限且核对操作与代次的自动恢复，修正物理 DNS 发现，并在提交 Kill Switch 前授权引导出口。平台状态未恢复或存在冲突时仍禁止启动新隧道。
- HTTP/3 receive, cancellation, recovery, and PMTU handling have been hardened. Network Quality retains real source samples across coalesced delivery, with coordinated Windows and Android sampling and corrected Android output status. These are implementation fixes, not measured throughput claims.
  <br>HTTP/3 接收、取消、恢复及 PMTU 处理得到强化。网络质量中心在合并投递时保留真实源样本，协调 Windows 与 Android 的采样，并修正 Android 输出状态。这些是实现修复，不代表已经测得吞吐提升。
- Zero Trust enrollment is available during onboarding. Native Windows and Android layouts now use clearer open sections, improved account and proxy workflows, unsaved-change handling, and Windows window sizing that respects the display work area. Zero Trust remains experimental.
  <br>首次配置流程新增 Zero Trust 注册。Windows 与 Android 原生布局采用更清晰的开放分区，改进账户及代理配置流程和未保存修改处理，并让 Windows 窗口尺寸适应显示器工作区。Zero Trust 仍为实验性功能。
- The Windows installer now selects from 21 localized interfaces in one signed bundle per architecture. Windows tray and uninstall text, plus Android notifications, Quick Settings, and shortcuts, follow the selected UI language.
  <br>Windows 安装程序现在会在每个架构的单一签名安装包中选择 21 种本地化界面。Windows 托盘与卸载文案，以及 Android 通知、快捷设置和快捷方式，也会跟随所选界面语言。
- Dependency, release-tooling, guide, and screenshot updates accompany these changes. Real installation, upgrade, VPN recovery, leak, and performance validation requires isolated environments; compile-only and MSI table checks must not be presented as those runtime results.
  <br>此版本还更新依赖、发布工具、指南和截图。真实安装、升级、VPN 恢复、泄漏及性能验证需要隔离环境；编译与 MSI 表检查不得被表述为这些运行时验证结果。

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
