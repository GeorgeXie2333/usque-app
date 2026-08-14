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

Usque 是面向个人版 Cloudflare WARP（Consumer WARP）的非官方图形客户端。界面用 Flutter，传输、CONNECT-IP、DNS、代理和连接状态由 Rust 引擎处理，不用 WebView。

> [!IMPORTANT]
> 当前版本是 **v0.1.2**。只有 [`v0.1.2` GitHub Release](https://github.com/GeorgeXie2333/usque-app/releases/tag/v0.1.2) 上的附件，并且校验和、签名指纹对得上，才算正式包。Pull Request 里的构建、本地构建和没打标签的二进制都不算。

Usque 是独立项目，与 Cloudflare 没有隶属、赞助或背书关系。Cloudflare 和 WARP 是 Cloudflare, Inc. 的商标。使用个人版 WARP 仍须遵守 Cloudflare 的条款和隐私政策。

## 发布范围

`main` 上的 `v0.1.2` 标签会构建并检查下面六个安装包：

| 平台 | 安装包 | 最低系统 | 架构 |
| --- | --- | --- | --- |
| Windows | MSI | Windows 10 22H2，Build 19045 | x64-v2 |
| Windows | MSI | Windows 10 22H2，Build 19045 | ARM64 |
| Android / Android TV | 分 ABI 安装包 | Android 8.0，API 26 | ARMv8（`arm64-v8a`） |
| Android / Android TV | 分 ABI 安装包 | Android 8.0，API 26 | x64（`x86_64`） |
| Android / Android TV | 分 ABI 安装包 | Android 8.0，API 26 | ARMv7（`armeabi-v7a`） |
| Android / Android TV | 通用安装包 | Android 8.0，API 26 | 上述三种 Android ABI |

仓库里有 macOS 源码，但当前不构建、不发布。这个版本也没有 iOS、Zero Trust、应用商店、公开命令行，或把多条路径的带宽加在一起。

## 主要功能

- 注册个人版 WARP、用 License Key 注册，以及导入/导出 WARP Secret。
- VPN、SOCKS5、HTTP 代理和 Windows 系统代理可以一起开，共用一条 MASQUE 通道。
- 先走 HTTP/3（QUIC），不行再回退 HTTP/2（TLS）；入口用 IPv4/IPv6 Happy Eyeballs。
- 全隧道 VPN、隧道内 DNS、Kill Switch、局域网访问和自定义 CIDR 绕过。
- SOCKS5 的 TCP/UDP 和 HTTP 的 CONNECT/普通转发；代理默认只听本机回环。
- 多个配置档，同时只有一个在用，身份按配置档分开存。
- Android 快捷设置磁贴、桌面快捷方式、开机恢复，以及电视遥控器操作。
- Windows 托盘、单实例唤醒、开机启动、关闭后进托盘。
- 诊断只留在本地并做脱敏。没有用量统计，也不会自动上传。

选 IPv4 还是 IPv6 的 MASQUE 端点，只决定物理入口。两条入口都能在 CONNECT-IP 里带 IPv4 和 IPv6。Usque 同一时间只保一条传输，不会把多条路径的带宽加起来。

## 默认网络设置

| 设置 | 默认值 |
| --- | --- |
| 端点 IPv4 | `162.159.198.2` |
| 端点 IPv6 | `2606:4700:103::2` |
| 端口 | `443` |
| SNI | `speed.cloudflare.com` |
| 传输 | 自动：先 HTTP/3，再 HTTP/2 |
| MTU | `1280` |
| 备用 DNS | `1.1.1.1`、`2606:4700:4700::1111` |
| SOCKS5 | `127.0.0.1:1080`、`[::1]:1080` |
| HTTP 代理 | `127.0.0.1:8080`、`[::1]:8080` |

这些值可以改，也可以一键改回默认。代理如果监听非回环地址，不会设账号密码，界面会一直显示警告。

## 获取与安装

请只从[正式发布页](https://github.com/GeorgeXie2333/usque-app/releases/tag/v0.1.2)下载 `v0.1.2`：

| 目标 | 文件 |
| --- | --- |
| Windows x64 | [`usque-v0.1.2-windows-x64-v2.msi`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-windows-x64-v2.msi) |
| Windows ARM64 | [`usque-v0.1.2-windows-arm64.msi`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-windows-arm64.msi) |
| Android ARMv8 | [`usque-v0.1.2-android-arm64-v8a.apk`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-android-arm64-v8a.apk) |
| Android x64 | [`usque-v0.1.2-android-x86_64.apk`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-android-x86_64.apk) |
| Android ARMv7 | [`usque-v0.1.2-android-armeabi-v7a.apk`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-android-armeabi-v7a.apk) |
| Android 通用包 | [`usque-v0.1.2-android-universal.apk`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-android-universal.apk) |

尽量选和设备 ABI 一致的 APK。通用包装了 ARMv8、x64 和 ARMv7 的原生库，体积更大，只在搞不清架构时用。发布页还有 `SHA256SUMS`、每个包对应的 `.sha256`、SPDX/CycloneDX SBOM、许可证清单和构建证明。

- 1.0 之前的 Windows 包是固定自签名。点掉系统警告前，先对一下公布的 SHA-256 和证书指纹。
- 1.0 之前的 Android 包也是项目自己的自签名证书，不在 Google Play 上，可能要手动安装或用 ADB。
- 以后换到 v1.0.0 签名会单独发一版。
- 发布流水线会编译、签名，并检查架构、校验和、SBOM 和构建来源。它不会在真机上装包，也不会跑长时间 VPN。
- Usque 不会自己装更新。可选的更新检查只打开发布页。
- Windows 卸载会在系统应用页先确认，再清掉 Usque 改过的网络状态；如果勾选，也可以删掉当前用户的本地数据。

核对安装包、升级、卸载和恢复见[安装说明（英文）](docs/INSTALLATION.md)。

## 可同时开的出口

一个配置档可以同时开多种出口，它们共用一条已固定端点的 MASQUE 通道和包复用。

| 出口 | 做什么 |
| --- | --- |
| VPN/TUN | 建系统隧道，管路由、DNS 和 Kill Switch。 |
| SOCKS5 | TCP 和 UDP，默认远程解析 DNS。 |
| HTTP 代理 | CONNECT 和普通 HTTP 转发。 |
| Windows 系统代理 | 依赖 HTTP 出口，把系统代理指到本地监听。 |

Windows 默认开 VPN（TUN）、SOCKS5 和 HTTP，系统代理默认关。Android 默认开 VPN、SOCKS5 和 HTTP。也可以全关，只留传输。

## 安全与隐私

- 端点固定不能关，界面里没有「不安全 TLS」开关。
- Secret、私钥、Token、设备 ID、License 和端点固定信息放在 Windows 凭据管理器或 Android Keystore。
- 导出 Secret 必须确认，并且只写到你选的位置。
- Windows 引擎没有特权；只有一个很小的 Agent 管 TUN、路由、DNS、防火墙和系统代理。
- Android 用 `VpnService`，VPN 跑在单独的 `:vpn` 进程里。
- 日志默认 INFO，最多留 7 天或 20 MiB。

报漏洞前先看 [SECURITY.md](SECURITY.md)（英文）。不要在公开 Issue 里贴凭据或未脱敏的诊断包。

## 构建与贡献

工具链钉在 Rust `1.97.1`、Flutter `3.44.7`、Android NDK `29.0.14206865` 以及仓库里的打包工具。环境、检查命令和 Pull Request 要求见 [CONTRIBUTING.md](CONTRIBUTING.md)（英文）。

进度见[实现进度（英文）](docs/IMPLEMENTATION.md)，签名和发布流程见[发布说明（英文）](docs/RELEASE.md)。

## 上游与许可

协议和行为参考归档的 Go 客户端 [Diniboy1123/usque](https://github.com/Diniboy1123/usque)，源码在本仓库的 `oracle/go`。Flutter 界面和 Rust 引擎是新写的。上游版权仍写在许可证里。

源码使用 [MIT License](LICENSE.md)，第三方组件保留各自的许可证。
