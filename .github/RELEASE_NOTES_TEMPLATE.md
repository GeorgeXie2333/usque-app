<!--
Rewrite the summary, Highlights and Before upgrading for each release.
Keep it short: one line per item, English first, then Simplified Chinese.
Rendering is not approval.
-->

## Usque {{release_tag}} official release / Usque {{release_tag}} 正式版发布

Usque {{release_tag}} brings a Linux desktop app, adds optional Proton VPN support, and makes everyday connections easier to manage.

Usque {{release_tag}} 带来 Linux 桌面版和可选的 Proton VPN 支持，让日常连接更容易管理。

## Highlights / 更新亮点 ✨

- **Linux desktop** — Usque is now available on Linux, with downloads for different devices and popular distributions.
  <br>**Linux 桌面版** — 现在可以在 Linux 上使用 Usque，并为不同设备和常见发行版提供下载。

- **Proton VPN (experimental)** — Use Proton VPN with WARP, sign in or continue as a guest, and choose a server by country or city. You can also select another server while connected.
  <br>**Proton VPN（实验性）** — 在 WARP 连接基础上使用 Proton VPN，支持账号登录或游客模式，并可按国家和城市选择节点；连接期间也能选择其他节点。

- **A clearer home screen** — See your connection, selected exit and traffic together in a layout adapted to your device.
  <br>**更清晰的首页** — 在一处查看连接状态、所选出口和流量，桌面与手机布局都更加清晰。

- **More intuitive network checks** — Network quality and diagnostics now present results and suggested next steps more clearly.
  <br>**更直观的网络检查** — 网络质量和诊断页面更清楚地展示结果与建议。

- **Connection recovery** — Improved recovery after network changes and connection interruptions.
  <br>**连接恢复** — 改进网络变化和连接中断后的恢复体验。

- **Everyday refinements** — Simplified connection settings and more complete translations across 21 languages.
  <br>**日常体验改进** — 简化连接设置，并完善 21 种语言的界面翻译。

## Download / 下载 📥

| Platform / 平台 | Download / 下载 |
| :---: | :---: |
| ![Android](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android.svg?raw=true)<br>**Android 8.0+ (API 26)**<br><sub>Android TV supported / 支持 Android TV</sub> | [![APK ARMv8 (arm64-v8a)](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-arm64-v8a.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-arm64-v8a.apk) [![APK x64 (x86_64)](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-x86_64.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-x86_64.apk)<br>[![APK ARMv7 (armeabi-v7a)](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-armeabi-v7a.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-armeabi-v7a.apk) [![APK Universal](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/android-universal.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-android-universal.apk) |
| ![Windows](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/windows.svg?raw=true)<br>**Windows 10 2004+ (build 19041)** | [![EXE x64-v2](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/windows-x64-v2.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-windows-x64-v2.exe) [![EXE ARM64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/windows-arm64.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-windows-arm64.exe) |
| ![Linux](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/linux.svg?raw=true)<br>**Linux**<br><sub>Ubuntu 22.04+ · Debian 12+ · Fedora · systemd</sub> | [![AppImage amd64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/linux-amd64-appimage.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-amd64.AppImage) [![deb amd64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/linux-amd64-deb.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-amd64.deb) [![rpm amd64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/linux-amd64-rpm.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-amd64.rpm)<br>[![AppImage arm64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/linux-arm64-appimage.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-arm64.AppImage) [![deb arm64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/linux-arm64-deb.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-arm64.deb) [![rpm arm64](https://github.com/{{repository}}/blob/{{release_tag}}/docs/assets/release/linux-arm64-rpm.svg?raw=true)](https://github.com/{{repository}}/releases/download/{{release_tag}}/usque-{{release_tag}}-linux-arm64.rpm) |

> [!TIP]
> Download packages only from this release. Not sure which APK? Use **Universal**. On Windows, install the EXE; the MSI files are only for in-app updates.
>
> 请仅从此 Release 下载。不确定选哪个 APK 时请用 **Universal**。Windows 请安装 EXE，MSI 文件仅供应用内更新。

<details>
<summary>Usage and privacy / 使用与隐私</summary>

- **Proton VPN** — Choose it in **Proxy → Chain proxy** and complete any verification when prompted. [Proton VPN guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/PROTON_VPN.md)
  <br>**Proton VPN** — 在“代理 → 链式代理”中选择，并按提示完成验证。[Proton VPN 指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/PROTON_VPN.md)

- **Server changes** — Choose a server while connected, then select **Apply and reconnect** to use it.
  <br>**更换节点** — 连接期间可选择节点，然后点击“应用并重新连接”以使用新节点。

- **Your network choices** — Proton account setup uses your current network; direct rules follow your saved settings. [Connection and privacy guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/CHAIN_PROXY.md)
  <br>**网络选择** — Proton 账号设置使用当前网络；直连规则遵循你保存的设置。[连接与隐私指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/CHAIN_PROXY.md)

- **Android blocking** — To keep apps blocked when the VPN stops, enable **Always-on VPN** and **Block connections without VPN** in Android settings.
  <br>**Android 断网保护** — 若要在 VPN 停止后继续阻止应用联网，请在系统设置中开启“始终开启的 VPN”和“阻止未使用 VPN 的连接”。

</details>

## Verify before installing / 安装前验证 🔐

Compare the file's SHA-256 with [SHA256SUMS](https://github.com/{{repository}}/releases/download/{{release_tag}}/SHA256SUMS) and check the signer below. Stop if anything differs. [Step-by-step guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md#verify-before-installing)

请将文件 SHA-256 与 [SHA256SUMS](https://github.com/{{repository}}/releases/download/{{release_tag}}/SHA256SUMS) 比对并核对下方签名，任何不一致都请停止安装。[详细步骤](https://github.com/{{repository}}/blob/{{release_tag}}/docs/INSTALLATION.md#verify-before-installing)

| Signer / 签名 | SHA-256 / key |
| --- | --- |
| Windows Authenticode | `{{windows_signer_sha256}}` |
| Android release certificate | `{{android_signer_sha256}}` |
| Linux Ed25519 public key | `{{linux_ed25519_public_key}}` |

Pre-1.0 packages are self-signed, so Windows may show an unknown-publisher warning. Linux packages are verified with the `.manifest.json` and `.sig` files ([Linux guide](https://github.com/{{repository}}/blob/{{release_tag}}/docs/LINUX_DESKTOP.md)). Also attached: [release manifest](https://github.com/{{repository}}/releases/download/{{release_tag}}/release-manifest.json) and per-package SPDX SBOMs.

1.0 之前的安装包为自签名，Windows 可能提示“未知发布者”。Linux 安装包通过 `.manifest.json` 和 `.sig` 校验（[Linux 指南](https://github.com/{{repository}}/blob/{{release_tag}}/docs/LINUX_DESKTOP.md)）。另附[发布清单](https://github.com/{{repository}}/releases/download/{{release_tag}}/release-manifest.json)及逐包 SPDX SBOM。

## Feedback / 问题反馈 💬

[Bug report / 错误反馈](https://github.com/{{repository}}/issues/new?template=bug.yml) · [Feature request / 功能建议](https://github.com/{{repository}}/issues/new?template=feature.yml) · [Security issue (private) / 安全问题（私密）](https://github.com/{{repository}}/security/advisories/new)

Remove credentials, tokens and personal addresses from logs before sharing. / 分享日志前请删除凭据、令牌和个人地址。

---

Cloudflare and WARP are trademarks and/or registered trademarks of Cloudflare, Inc. in the United States and other jurisdictions. / Cloudflare 和 WARP 是 Cloudflare, Inc. 在美国及其他司法管辖区的商标和/或注册商标。
