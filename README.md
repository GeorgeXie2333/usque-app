<p align="center">
  <img src="assets/branding/usque-readme-banner.png" alt="Usque — unofficial client compatible with Cloudflare WARP" width="100%">
</p>

<p align="center">
  <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <a href="https://github.com/GeorgeXie2333/usque-app/actions/workflows/pr-check.yml"><img alt="PR Check" src="https://github.com/GeorgeXie2333/usque-app/actions/workflows/pr-check.yml/badge.svg"></a>
  <a href="https://github.com/GeorgeXie2333/usque-app/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/GeorgeXie2333/usque-app/actions/workflows/ci.yml/badge.svg?branch=main"></a>
  <a href="https://github.com/GeorgeXie2333/usque-app/actions/workflows/build.yml"><img alt="Build" src="https://github.com/GeorgeXie2333/usque-app/actions/workflows/build.yml/badge.svg"></a>
  <a href="LICENSE.md"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-F48120.svg"></a>
  <img alt="Rust 1.97.1" src="https://img.shields.io/badge/Rust-1.97.1-dea584.svg?logo=rust&logoColor=white">
  <img alt="Flutter 3.44.7" src="https://img.shields.io/badge/Flutter-3.44.7-02569B.svg?logo=flutter&logoColor=white">
  <img alt="Windows 10 22H2 or later" src="https://img.shields.io/badge/Windows-10%2022H2%2B-2F2F2F.svg">
  <img alt="Android 8 or later" src="https://img.shields.io/badge/Android-8.0%2B-2F2F2F.svg">
</p>

# Usque

Usque is an unofficial GUI client for consumer Cloudflare WARP. Flutter draws the UI. A Rust engine handles MASQUE, CONNECT-IP, DNS, proxies, and connection state. There is no WebView.

> [!IMPORTANT]
> The current release is **v0.1.2**. Only files attached to the [`v0.1.2` GitHub Release](https://github.com/GeorgeXie2333/usque-app/releases/tag/v0.1.2), with matching checksums and signer fingerprints, are official. Pull Request artifacts, local builds, and untagged binaries are not.

Usque is an independent project. It is not affiliated with, sponsored by, or endorsed by Cloudflare. Cloudflare and WARP are trademarks of Cloudflare, Inc. Use of consumer WARP remains subject to Cloudflare's terms and privacy policy.

## Release targets

The `v0.1.2` tag on `main` builds and checks these six packages:

| Platform | Package | Minimum OS | Architecture |
| --- | --- | --- | --- |
| Windows | MSI | Windows 10 22H2, build 19045 | x64-v2 |
| Windows | MSI | Windows 10 22H2, build 19045 | ARM64 |
| Android / Android TV | per-ABI APK | Android 8.0, API 26 | ARMv8 (`arm64-v8a`) |
| Android / Android TV | per-ABI APK | Android 8.0, API 26 | x64 (`x86_64`) |
| Android / Android TV | per-ABI APK | Android 8.0, API 26 | ARMv7 (`armeabi-v7a`) |
| Android / Android TV | universal APK | Android 8.0, API 26 | all three Android ABIs |

macOS source is in the tree but is not built or released. This release does not include iOS, Zero Trust, store listings, a public CLI, or multipath bandwidth aggregation.

## Highlights

- Consumer WARP registration, WARP License Key registration, and WARP Secret import/export.
- VPN, SOCKS5, HTTP proxy, and Windows system proxy can run together on one MASQUE channel.
- HTTP/3 over QUIC, falling back to HTTP/2 over TLS, with IPv4/IPv6 Happy Eyeballs for the physical path.
- Full-tunnel VPN, tunneled DNS, Kill Switch, LAN access, and custom CIDR bypass rules.
- SOCKS5 TCP/UDP and HTTP CONNECT/forward; listeners default to loopback.
- Several profiles, one active at a time, with identity stored per profile.
- Android Quick Settings tile, launcher shortcuts, boot recovery, and TV navigation.
- Windows tray, single-instance activation, start on boot, and close-to-tray.
- Local redacted diagnostics. No analytics, telemetry, or automatic upload.

Choosing an IPv4 or IPv6 MASQUE endpoint only picks the physical ingress. Either path can carry IPv4 and IPv6 inside CONNECT-IP. Usque keeps one active transport; it does not add bandwidth across paths.

## Default network settings

| Setting | Default |
| --- | --- |
| Endpoint IPv4 | `162.159.198.2` |
| Endpoint IPv6 | `2606:4700:103::2` |
| Port | `443` |
| SNI | `speed.cloudflare.com` |
| Transport | Auto: HTTP/3, then HTTP/2 |
| MTU | `1280` |
| Fallback DNS | `1.1.1.1`, `2606:4700:4700::1111` |
| SOCKS5 | `127.0.0.1:1080`, `[::1]:1080` |
| HTTP Proxy | `127.0.0.1:8080`, `[::1]:8080` |

These values can be changed and reset. A non-loopback proxy listener has no password and always shows a warning.

## Availability and installation

Download `v0.1.2` only from the [official release page](https://github.com/GeorgeXie2333/usque-app/releases/tag/v0.1.2):

| Target | File |
| --- | --- |
| Windows x64 | [`usque-v0.1.2-windows-x64-v2.msi`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-windows-x64-v2.msi) |
| Windows ARM64 | [`usque-v0.1.2-windows-arm64.msi`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-windows-arm64.msi) |
| Android ARMv8 | [`usque-v0.1.2-android-arm64-v8a.apk`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-android-arm64-v8a.apk) |
| Android x64 | [`usque-v0.1.2-android-x86_64.apk`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-android-x86_64.apk) |
| Android ARMv7 | [`usque-v0.1.2-android-armeabi-v7a.apk`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-android-armeabi-v7a.apk) |
| Android universal | [`usque-v0.1.2-android-universal.apk`](https://github.com/GeorgeXie2333/usque-app/releases/download/v0.1.2/usque-v0.1.2-android-universal.apk) |

Prefer the APK that matches the device ABI. The universal APK includes ARMv8, x64, and ARMv7 libraries and is larger; use it when the architecture is unknown. The release also has `SHA256SUMS`, a `.sha256` file per package, SPDX/CycloneDX SBOMs, license inventories, and build attestations.

- Pre-1.0 Windows packages use a fixed self-signed identity. Check the published SHA-256 and certificate fingerprint before accepting the OS warning.
- Pre-1.0 Android packages use a project-controlled self-signed certificate and are not on Google Play. You may need a manual install or ADB.
- A later v1.0.0 signing change will be its own release.
- The release workflow compiles, signs, and checks architecture, checksums, SBOMs, and provenance. It does not install packages on real devices or run long VPN tests.
- Usque never installs updates by itself. Optional update checks only open the release page.
- Windows uninstall asks for confirmation in Settings, restores Usque-owned network state, and can delete the current user's local data if you ask.

See [Installation and removal](docs/INSTALLATION.md) for verification, upgrades, uninstall, and recovery.

## Outputs

One profile can enable several outputs. They share one pinned MASQUE transport and a packet multiplexer.

| Output | Behavior |
| --- | --- |
| VPN/TUN | Creates a system tunnel and manages routes, DNS, and Kill Switch rules. |
| SOCKS5 | TCP and UDP; remote DNS by default. |
| HTTP Proxy | CONNECT and ordinary HTTP forwarding. |
| Windows system proxy | Needs HTTP output; points Windows at the local listener. |

Windows defaults to VPN/TUN + SOCKS5 + HTTP, with the system proxy off. Android defaults to VPN + SOCKS5 + HTTP. You can turn every output off and leave only the transport up.

## Security and privacy

- Endpoint pinning is always on. The GUI has no insecure TLS mode.
- Secrets, private keys, tokens, device identifiers, licenses, and endpoint pins go in Windows Credential Manager or Android Keystore.
- Secret export is explicit, confirmed, and written only to a path you pick.
- The Windows engine runs unprivileged. A small Agent owns TUN, routes, DNS, firewall, and system-proxy state.
- Android uses `VpnService` and an isolated `:vpn` process.
- Logs default to INFO and stop at 7 days or 20 MiB.

Read [SECURITY.md](SECURITY.md) before reporting a vulnerability. Do not put credentials or raw diagnostics in a public Issue. Official package signatures are described in the [code signing policy](docs/CODE_SIGNING.md).

## Build and contribute

The tree pins Rust `1.97.1`, Flutter `3.44.7`, Android NDK `29.0.14206865`, and the packaging tools. [CONTRIBUTING.md](CONTRIBUTING.md) has setup, checks, safety limits, and pull request rules.

Progress is in [Implementation](docs/IMPLEMENTATION.md). Signing and the release workflow are in [Release process](docs/RELEASE.md).

## Upstream and license

Protocol behavior follows [Diniboy1123/usque](https://github.com/Diniboy1123/usque). This repository keeps a snapshot of that client in `oracle/go` for interoperability tests. The Flutter UI and Rust engine are new code. Upstream copyright stays in the license.

Source is [MIT](LICENSE.md). Third-party components keep their own licenses.
