<p align="center">
  <img src="assets/branding/usque-readme-banner.png" alt="Usque — 兼容 Cloudflare WARP 的非官方客户端" width="100%">
</p>

<p align="center">
  <a href="README.md">English</a>
</p>

<p align="center">
  <a href="https://github.com/GeorgeXie2333/usque-app/actions/workflows/pr-check.yml"><img alt="PR Check" src="https://github.com/GeorgeXie2333/usque-app/actions/workflows/pr-check.yml/badge.svg"></a>
  <a href="https://github.com/GeorgeXie2333/usque-app/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/GeorgeXie2333/usque-app/actions/workflows/ci.yml/badge.svg?branch=main"></a>
  <a href="https://github.com/GeorgeXie2333/usque-app/actions/workflows/build.yml"><img alt="Build" src="https://github.com/GeorgeXie2333/usque-app/actions/workflows/build.yml/badge.svg"></a>
  <a href="LICENSE.md"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-F48120.svg"></a>
  <img alt="Windows 10 22H2 或更高版本" src="https://img.shields.io/badge/Windows-10%2022H2%2B-2F2F2F.svg">
  <img alt="Android 8 或更高版本" src="https://img.shields.io/badge/Android-8.0%2B-2F2F2F.svg">
</p>

# Usque

Usque 是面向 Consumer Cloudflare WARP 的开源原生 GUI 客户端。Flutter 负责界面，内存安全的 Rust Engine 负责 MASQUE、CONNECT-IP、DNS、代理与连接状态；项目不使用 WebView。

> [!IMPORTANT]
> 当前发布版本为 **v0.1.2**。只有受保护的 [`v0.1.2` GitHub Release](https://github.com/GeorgeXie2333/usque-app/releases/tag/v0.1.2) 所附文件，并且校验和与签名者指纹一致时，才属于官方产物。Pull Request 产物、本地构建和未打标签的二进制均为开发输出。

Usque 是独立项目，与 Cloudflare 无隶属、赞助或背书关系。Cloudflare 与 WARP 是 Cloudflare, Inc. 的商标。使用 Consumer WARP 仍须遵守 Cloudflare 适用的条款与隐私政策。

## 发布目标

受保护的 `v0.1.2` 工作流从同一个 `main` 精确提交构建并验证以下六个可安装产物。

| 平台 | 产物 | 最低系统 | 架构 / 变体 |
| --- | --- | --- | --- |
| Windows | MSI | Windows 10 22H2，Build 19045 | x86-64-v2 |
| Windows | MSI | Windows 10 22H2，Build 19045 | ARM64 |
| Android / Android TV | 拆分 APK | Android 8.0，API 26 | ARMv8（`arm64-v8a`） |
| Android / Android TV | 拆分 APK | Android 8.0，API 26 | x64（`x86_64`） |
| Android / Android TV | 拆分 APK | Android 8.0，API 26 | ARMv7（`armeabi-v7a`） |
| Android / Android TV | Universal APK | Android 8.0，API 26 | 包含上述三种 Android ABI |

macOS 保留为后续源码目标，但不参与当前构建或发布门禁。当前版本不支持 iOS、Zero Trust、应用商店、公开 CLI 或多路径带宽聚合。

## 主要功能

- 注册 Consumer WARP、使用 WARP License Key 注册，以及导入/导出现有 WARP Secret。
- VPN、SOCKS5、HTTP Proxy 与 Windows 系统代理可组合输出，共享一条 MASQUE 通道。
- HTTP/3 over QUIC，失败后回退到 HTTP/2 over TLS，并使用 IPv4/IPv6 Happy Eyeballs 选择入口。
- 全隧道 VPN、隧道内 DNS、Kill Switch、局域网访问和自定义 CIDR 绕过。
- SOCKS5 TCP/UDP 与 HTTP CONNECT/Forward；代理默认只监听 Loopback。
- 多 Profile、单一 Active Profile，并为各 Profile 隔离保存身份。
- Android 快捷设置磁贴、启动器快捷操作、开机恢复和 TV 导航。
- Windows 系统托盘、单实例唤醒、开机启动和关闭到托盘。
- 本地脱敏诊断，无分析、自动遥测或自动上传。

选择 IPv4 或 IPv6 MASQUE Endpoint 只会改变物理入口。任一入口均可在 CONNECT-IP 内承载 IPv4 与 IPv6 数据；Usque 始终只保留一条活动传输，不做带宽叠加。

## 默认网络设置

| 设置 | 默认值 |
| --- | --- |
| Endpoint IPv4 | `162.159.198.2` |
| Endpoint IPv6 | `2606:4700:103::2` |
| 端口 | `443` |
| SNI | `speed.cloudflare.com` |
| 传输 | Auto：HTTP/3，然后 HTTP/2 |
| MTU | `1280` |
| 备用 DNS | `1.1.1.1`、`2606:4700:4700::1111` |
| SOCKS5 | `127.0.0.1:1080`、`[::1]:1080` |
| HTTP Proxy | `127.0.0.1:8080`、`[::1]:8080` |

高级用户可以修改这些值并一键恢复。非 Loopback 代理监听刻意不提供账号密码认证，并始终显示醒目的安全警告。

## 获取与安装

请只从[官方发布页面](https://github.com/GeorgeXie2333/usque-app/releases/tag/v0.1.2)下载 `v0.1.2`：

| 目标 | 文件 |
| --- | --- |
| Windows x64 | [`usque-v0.1.2-windows-x64-v2.msi`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-windows-x64-v2.msi) |
| Windows ARM64 | [`usque-v0.1.2-windows-arm64.msi`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-windows-arm64.msi) |
| Android ARMv8 | [`usque-v0.1.2-android-arm64-v8a.apk`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-android-arm64-v8a.apk) |
| Android x64 | [`usque-v0.1.2-android-x86_64.apk`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-android-x86_64.apk) |
| Android ARMv7 | [`usque-v0.1.2-android-armeabi-v7a.apk`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-android-armeabi-v7a.apk) |
| Android Universal | [`usque-v0.1.2-android-universal.apk`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-android-universal.apk) |

优先下载与设备 ABI 匹配的 APK。Universal APK 同时包含 ARMv8、x64 和 ARMv7 原生库，仅建议在无法确定设备架构时使用，文件也会比拆分 APK 更大。发布页同时提供 `SHA256SUMS`、每个安装包对应的 `.sha256` 文件、SPDX/CycloneDX SBOM、许可证清单和构建证明。

- 1.0 之前的 Windows 安装包使用固定自签名身份。接受系统警告前，请核对发布的 SHA-256 与证书指纹。
- 1.0 之前的 Android 安装包使用项目自行管理的固定自签名 Release 证书，不通过 Google Play 分发，可能需要高级侧载流程或 ADB。
- v1.0.0 的签名迁移将作为独立变更进行兼容性审查。
- 受保护工作流执行 CI、架构、签名、安装包、校验和、SBOM 与构建来源检查，但不声称已完成硬件实验室、物理设备或长时间 VPN 验证。
- Usque 不会自动安装更新；可选的更新检查只会打开发布页面。
- Windows 卸载会先恢复 Usque 管理的网络状态，并可选择删除当前用户的本地数据。

完整的产物验证、平台警告、升级、卸载和恢复边界见[安装与卸载](docs/INSTALLATION.md)。

## 可组合输出

一个 Profile 可以同时启用多种输出；它们共享一条严格 Pin 的 MASQUE 传输和 Packet Multiplexer。

| 输出 | 行为 |
| --- | --- |
| VPN/TUN | 创建系统隧道并管理路由、DNS 和 Kill Switch。 |
| SOCKS5 | 支持 TCP 与 UDP，默认远程解析 DNS。 |
| HTTP Proxy | 支持 CONNECT 和普通 HTTP 转发。 |
| Windows 系统代理 | 依赖 HTTP 输出，将 Windows 指向本地监听器。 |

Windows 默认启用 SOCKS5、HTTP 和 VPN（TUN），默认关闭系统代理；Android 默认启用 VPN、SOCKS5 和 HTTP。允许关闭全部输出，只保持传输可用。

## 安全与隐私

- Endpoint Pin 必须启用，GUI 不提供不安全 TLS 模式。
- Secret、私钥、Token、设备 ID、License 与 Endpoint Pin 保存在 Windows Credential Manager 或 Android Keystore。
- Secret 导出必须明确确认，并只写入用户选择的目标。
- Windows Engine 不持有特权；最小权限 Agent 只管理 TUN、路由、DNS、防火墙和系统代理。
- Android 使用 `VpnService` 和独立 `:vpn` 进程。
- 日志默认为 INFO，最多保留 7 天或 20 MiB。

报告漏洞前请阅读 [SECURITY.md](SECURITY.md)。不得在公开 Issue 中提交凭据或未脱敏诊断包。

## 构建与贡献

项目固定使用 Rust `1.97.1`、Flutter commit `84fc5cbb223bc12f83d65b647ff8a56caf779ffd`、Android NDK `29.0.14206865` 和锁定的打包工具。开发环境、检查命令、安全边界及 Pull Request 要求见 [CONTRIBUTING.md](CONTRIBUTING.md)。

架构和进度见[实施路线](docs/IMPLEMENTATION.md)，受保护的签名及供应链检查见[发布流程](docs/RELEASE.md)。

## 上游与许可

Usque GUI fork 自 [Diniboy1123/usque](https://github.com/Diniboy1123/usque)，并保留上游版权和归属。

源码采用 [MIT License](LICENSE.md)，第三方组件保留各自许可证。
