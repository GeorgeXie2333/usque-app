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
</p>

# Usque

Usque 是面向 Windows 和 Android / Android TV 的非官方 Cloudflare WARP 客户端。它将系统 VPN、SOCKS5 和 HTTP 代理整合在原生 Flutter 界面中，由 Rust MASQUE 引擎提供网络能力，不使用 WebView。

> [!IMPORTANT]
> 请仅从 [GitHub Releases](https://github.com/GeorgeXie2333/usque-app/releases) 下载正式安装包。Pull Request 构建、本地构建及未打标签的二进制均非正式发布。开发分支文档可能包含尚未发布的改动；请以安装包对应的发布说明和标签下文档为准。

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
      <img src="assets/screenshots/usque-android-home.jpg" alt="Usque Android 主界面" width="280">
    </td>
  </tr>
</table>

## 下载与安装

本文对应 **v0.2.7** 的源码和功能。已发布版本请查看 [GitHub Releases](https://github.com/GeorgeXie2333/usque-app/releases)。安装包共六种，另外提供两个仅供应用内更新使用的 Windows MSI 文件：

| 平台 | 最低系统 | 安装包 |
| --- | --- | --- |
| Windows | Windows 10 22H2，Build 19045 | x64-v2 安装程序 EXE 或 ARM64 安装程序 EXE |
| Android / Android TV | Android 8.0，API 26 | arm64-v8a、x86_64 或 armeabi-v7a APK |
| Android / Android TV | Android 8.0，API 26 | 包含上述三种 ABI 的通用 APK |

请选择与设备架构匹配的安装包。不清楚 Android 设备架构时，可使用体积更大的通用 APK。安装前，将软件包 SHA-256 与 `SHA256SUMS` 及 GitHub 显示的文件校验值比对，再核验发布说明中的签名者指纹。任何一项不一致都应停止安装。

1.0 之前的安装包使用项目自行管理的固定自签名证书。Windows 可能显示“未知发布者”警告；Android 安装包不通过 Google Play 分发。不要通过关闭杀毒软件、防火墙或导入非官方安装包提供的证书来绕过警告。

升级、卸载、恢复及 Android 开发者验证说明见[安装指南](docs/INSTALLATION.md)，官方签名身份见[代码签名策略](docs/CODE_SIGNING.md)。更新下载需要用户确认，安装通过平台安装程序完成，不会未经确认自动安装。

## 首次连接

1. 按[安装指南](docs/INSTALLATION.md#verify-before-installing)核验正式安装包，安装后打开 Usque。
2. 完成首次启动的权限与条款步骤，注册个人版 WARP 账号；也可以填写 WARP License Key。目前不支持导入新的 WARP Secret。
3. 在“VPN 与本地代理”中选择要启用的联网方式，然后在主页连接。Android 首次启用 VPN 时会请求授权；仅使用 SOCKS5 或 HTTP 代理时无需此授权。

| 联网方式 | 用途 |
| --- | --- |
| VPN/TUN | 让系统流量经过隧道，并遵循绕过规则和 Android 分应用设置。 |
| SOCKS5 | 为支持它的应用提供本地 TCP/UDP 代理，默认通过远程 DNS 查询域名。 |
| HTTP 代理 | 为支持它的应用提供本地 HTTP 代理，也支持通过 CONNECT 访问 HTTPS。 |
| Windows 系统代理 | 将 Windows 的代理设置指向 Usque；需同时启用 HTTP 代理。 |

VPN、SOCKS5 和 HTTP 默认开启，Windows 系统代理默认关闭。它们共用一条 WARP 连接，可以同时使用。
全部关闭后，Usque 仍可保持传输连接，但不再提供上述应用联网方式。
每个账号单独保存凭据，一次只能使用一个账号连接；网络设置由所有账号共享。

## 主要功能

- 可选的[链式代理](docs/CHAIN_PROXY.md)：在“代理 → 链式代理”依次选择 **OpenVPN**、
  **WireGuard**、[**WARP via WireGuard**](docs/WARP_WIREGUARD.md) 或 **VPN Gate**。
  自定义配置支持文件或文本导入，保存多份后选用并应用。
  VPN、SOCKS5 和 HTTP 共用最终出口，显式直连规则仍然生效。此功能默认关闭。
- 可手动启用[实验性 L4 模式](docs/L4_PROXY.md)，通过 HTTP/3 代理 TCP 流量。
  未启用 OpenVPN TCP 链式出口时，它不转发普通 UDP 流量，需要 UDP 的应用可能无法正常使用。自动模式不会选择 L4。
- 自动尝试 HTTP/3，失败时回退到 HTTP/2，并尝试通过 IPv4 和 IPv6 寻找可达入口。
  H3 在支持的网络切换场景下可以迁移连接，详见[路径行为](docs/h3-path-infrastructure.md)。
- 全隧道 VPN、隧道内 DNS、Kill Switch（断网保护）、局域网访问和自定义 CIDR 绕过规则。
- 可选的按国家直连：单独下载所选国家的 GeoIP 数据和全局 GeoSite 域名规则。
  能看到域名时按域名判断，否则按 IP 判断；无法识别的目标继续走隧道。
- 本地[网络诊断](docs/network-doctor.md)和网络质量页面，展示延迟、丢包率及其测量状态、队列和最近 60 秒的趋势。
  标准检查只读取本地状态；深度检查会在你确认后发送测试请求。
- Windows 支持托盘、单实例、开机启动和关闭窗口后最小化到托盘；
  Android 支持快捷设置磁贴、启动器快捷方式、开机恢复和电视遥控器导航。
  提供 21 种语言，以及浅色、深色主题。
- 可在确认后将个人版 WARP Secret 导出到指定文件。Usque 目前无法重新导入该文件，因此它不能用于重装后的账号恢复。

Android 的“分应用代理”对所有账号生效。关闭时，所有应用都使用 VPN；开启后，只有勾选的应用使用 VPN，
新安装的应用需要手动勾选。如果同时启用了系统的“阻止未使用 VPN 的连接”，未勾选的应用将无法联网。

## 隐私与限制

- 强制校验 WARP 服务端公钥，不提供跳过校验的选项。凭据保存在 Windows 凭据管理器或 Android Keystore 中。
  Windows 由独立 Agent 执行需要管理员权限的网络操作；Android 使用独立的 VPN 进程。
- 代理默认仅供本机使用。SOCKS5 和 HTTP 支持可选的用户名、密码认证；未配置凭据时不要求认证。
  仅使用代理不能提供系统级 VPN 断网保护。
- 诊断在本地生成并脱敏，不进行用户行为统计，也不自动上传。网络质量历史只保留在内存中。
  日志默认为 INFO，最多保留 7 天或 20 MiB。不要将凭据或原始诊断包放入公开 Issue；漏洞请通过 [SECURITY.md](SECURITY.md) 私密报告。
- Android 应用内的断网保护无法在 VPN 进程结束后继续工作。VPN Gate 无法继续连接时，也会断开 VPN。
  如果需要在 VPN 结束后继续阻止应用联网，请同时开启系统的“始终开启的 VPN”和“阻止未使用 VPN 的连接”。
  操作方法见 [Android 安装说明](docs/INSTALLATION.md#android-and-android-tv)。
- Usque 不会通过多条路径叠加带宽。部分指标无法测量，例如 HTTP/2 的丢包率和 PMTU。
  本地诊断通过并不代表已经证明没有流量泄漏，或测得了性能提升。

### DNS 隐私

按国家直连默认使用 **System**：匹配规则的域名查询会发送给当前网络使用的 DNS 服务器，位于 VPN 隧道之外。
也可以选择 **DoH** 或 **DoT**，自行填写加密 DNS 服务器的域名和 IP 地址。
查询会发送给该服务器；连接失败时不会改用明文 DNS。详见[配置步骤与示例](docs/encrypted-direct-dns.md)。

其他远程 VPN 查询使用最终出口提供的 DNS：通常为 WARP，启用链式代理后使用所选最终出口。
手动设置的本地 DNS 和代理 DNS 仍按各自规则生效。
应用自行使用加密 DNS 时，Usque 看不到域名，此时按 IP 判断是否直连。
断开连接后下载规则，也仍受 Android 系统阻断设置和尚未解除的 Windows 断网保护约束。

### 实验性功能与支持范围

[Zero Trust 注册](docs/ZERO_TRUST_EXPERIMENTAL.md)仍属实验性功能：可以用组织身份建立 MASQUE 公网隧道，
但不提供完整的 Cloudflare One Client 功能。仓库保留 macOS 源码，但不构建或发布；
当前也不提供 iOS 版本、应用商店分发或公开命令行工具。

## 默认网络设置

| 设置 | 默认值 |
| --- | --- |
| 个人版端点 IPv4 | `162.159.198.2` |
| 个人版端点 IPv6 | `2606:4700:103::2` |
| 端口 / SNI | `443` / `speed.cloudflare.com` |
| 传输 | 自动：先 HTTP/3，再 HTTP/2 |
| HTTP/3 拥塞控制 | `cubic`；可选择 BBRv2、实验性 BBRv3 和 `reno` |
| QUIC UDP 接收缓冲 | Windows/Android 的 H3、L4 均以 [2 MiB 为目标](docs/UDP_RECEIVE_BUFFER.md)申请；实际容量由系统决定 |
| TUN MTU | `1280` |
| 备用 DNS | `1.1.1.1`、`2606:4700:4700::1111` |
| SOCKS5 | `127.0.0.1:1080`、`[::1]:1080` |
| HTTP 代理 | `127.0.0.1:8080`、`[::1]:8080` |

修改代理地址、端口和 DNS 后，需要点击应用更改才会提交。高级设置中的重置只会填入默认值，仍需提交才会生效。Zero Trust 入口地址由注册返回，不能手动修改。

拥塞控制的更改会保存，并在下一次手动连接或重试时生效，不影响当前会话及其自动重连。HTTP/2 使用系统 TCP。详见 [HTTP/3 拥塞控制](docs/congestion-control.md)。

## 文档与开发

从[文档索引](docs/README.md)选择需要的内容。技术文档目前以英文为主。

| 需要了解 | 阅读文档 |
| --- | --- |
| 安装、更新、卸载与恢复 | [安装指南](docs/INSTALLATION.md) |
| 本地网络质量检查 | [Network Doctor](docs/network-doctor.md) |
| 安全地构建和测试改动 | [贡献指南](CONTRIBUTING.md) |
| 实现与验证状态 | [实现进度](docs/IMPLEMENTATION.md) |
| 维护正式发布 | [发布流程](docs/RELEASE.md) |

请遵循贡献指南中的固定工具链和按改动范围划分的检查。仅编译构建和确定性测试可在开发机进行；安装开发包或测试 VPN 生命周期必须使用规定的隔离环境。构建成功不等于完成安装、泄漏或性能验证。

## 上游与许可

协议与行为参考 [Diniboy1123/usque](https://github.com/Diniboy1123/usque)。本仓库在 `oracle/go` 中保存一份快照，供互操作测试使用。Flutter 界面与 Rust 引擎为本项目新实现。上游版权声明见许可证。

第一方源码采用 [MIT License](LICENSE.md)，第三方组件保留各自许可证。可选的[链式代理](docs/CHAIN_PROXY.md)内嵌 OpenVPN 3 Core（MPL-2.0）和 Mbed TLS（Apache-2.0）。对应源码、补丁和许可文本保存在 `third_party`；应用中的 VPN Gate 页面可查看原生依赖许可声明。

WireGuard 使用 BoringTun 0.7.1（BSD-3-Clause），本地 SVG 图标使用 flutter_svg 2.3.0（MIT）；相应声明已纳入应用许可证列表。
