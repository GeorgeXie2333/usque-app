<!--
Rewrite the summary, Highlights and Before upgrading for each release.
Keep it short: one line per item, English first, then Simplified Chinese.
Rendering is not approval.
-->

## Usque {{release_tag}} pre-release / Usque {{release_tag}} 预发布

Usque {{release_tag}} introduces endpoint subscriptions, gives Android users more control over which apps use the VPN, and makes larger routing lists easier to manage.

Usque {{release_tag}} 新增端点订阅，让 Android 用户更灵活地选择哪些应用使用 VPN，也让较长的分流规则列表更容易管理。

## Highlights / 更新亮点 ✨

- **Endpoint subscriptions** — Import a list for your WARP account type, save favorites, check connection latency, and switch endpoints from Home.
  <br>**端点订阅** — 按 WARP 账号类型导入列表，收藏常用端点、查看连接延迟，并在首页快速切换。

- **Per-app VPN on Android** — Send only selected apps through the VPN, or let selected apps bypass it. Each mode remembers its own list.
  <br>**Android 按应用设置 VPN** — 可以只让选中的应用使用 VPN，也可以让选中的应用直连。两种模式各自记住应用列表。

- **More room for routing rules** — Add up to 4,096 custom rules per type and search longer lists to find the rules you want to change.
  <br>**更多自定义分流规则** — 每类最多可添加 4,096 条自定义规则，并可在较长的列表中搜索要修改的条目。

- **Connection recovery** — Improved reconnection after interruptions and when changing chain exits.
  <br>**连接恢复** — 改进连接中断及切换链式出口后的重新连接体验。

## Download / 下载 📥

| Platform / 平台 | Download / 下载 |
| :---: | :---: |
| ![Windows](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/windows.svg?raw=true)<br>**Windows 10 2004+ (build 19041)** | [![EXE x64-v2](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/windows-x64-v2.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-windows-x64-v2.exe) [![EXE ARM64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/windows-arm64.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-windows-arm64.exe) |
| ![Apple](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/apple.svg?raw=true)<br>**macOS 13+** | [![DMG Apple Silicon (arm64)](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/macos-arm64.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-macos-arm64.dmg) [![DMG Intel (x86_64)](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/macos-x86_64.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-macos-x86_64.dmg) |
| ![Linux](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/linux.svg?raw=true)<br>**Linux**<br><sub>Ubuntu 22.04+ · Debian 12+ · Fedora · systemd</sub> | [![AppImage amd64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/linux-amd64-appimage.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-amd64.AppImage) [![deb amd64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/linux-amd64-deb.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-amd64.deb) [![rpm amd64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/linux-amd64-rpm.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-amd64.rpm)<br>[![AppImage arm64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/linux-arm64-appimage.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-arm64.AppImage) [![deb arm64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/linux-arm64-deb.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-arm64.deb) [![rpm arm64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/linux-arm64-rpm.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-arm64.rpm) |
| ![Android](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android.svg?raw=true)<br>**Android 8.0+ (API 26)**<br><sub>Android TV supported / 支持 Android TV</sub> | [![APK ARMv8 (arm64-v8a)](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-arm64-v8a.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-arm64-v8a.apk) [![APK x64 (x86_64)](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-x86_64.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-x86_64.apk)<br>[![APK ARMv7 (armeabi-v7a)](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-armeabi-v7a.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-armeabi-v7a.apk) [![APK Universal](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-universal.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-universal.apk) |

> [!TIP]
> Download packages only from this release. Not sure which APK? Use **Universal**. On Windows, install the EXE; the MSI files are only for in-app updates.
>
> 请仅从此 Release 下载。不确定选哪个 APK 时请用 **Universal**。Windows 请安装 EXE，MSI 文件仅供应用内更新。

Install this beta manually and back up your settings before upgrading.

此 Beta 版需要手动安装。升级前请先备份设置。

macOS provides local proxy mode in this release. System-wide VPN is not available. The app uses a project signature and is not Apple-notarized, so macOS may ask you to allow it in System Settings.

此版本的 macOS 版提供本地代理模式，暂不支持系统级 VPN。应用使用项目签名，尚未经过 Apple 公证；首次打开时，macOS 可能要求你在系统设置中允许。

<details>
<summary>Usage and privacy / 使用与隐私</summary>

- **Returning to v0.3.5** — Restore your backup if you go back to v0.3.5. That version cannot open settings saved by this beta. [Upgrade guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md#configuration-compatibility-when-upgrading)
  <br>**返回 v0.3.5** — 如需返回 v0.3.5，请恢复备份。v0.3.5 无法直接读取此 Beta 保存的设置。[升级指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md#configuration-compatibility-when-upgrading)

- **Scenes** — Open **Profiles** to create or use a scene and review its changes before switching. [Scenes guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/SCENARIOS.md)
  <br>**场景** — 打开“配置”创建或使用场景，切换前可查看变化。[场景指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/SCENARIOS.md)

- **Proton VPN** — Choose it in **Proxy → Chain proxy**. On macOS, import a Proton configuration if sign-in requires browser verification. [Proton VPN guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/PROTON_VPN.md)
  <br>**Proton VPN** — 在“代理 → 链式代理”中选择；在 macOS 上，如果登录需要网页验证，请导入 Proton 配置文件。[Proton VPN 指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/PROTON_VPN.md)

- **Server changes** — Choose a server while connected, then select **Apply and reconnect** to use it.
  <br>**更换节点** — 连接期间可选择节点，然后点击“应用并重新连接”以使用新节点。

- **Your network choices** — Proton account setup uses your current network; direct rules follow your saved settings. [Connection and privacy guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/CHAIN_PROXY.md)
  <br>**网络选择** — Proton 账号设置使用当前网络；直连规则遵循你保存的设置。[连接与隐私指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/CHAIN_PROXY.md)

- **Android blocking** — When **Block connections without VPN** is on, apps excluded from the VPN are blocked from going online.
  <br>**Android 断网保护** — 开启“屏蔽未使用 VPN 的所有连接”后，排除在 VPN 之外的应用将无法联网。

</details>

## Verify before installing / 安装前验证 🔐

Compare the file's SHA-256 with [SHA256SUMS](https://github.com/{{repository}}/releases/download/{{release_tag}}/SHA256SUMS) and check the signer below. Stop if anything differs. [Step-by-step guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md#verify-before-installing)

请将文件 SHA-256 与 [SHA256SUMS](https://github.com/{{repository}}/releases/download/{{release_tag}}/SHA256SUMS) 比对并核对下方签名，任何不一致都请停止安装。[详细步骤](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md#verify-before-installing)

| Signer / 签名 | SHA-256 / key |
| --- | --- |
| Windows Authenticode | `{{windows_signer_sha256}}` |
| macOS self-signed certificate / macOS 自签名证书 | `{{macos_signer_sha256}}` |
| Linux Ed25519 public key | `{{linux_ed25519_public_key}}` |
| Android release certificate | `{{android_signer_sha256}}` |

Mac DMGs carry the project signature and GitHub build provenance. Apple notarization is unavailable with this identity. / macOS DMG 使用项目自签名和 GitHub 构建来源证明，此证书无法取得 Apple 公证。[Verify the DMG / 验证 DMG](https://github.com/{{repository}}/blob/{{release_tag}}/docs/CODE_SIGNING.md#configure-and-verify-macos-release-signing)

Pre-1.0 packages are self-signed, so Windows may show an unknown-publisher warning. Linux packages are verified with the `.manifest.json` and `.sig` files ([Linux guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/LINUX_DESKTOP.md)). Also attached: [release manifest](https://github.com/{{repository}}/releases/download/{{release_tag}}/release-manifest.json) and per-package SPDX SBOMs.

1.0 之前的安装包为自签名，Windows 可能提示“未知发布者”。Linux 安装包通过 `.manifest.json` 和 `.sig` 校验（[Linux 指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/LINUX_DESKTOP.md)）。另附[发布清单](https://github.com/{{repository}}/releases/download/{{release_tag}}/release-manifest.json)及逐包 SPDX SBOM。

## Feedback / 问题反馈 💬

[Bug report / 错误反馈](https://github.com/{{repository}}/issues/new?template=bug.yml) · [Feature request / 功能建议](https://github.com/{{repository}}/issues/new?template=feature.yml) · [Security issue (private) / 安全问题（私密）](https://github.com/{{repository}}/security/advisories/new)

Remove credentials, tokens and personal addresses from logs before sharing. / 分享日志前请删除凭据、令牌和个人地址。

---

Cloudflare and WARP are trademarks and/or registered trademarks of Cloudflare, Inc. in the United States and other jurisdictions. / Cloudflare 和 WARP 是 Cloudflare, Inc. 在美国及其他司法管辖区的商标和/或注册商标。
