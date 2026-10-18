<!--
Rewrite the summary, Highlights and Before upgrading for each release.
Keep it short: one line per item, English first, then Simplified Chinese.
Rendering is not approval.
-->

## Usque {{release_tag}} official release / Usque {{release_tag}} 正式版发布

Usque {{release_tag}} adds Chrome 155 TLS fingerprints and built-in IPv6 endpoint presets, applies Ads rule updates without reconnecting, and makes connections recover more reliably.

Usque {{release_tag}} 新增 Chrome 155 TLS 指纹和内置 IPv6 端点预设，Ads 规则更新无需重连即可生效，连接恢复也更加可靠。

## Highlights / 更新亮点 ✨

- **Chrome 155 TLS fingerprint** — The new default for H2, H3 and L4. The original handshake is still available as **unsafe**.
  <br>**Chrome 155 TLS 指纹** — H2、H3 和 L4 的新默认值，原握手方式仍可选，现标记为 **unsafe**。

- **IPv6 endpoint presets** — Pick United States, Germany or Finland; addresses match your account type.
  <br>**IPv6 端点预设** — 可选美国、德国或芬兰，地址自动匹配账号类型。

- **Live Ads updates** — Rules download, verify and apply to new requests without reconnecting.
  <br>**Ads 在线更新** — 规则自动下载并校验，无需重连即可用于新请求。

- **More reliable recovery** — Registration fallback, steadier WARP DNS and DoT recovery, and DIRECT traffic no longer slips into WARP when your network lacks IPv4 or IPv6.
  <br>**恢复更可靠** — 注册增加备用请求，WARP DNS 与 DoT 恢复更稳定；当前网络缺少 IPv4 或 IPv6 时，DIRECT 流量不再误走 WARP。

- **Android and Windows** — Android asks for VPN permission only when a connection needs it. Windows 10 2004 (build 19041) is now supported.
  <br>**Android 与 Windows** — Android 仅在连接需要时才申请 VPN 权限；现已支持 Windows 10 2004（内部版本 19041）。

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

- **No downgrade after upgrading** — Settings migrate to schema 26, which v0.3.1 and older cannot read. Back up first if you may need to go back.
  <br>**升级后无法降级** — 配置会迁移到 schema 26，v0.3.1 及更早版本无法读取。如可能需要回退，请先备份。

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
