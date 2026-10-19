<!--
Rewrite the summary, Highlights and Before upgrading for each release.
Keep it short: one line per item, English first, then Simplified Chinese.
Rendering is not approval.
-->

## Usque {{release_tag}} pre-release / Usque {{release_tag}} 预发布

Usque {{release_tag}} is a pre-release introducing Linux desktop packages and experimental native Proton VPN chain exits, with improvements to chain-proxy setup, settings recovery and packaging.

Usque {{release_tag}} 是一个预发布版本，首次提供 Linux 桌面安装包和实验性原生 Proton VPN 链式出口，并改进链式代理设置、配置恢复与打包。

## Highlights / 更新亮点 ✨

- **Linux desktop preview** — Native amd64 and arm64 AppImage, deb and rpm packages, signed with the fixed project Ed25519 identity.
  <br>**Linux 桌面预览** — 提供原生 amd64 与 arm64 AppImage、deb、rpm 安装包，使用固定项目 Ed25519 身份签名。

- **Experimental Proton VPN exits** — Prepare a guest or account, select a server and connect after WARP. Complete any official human verification yourself; native Android and complete live-tunnel acceptance remain unverified.
  <br>**实验性 Proton VPN 出口** — 准备游客或账号、选择节点，经 WARP 串联连接。官方人机验证需本人完成；原生 Android 和完整实网隧道验收仍未完成。

- **Simpler chain setup** — A unified editor, full eligible Proton catalogues, reusable certificates and clearer authorization errors.
  <br>**简化链式配置** — 统一编辑器、完整可用 Proton 节点目录、证书复用与更清晰的授权错误。

- **Settings and packaging recovery** — Recover unconfirmed saves, isolate automatic-endpoint pin failures, and group Android version codes by build number.
  <br>**设置与打包恢复** — 恢复未确认的保存操作、隔离自动端点公钥校验失败，并按构建号分组 Android 版本代码。

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

## Before upgrading / 升级须知

- **Beta: install manually and back up first** — This is not the stable Latest release and is not offered to stable clients. It uses the same application identity, not a parallel installation. Do not downgrade migrated data to v0.3.2 or older.
  <br>**Beta：手动安装并先备份** — 本版不是稳定 Latest，不会推送给稳定版客户端；使用相同应用身份，不是并行安装。请勿将已迁移的数据降级给 v0.3.2 或更早版本。

- **Defaults stay off** — Ads, chain proxy and encrypted WARP DNS stay off until you enable them; your saved choices are kept.
  <br>**默认保持关闭** — Ads、链式代理和加密 WARP DNS 需手动开启；已保存的设置会保留。

- **Android blocking** — To keep apps blocked when the VPN stops, enable **Always-on VPN** and **Block connections without VPN** in Android settings.
  <br>**Android 断网保护** — 若要在 VPN 停止后继续阻止应用联网，请在系统设置中开启“始终开启的 VPN”和“阻止未使用 VPN 的连接”。

<details>
<summary>Technical details / 技术细节</summary>

- Configuration schema 26, Agent protocol 3, recovery journal 6. All IPC additions are append-only, and diagnostic exports keep their privacy filtering.
  <br>配置 schema 26、Agent 协议 3、恢复日志 6。IPC 改动均为追加，诊断导出继续执行隐私过滤。

- IPv6 presets look up their endpoint hostname through your network's DNS before the tunnel starts. Your DNS provider can see that lookup.
  <br>IPv6 预设在隧道建立前通过当前网络的 DNS 查询端点域名，DNS 服务商可以看到这次查询。

- Chrome 155 is a fixed TLS profile, not full browser emulation. ECH GREASE is not real ECH, and authentication and endpoint pins are always enforced.
  <br>Chrome 155 是固定 TLS 配置，并非完整模拟浏览器；ECH GREASE 不是真正的 ECH，认证与端点公钥校验始终生效。

- Proton account management and its verification window use the ordinary system HTTPS/DNS path; connection and renewal APIs use the established WARP session, while the private agent stays inside Proton. No automatic telemetry or diagnostic upload is added.
  <br>Proton 账号管理与验证窗口使用系统普通 HTTPS/DNS 路径；连接与续期 API 使用既有 WARP 会话，私有 agent 留在 Proton 内部。不新增自动遥测或诊断上传。

- Chain exits connect through the WARP tunnel. If the exit fails, traffic stops instead of falling back to WARP only. UDP-based exits need non-L4 mode.
  <br>链式出口经 WARP 隧道连接；出口失败时流量停止，不会退回仅 WARP。基于 UDP 的出口需要非 L4 模式。

- On Windows, the Usque adapter stays after disconnect for reuse and is removed when you fully exit the app.
  <br>Windows 断开后会保留 Usque 虚拟网卡以便复用，完全退出应用时移除。

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
