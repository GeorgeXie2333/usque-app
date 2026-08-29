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
  <img alt="Rust 1.97.1" src="https://img.shields.io/badge/Rust-1.97.1-dea584.svg?logo=rust&logoColor=white">
  <img alt="Flutter 3.44.7" src="https://img.shields.io/badge/Flutter-3.44.7-02569B.svg?logo=flutter&logoColor=white">
  <img alt="Windows 10 22H2 或更高版本" src="https://img.shields.io/badge/Windows-10%2022H2%2B-2F2F2F.svg">
  <img alt="Android 8 或更高版本" src="https://img.shields.io/badge/Android-8.0%2B-2F2F2F.svg">
</p>

# Usque

Usque 是面向 Cloudflare WARP 个人版（Consumer WARP）的非官方图形客户端。界面由 Flutter 实现；MASQUE、CONNECT-IP、DNS、代理与连接状态由 Rust 引擎处理。项目不使用 WebView。

> [!IMPORTANT]
> 当前发布版本为 **v0.2.1**。请仅从 [GitHub Releases 页面](https://github.com/GeorgeXie2333/usque-app/releases) 下载正式安装包。Pull Request 构建、本地构建以及未打标签的二进制均非正式发布。

Usque 为独立项目，与 Cloudflare 无隶属、赞助或背书关系。Cloudflare 与 WARP 是 Cloudflare, Inc. 的商标。使用个人版 WARP 仍须遵守 Cloudflare 的适用条款与隐私政策。

## 界面展示

<table>
  <tr>
    <td align="center" valign="top">
      <p><strong>Windows</strong></p>
      <img src="assets/screenshots/usque-windows-home.png" alt="Usque Windows 主界面" width="720">
    </td>
    <td align="center" valign="top">
      <p><strong>Android</strong></p>
      <img src="assets/screenshots/usque-android-home.png" alt="Usque Android 主界面" width="280">
    </td>
  </tr>
</table>

## 发布范围

`v0.2.1` 由 `main` 上的对应标签构建并校验以下六个安装包：

| 平台 | 安装包 | 最低系统 | 架构 |
| --- | --- | --- | --- |
| Windows | MSI | Windows 10 22H2，Build 19045 | x64-v2 |
| Windows | MSI | Windows 10 22H2，Build 19045 | ARM64 |
| Android / Android TV | 分 ABI 安装包 | Android 8.0，API 26 | ARMv8（`arm64-v8a`） |
| Android / Android TV | 分 ABI 安装包 | Android 8.0，API 26 | x64（`x86_64`） |
| Android / Android TV | 分 ABI 安装包 | Android 8.0，API 26 | ARMv7（`armeabi-v7a`） |
| Android / Android TV | 通用安装包 | Android 8.0，API 26 | 上述三种 Android ABI |

仓库保留 macOS 源码，但不参与当前构建与发布。本版本不提供 iOS、生产级 Zero Trust 支持、应用商店分发、公开命令行，也不支持多路径带宽聚合。源码构建包含下述受发布门槛约束的 Zero Trust 组织注册实验功能。

## 主要功能

- 支持个人版 WARP 注册、WARP License Key 注册，以及 WARP Secret 的导入与导出。
- Windows 与 Android 提供实验性的 Cloudflare Zero Trust 设备注册，仅用于以组织身份复用现有 MASQUE 公网隧道。
- VPN、SOCKS5、HTTP 代理与 Windows 系统代理可同时启用，共享同一条 MASQUE 通道。
- 优先使用 HTTP/3（QUIC），失败后回退至 HTTP/2（TLS）；物理入口通过 IPv4/IPv6 Happy Eyeballs 选择。
- 全隧道 VPN、隧道内 DNS、Kill Switch、局域网访问与自定义 CIDR 绕过。
- SOCKS5 支持 TCP/UDP，HTTP 支持 CONNECT 与普通转发；代理默认仅监听回环地址。
- 支持多个配置，同一时间仅一个处于活动状态；身份材料按配置隔离存储。
- Android 分应用代理（仅包含所选应用）：关闭时全部应用走 VPN；开启后仅勾选的应用走隧道。新安装的应用默认不进入隧道，直至勾选。
- Android 快捷设置磁贴、启动器快捷方式、开机恢复与电视端导航。
- Windows 系统托盘、单实例激活、开机启动，以及关闭后最小化到托盘。
- 诊断信息仅保存在本地并经过脱敏，不含统计分析、遥测或自动上传。

选择 IPv4 或 IPv6 MASQUE 端点仅改变物理入口。任一入口均可在 CONNECT-IP 内承载 IPv4 与 IPv6。Usque 同一时间只保持一条活动传输，不聚合多路径带宽。

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

上述默认值可修改，也可一键恢复。非回环代理监听不提供认证，并始终显示安全警告。

## 获取与安装

请从 [GitHub Releases 页面](https://github.com/GeorgeXie2333/usque-app/releases) 下载 Usque。请优先选择与设备 ABI 匹配的 APK。通用安装包同时包含 ARMv8、x64 与 ARMv7 原生库，体积更大，仅建议在无法确定设备架构时使用。GitHub 会为每个资源显示 SHA-256；安装前请核对摘要与已公布的签名者指纹。

- 1.0 之前的 Windows 安装包使用固定自签名身份。接受系统警告前，请核对已公布的 SHA-256 与证书指纹。
- 1.0 之前的 Android 安装包使用项目自行管理的固定自签名证书，不通过 Google Play 分发，可能需要手动安装或使用 ADB。
- v1.0.0 的签名变更将作为独立版本发布。
- 发布流程会编译并签名六个安装包，同时在 GitHub Release 附带 `release-manifest.json`、`SHA256SUMS`、每包 SPDX SBOM、`reliability-report.json` 与 `device-matrix.md`。同一签名候选还必须通过受保护的 Windows 快照虚拟机、Android 专用设备、独立网络观察器和受控性能实验室报告校验；受限抓包与原始实验数据仅保留在 CI 中。
- 启用自动检查后，Usque 会在每次进程启动后检查一次。仅在用户确认后才下载已验证的稳定版本；Windows 将其交给已签名的被动更新器，Android 则打开系统软件包安装器。手动检查始终获取实时发布数据。
- Windows 卸载会在系统设置中要求确认，随后恢复 Usque 修改过的网络状态；用户可选择删除当前用户的本地数据。

安装包校验、升级、卸载与恢复见[安装说明（英文）](docs/INSTALLATION.md)。

## 可组合输出

一个配置可同时启用多种输出。它们共享一条已固定端点的 MASQUE 传输与包复用。

| 输出 | 说明 |
| --- | --- |
| VPN/TUN | 创建系统隧道，并管理路由、DNS 与 Kill Switch。 |
| SOCKS5 | 支持 TCP 与 UDP，默认使用远程 DNS。 |
| HTTP 代理 | 支持 CONNECT 与普通 HTTP 转发。 |
| Windows 系统代理 | 依赖 HTTP 输出，将系统代理指向本地监听地址。 |

Windows 默认启用 VPN（TUN）、SOCKS5 与 HTTP，系统代理默认关闭。Android 默认启用 VPN、SOCKS5 与 HTTP。分应用代理是 Android 应用设置，不属于某个配置：开启后仅勾选的应用走 VPN。允许关闭全部输出，仅保留传输。

## 安全与隐私

- 端点固定为强制策略，界面不提供不安全的 TLS 模式。
- Secret、私钥、令牌、设备标识、许可证与端点固定信息存储于 Windows 凭据管理器或 Android Keystore。
- 导出 Secret 须经确认，且仅写入用户指定的位置。
- Windows 引擎不以特权运行；由最小权限 Agent 管理 TUN、路由、DNS、防火墙与系统代理。
- Android 使用 `VpnService`，并在独立的 `:vpn` 进程中运行。
- 日志级别默认为 INFO，最多保留 7 天或 20 MiB。

报告漏洞前请阅读 [SECURITY.md](SECURITY.md)（英文）。请勿在公开 Issue 中提交凭据或未经脱敏的诊断信息。正式包的签名规则见[代码签名策略（英文）](docs/CODE_SIGNING.md)。

## 构建与贡献

本项目固定使用 Rust `1.97.1`、Flutter `3.44.7`、Android NDK `29.0.14206865` 以及仓库内的打包工具。开发环境、检查命令与 Pull Request 要求见 [CONTRIBUTING.md](CONTRIBUTING.md)（英文）。

实现进度见[实现进度（英文）](docs/IMPLEMENTATION.md)，签名与发布流程见[发布说明（英文）](docs/RELEASE.md)。

## 上游与许可

协议与行为参考 [Diniboy1123/usque](https://github.com/Diniboy1123/usque)。本仓库在 `oracle/go` 中保存一份快照，供互操作测试使用。Flutter 界面与 Rust 引擎为本项目新实现。上游版权声明见许可证。

源码采用 [MIT License](LICENSE.md)，第三方组件保留各自许可证。
