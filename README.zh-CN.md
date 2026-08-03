<p align="center">
  <img src="assets/branding/usque-readme-banner.png" alt="Usque — 兼容 Cloudflare WARP 的非官方客户端" width="100%">
</p>

<p align="center">
  <a href="README.md">English</a>
</p>

# Usque

Usque 是面向 Consumer Cloudflare WARP 的开源原生 GUI 客户端。Flutter 负责界面，内存安全的 Rust Engine 负责 MASQUE、CONNECT-IP、DNS、代理与连接状态；项目不使用 WebView。

> [!IMPORTANT]
> Usque GUI 正在开发中，目前还没有可安装的公开 Beta。共享 Rust 数据通道、Android VPN 纵向切片、Windows Agent 与 MSI 定义已经存在，但实机、全新安装、性能、签名和隔离防泄漏门禁仍未完成。请勿将仓库当前版本当作生产 VPN 使用。

Usque 是独立项目，与 Cloudflare 无隶属、赞助或背书关系。Cloudflare 与 WARP 是 Cloudflare, Inc. 的商标。使用 Consumer WARP 仍须遵守 Cloudflare 适用的条款与隐私政策。

## 首个 Beta 目标

只有下列所有目标均通过安装、互操作与防泄漏测试后，才会统一发布首个公开 Beta。

| 平台 | 原生产物 | 最低系统 | 兼容产物 |
| --- | --- | --- | --- |
| Windows | ARM64、x86-64-v2 | Windows 10 22H2（Build 19045） | 适用于旧 CPU 的 x86-64-v1 |
| Android / Android TV | arm64-v8a、armeabi-v7a、x86_64、Universal APK | Android 8.0 / API 26 | Universal APK |

macOS 源码保留为后续目标，但不参与 `v0.1.0-beta.1` 的构建、测试、打包或发布阻断。首个 Beta 不支持 Linux、iOS、Zero Trust、应用商店、公开 CLI 或多路径带宽叠加。

## Beta 功能

- 注册 Consumer WARP 身份或手动录入 WARP Secret。
- 原生 VPN 全隧道、隧道内 DNS、Kill Switch、局域网访问与 CIDR 绕过。
- 互斥的 SOCKS5 与 HTTP Proxy 辅助模式。
- 默认 HTTP/3 over QUIC，失败后自动回退 HTTP/2 over TLS。
- IPv4/IPv6 Happy Eyeballs 竞速，但只保留一条活动通道。
- 可保存多个 Profile，同一时间只有一个 Active Profile。
- 连接后通过隧道调用 [IP.SB](https://ip.sb/api)，显示出口 IPv4、IPv6 与位置。
- 本地诊断会脱敏；无自动遥测或上传。
- 英语/简中、浅色/深色、键盘、屏幕阅读器和 Android TV D-pad。

## 默认网络设置

| 设置 | 默认值 |
| --- | --- |
| Endpoint IPv4 | `162.159.198.2` |
| Endpoint IPv6 | `2606:4700:103::2` |
| 端口 | `443` |
| SNI | `www.visa.cn` |
| 传输 | Auto：HTTP/3，然后 HTTP/2 |
| MTU | `1280` |
| 备用 DNS | `1.1.1.1`、`2606:4700:4700::1111` |
| SOCKS5 | `127.0.0.1:1080`、`[::1]:1080` |
| HTTP Proxy | `127.0.0.1:8080`、`[::1]:8080` |

高级用户可以修改这些值并一键恢复默认。非 Loopback 代理监听不添加用户名/密码认证，并会持续显示醒目的安全警告。

## 安装与更新

目前没有可安装版本。公开 Beta 准备完成后，安装包只会发布在本仓库的 GitHub Releases 页面。

Windows 使用稳定自签名身份。安装前请核对发布页的 SHA-256 和证书指纹，再按系统提示完成安装。Android APK 使用固定 Release Keystore，但首个 Beta 暂不登记 Android Developer Console；随着 Android 开发者验证扩大，安装时可能需要高级侧载流程或 ADB。项目不会进入 Microsoft Store 或 Google Play。

Usque 不会自动安装更新。应用最多每 24 小时检查一次 GitHub prerelease，只提示并打开下载页；用户可以关闭检查。

## 三种互斥模式

- **VPN**：创建系统隧道并管理路由、DNS 与 Kill Switch。
- **SOCKS5**：支持 TCP 与 UDP，默认远程解析 DNS。
- **HTTP Proxy**：支持 CONNECT 与普通 HTTP 转发。

VPN 是默认模式。除非高级用户主动修改，代理只监听 IPv4/IPv6 Loopback。

## 安全与隐私

- GUI 不提供不安全 TLS 选项，Endpoint Pin 必须启用。
- WARP Secret、私钥、Token、设备 ID、License 与 Endpoint Pin 应由 Windows Credential Manager 或 Android Keystore 保存。
- 首个 Beta 将身份材料视为只写秘密：可替换或清除，但不能以明文查看、复制或导出。
- 桌面端拆分为非特权 Engine 与最小权限 Agent；Agent 只管理 TUN、路由、DNS、防火墙和系统代理。
- Android 的 `VpnService` 与 Rust 库运行在独立 `:vpn` 进程。
- Kill Switch 设计为在重连和 Engine/Agent 恢复期间继续阻断泄漏；故障注入测试未通过前禁止发布 Beta。
- 日志默认 INFO，最多保留 7 天或 20 MiB；无分析、遥测、自动诊断上传，也不会绕过隧道下载旗帜。

安全问题报告方式和当前成熟度说明见 [SECURITY.md](SECURITY.md)。

## 仓库结构

```text
apps/usque_gui/       Flutter UI 与 Windows/Android 宿主；macOS 源码延后
crates/usque-core/    Profile、状态机、传输调度与出口探测
crates/usque-protocol RFC 9484 CONNECT-IP 编解码
crates/usque-ipc/     版本化 protobuf 帧
crates/usque-platform 最小权限平台边界
crates/usque-engine/  非特权桌面 Engine 进程
crates/usque-android/ Android JNI 边界
proto/usque/v1/       公共控制和事件契约
oracle/go/            归档的上游 Go 行为 Oracle，不发布
tool/                 可复现的资源与打包辅助工具
```

原 Go 实现保存在 `oracle/go`，只用于协议对照、抓包复现和回归测试，不再作为 CLI 发布。

## 从源码构建

锁定工具链：

- `rust-toolchain.toml` 中的 Rust `1.97.1`
- Flutter `3.44.7` / Dart `3.12.2`
- 对应平台编译器：Visual Studio Build Tools 或 Android SDK/NDK

运行平台无关检查：

```shell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked

cd apps/usque_gui
flutter pub get --enforce-lockfile
flutter analyze --no-pub
flutter test --no-pub
```

Windows 可通过开发人员模式提供 Flutter 所需的符号链接；如果有意保持关闭，可在 `flutter pub get` 后运行 `tool/prepare_windows_plugin_junctions.ps1`，它只会创建项目内插件 Junction。Android 构建会由 Gradle 调用 `tool/build_android_rust.ps1`，并要求 NDK `29.0.14206865`；Release 还需要受保护的固定 Keystore。

当前代码仍是开发纵向切片。Windows 已能建立严格 Pin 的 Cloudflare HTTP/3/QUIC 与 HTTP/2/TLS 数据通道，并在不创建 TUN 的情况下提供 SOCKS5 TCP/UDP 以及 HTTP CONNECT/Forward；其最小权限 Agent 也已实现认证 IPC、Wintun 共享内存包环、事务化路由/DNS/系统代理、持久 WFP Kill Switch、崩溃重附着和失败关闭的 WiX MSI 定义。Android 已具备真实 Rust Runtime、MASQUE 数据通道、保留 TUN 的重连、路由排除规划、Binder 事件、Keystore 身份和纯代理服务路径。这些实现已具备单元、Mock、编译和回环代理测试，但两个平台均未完成干净系统、实机、24 小时、性能和独立网关防泄漏实验室验证；在全部门槛通过前不得发布 Beta。明确里程碑见 [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md)，受保护发布链见 [docs/RELEASE.md](docs/RELEASE.md)。

## 贡献、上游与许可

提交修改前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。传输改动必须补充 Go Oracle 互操作覆盖；路由、DNS、TUN、防火墙或生命周期改动必须补充防泄漏测试。

Usque GUI fork 自 [Diniboy1123/usque](https://github.com/Diniboy1123/usque)，Android 兼容行为参考 [Abobo7/usque-android](https://github.com/Abobo7/usque-android)，完整保留上游版权与归属。

源码使用 [MIT License](LICENSE.md)，第三方组件保留各自许可。
