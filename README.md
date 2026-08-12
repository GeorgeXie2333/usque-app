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
  <img alt="Windows 10 22H2 or later" src="https://img.shields.io/badge/Windows-10%2022H2%2B-2F2F2F.svg">
  <img alt="Android 8 or later" src="https://img.shields.io/badge/Android-8.0%2B-2F2F2F.svg">
</p>

# Usque

Usque is an open-source, native GUI client for Consumer Cloudflare WARP. Flutter renders the interface, while a memory-safe Rust engine owns MASQUE, CONNECT-IP, DNS, proxying, and connection state. Usque does not use a WebView.

> [!IMPORTANT]
> Usque is under active development. **No public beta binary is available yet.** The repository has not completed its clean-install, real-device, performance, signing, and independent leak-prevention gates. Do not use repository builds as a production VPN.

Usque is an independent project. It is not affiliated with, sponsored by, or endorsed by Cloudflare. Cloudflare and WARP are trademarks of Cloudflare, Inc. Use of Consumer WARP remains subject to Cloudflare's applicable terms and privacy policy.

## Beta target

The `v0.1.1-beta.2` release is blocked until every declared target passes installation, interoperability, lifecycle, and leak-prevention testing.

| Platform | Planned package | Minimum OS | Architecture |
| --- | --- | --- | --- |
| Windows | MSI | Windows 10 22H2, build 19045 | x86-64-v2 |
| Android / Android TV | APK | Android 8.0, API 26 | arm64-v8a |

macOS remains a future source target and is not a build or release gate. iOS, Zero Trust, application stores, a public CLI, and multipath bandwidth aggregation are outside this beta.

## Highlights

- Consumer WARP registration, WARP License Key registration, and existing WARP Secret import/export.
- Composable VPN, SOCKS5, HTTP Proxy, and Windows system-proxy outputs over one MASQUE channel.
- HTTP/3 over QUIC with HTTP/2 over TLS fallback and IPv4/IPv6 Happy Eyeballs ingress selection.
- Full-tunnel VPN, tunneled DNS, Kill Switch, LAN access, and user-defined CIDR bypass rules.
- SOCKS5 TCP/UDP and HTTP CONNECT/Forward with loopback-only listeners by default.
- Multiple profiles with one active profile and securely isolated identity material.
- Android Quick Settings Tile, launcher shortcuts, boot recovery, and TV-safe navigation.
- Windows tray integration, single-instance activation, start-on-boot, and close-to-tray behavior.
- Local, redacted diagnostics with no analytics, automatic telemetry, or automatic upload.

Selecting an IPv4 or IPv6 MASQUE endpoint changes only the physical ingress. Either ingress can carry both IPv4 and IPv6 packets inside CONNECT-IP; Usque keeps one active transport and does not aggregate bandwidth.

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

Advanced users can change these values and restore them in one action. A non-loopback proxy listener is intentionally unauthenticated and always displays a prominent security warning.

## Availability and installation

There are no public installable releases yet. When the beta is ready, packages will appear only on this repository's [GitHub Releases](https://github.com/GeorgeXie2333/usque-app/releases) page.

- Windows packages use a stable self-signed identity. Verify the published SHA-256 checksum and certificate fingerprint before accepting the operating-system warning.
- Android packages use a fixed release keystore and are distributed outside Google Play. Advanced sideloading or ADB may be required.
- Updates are never installed automatically. Optional update checks only open the release page.
- Windows uninstall restores Usque-owned network state before removing the service and can optionally delete the current user's local data.

See [Installation and removal](docs/INSTALLATION.md) for package verification, platform warnings, upgrades, uninstall behavior, and recovery boundaries.

## Outputs

One profile can enable several outputs at once. They share one pinned MASQUE transport and a packet multiplexer.

| Output | Behavior |
| --- | --- |
| VPN/TUN | Creates a system tunnel and manages routes, DNS, and Kill Switch rules. |
| SOCKS5 | Supports TCP and UDP with remote DNS by default. |
| HTTP Proxy | Supports CONNECT and ordinary HTTP forwarding. |
| Windows system proxy | Depends on HTTP output and points Windows at its local listener. |

Windows defaults to VPN/TUN + SOCKS5 + HTTP with the system proxy disabled. Android defaults to VPN + SOCKS5 + HTTP. Turning every output off is allowed and keeps only the transport available.

## Security and privacy

- Endpoint pinning is mandatory; the GUI has no insecure TLS mode.
- Secrets, private keys, tokens, device identifiers, licenses, and endpoint pins are stored in Windows Credential Manager or Android Keystore.
- Secret export is explicit, confirmed, and written only to a user-selected destination.
- The Windows engine is unprivileged; a narrow Agent owns only TUN, routes, DNS, firewall, and system-proxy state.
- Android uses `VpnService` and an isolated `:vpn` process.
- Logs default to INFO and are capped at 7 days or 20 MiB.

Read [SECURITY.md](SECURITY.md) before reporting a vulnerability. Never put credentials or unredacted diagnostics in a public Issue.

## Build and contribute

The project pins Rust `1.97.1`, Flutter commit `84fc5cbb223bc12f83d65b647ff8a56caf779ffd`, Android NDK `29.0.14206865`, and its packaging tools. Start with [CONTRIBUTING.md](CONTRIBUTING.md) for setup, check commands, safety boundaries, and Pull Request requirements.

Architecture and status are tracked in [Implementation roadmap](docs/IMPLEMENTATION.md). The protected signing and laboratory chain is documented in [Release process](docs/RELEASE.md).

## Upstream and license

Usque GUI is forked from [Diniboy1123/usque](https://github.com/Diniboy1123/usque). Upstream copyright and attribution are retained.

Source code is released under the [MIT License](LICENSE.md). Third-party components retain their own licenses.
