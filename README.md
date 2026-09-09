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
</p>

# Usque

Usque is an unofficial Cloudflare WARP client for Windows and Android / Android TV. It combines a system VPN, SOCKS5, and HTTP proxy in a native Flutter interface, powered by a Rust MASQUE engine. There is no WebView.

> [!IMPORTANT]
> Download official packages only from [GitHub Releases](https://github.com/GeorgeXie2333/usque-app/releases). Pull Request artifacts, local builds, and untagged binaries are not official. Development-branch documentation can describe changes not yet released; check the release notes and documentation at your package's tag.

Usque is an independent project. It is not affiliated with, sponsored by, or endorsed by Cloudflare. Cloudflare and WARP are trademarks of Cloudflare, Inc. Use of consumer WARP remains subject to Cloudflare's terms and privacy policy.

## Screenshots

<table>
  <tr>
    <td align="center" valign="top">
      <p><strong>Windows</strong></p>
      <img src="assets/screenshots/usque-windows-home.png" alt="Usque Home on Windows" width="720">
    </td>
    <td align="center" valign="top">
      <p><strong>Android</strong></p>
      <img src="assets/screenshots/usque-android-home.jpg" alt="Usque Home on Android" width="280">
    </td>
  </tr>
</table>

## Download and install

The release target is **v0.2.5**, a feature and reliability release for Windows and Android. Its tag workflow produces six user-facing installers plus two Windows MSI payloads reserved for automatic updates:

| Platform | Minimum OS | Packages |
| --- | --- | --- |
| Windows | Windows 10 22H2, build 19045 | x64-v2 installer EXE or ARM64 installer EXE |
| Android / Android TV | Android 8.0, API 26 | arm64-v8a, x86_64, or armeabi-v7a APK |
| Android / Android TV | Android 8.0, API 26 | Universal APK containing all three ABIs |

Choose the package matching your device architecture. Use the larger universal APK when the Android ABI is unknown. Before installing, compare the package SHA-256 with `SHA256SUMS` and GitHub's asset digest, then verify the signer fingerprint published in the release notes. Stop if any value differs.

Pre-1.0 packages use fixed, project-controlled self-signed certificates. Windows may show an unknown-publisher warning; Android packages are installed outside Google Play. Do not disable antivirus or the firewall, or import certificates from unofficial packages, to bypass a warning.

See [Installation and removal](docs/INSTALLATION.md) for upgrades, uninstall, recovery, and Android developer-verification details, and [Code signing](docs/CODE_SIGNING.md) for official identities. Updates require confirmation before downloading and use the platform installer; there is no unattended installation.

## First connection

1. Install a verified official package and open Usque.
2. Complete the first-run permissions and terms steps. Register a Consumer WARP identity, optionally with a WARP License Key. New WARP Secret imports are not supported.
3. Choose the outputs you need, then connect from Home. Android requests VPN consent when VPN output is first enabled; SOCKS5/HTTP-only use does not require it.

| Output | What it does |
| --- | --- |
| VPN/TUN | Routes system traffic through the tunnel, subject to your bypass and Android per-app settings. |
| SOCKS5 | Provides a local TCP/UDP proxy; remote DNS is the default. |
| HTTP proxy | Provides HTTP CONNECT and ordinary HTTP forwarding. |
| Windows system proxy | Points Windows at the local HTTP listener; requires HTTP output. |

VPN, SOCKS5, and HTTP are enabled by default on both platforms; Windows system proxy is off. Outputs share one MASQUE transport and can run together. Turning every output off leaves only the transport. Identities are stored per account, with one active account at a time; network settings are shared across accounts.

## Features

- Opt-in [experimental L4 proxy mode](docs/L4_PROXY.md): TCP CONNECT over H3
  for SOCKS5, HTTP and Windows/Android TUN, with DNS conversion and
  identity-derived Consumer/Zero Trust SNI. Auto still excludes L4.

- Consumer WARP accounts, optional License Key registration, and explicit, confirmed Secret export to a file you choose. Export does not provide an import/restore workflow in Usque.
- Auto HTTP/3 (QUIC) with HTTP/2 (TLS) fallback and IPv4/IPv6 Happy Eyeballs for the physical path. H3 supports same-family path migration and automatic outer-path PMTU discovery.
- Full-tunnel VPN, tunneled DNS, Kill Switch, LAN access, and custom CIDR bypass rules.
- Optional country-based direct routing: separately downloaded per-country GeoIP data and one verified global V2Fly GeoSite catalog. Known names use GeoSite; destinations without a visible name use GeoIP. Unknown destinations stay on MASQUE.
- A local Network Quality page with RTT, loss availability, queues, PMTU, migration, direct DNS, and 60-second trends. Network Doctor offers read-only Standard checks and explicitly authorized Deep checks.
- Windows tray, single-instance activation, start on boot, and close-to-tray; Android Quick Settings tile, launcher shortcuts, boot recovery, and TV navigation. Twenty-one language catalogs, plus light and dark themes.

Android per-app proxy is an app-wide include-only setting, not an account setting. When off, all apps use the VPN. When on, only selected apps do; newly installed apps stay outside the tunnel until selected. With Android **Block connections without VPN**, unselected apps are blocked instead of bypassing it.

## Privacy and limits

- Endpoint pinning is mandatory; there is no insecure TLS mode. Identity material is kept in Windows Credential Manager or Android Keystore. The Windows UI and Engine are unprivileged; a separate Agent manages privileged network state. Android uses a dedicated `:vpn` process.
- Proxy listeners default to loopback. Non-loopback listeners have no authentication and display a warning. Proxy-only mode is not a system-wide VPN Kill Switch.
- Diagnostics are local and redacted, with no analytics or automatic upload; quality history stays in memory. Logs default to INFO and are limited to 7 days or 20 MiB. Never post credentials or raw diagnostic bundles in a public Issue; report vulnerabilities through [SECURITY.md](SECURITY.md).
- Android's in-app Kill Switch does not survive the VPN process being killed. Use system **Always-on VPN** together with **Block connections without VPN** for that protection; see the [Android installation guidance](docs/INSTALLATION.md#android-and-android-tv).

Direct-country DNS is an explicit choice: **System** (default), **DoH**, or **DoT**. System exposes matching domains to the physical DNS provider; DoH/DoT exposes them to your chosen encrypted resolver, with numeric bootstrap, strict TLS, and no plaintext fallback. Other VPN queries continue through WARP DNS; proxy DNS settings remain separate. Application-owned encrypted DNS hides names from Usque, so classification uses GeoIP. Rule downloads still obey Android Lockdown and any surviving Windows Kill Switch while disconnected. See [Direct DNS](docs/encrypted-direct-dns.md).

There is one selected data plane, not multipath bandwidth aggregation. L4 may briefly keep a draining QUIC session during GOAWAY. Either physical endpoint family can carry IPv4 and IPv6 inside CONNECT-IP. Migration is same-family only; automatic PMTU does not raise the configured TUN MTU, and H2 loss and PMTU are N/A. Doctor results do not prove zero externally observed leaks or measured performance gains. Protected-runner validation is optional for publication; missing or failed evidence is never a pass.

Zero Trust enrollment is **experimental**, limited to an organization identity using the existing MASQUE Internet tunnel. It is not production-supported Cloudflare One Client compatibility. Read its [scope and validation requirements](docs/ZERO_TRUST_EXPERIMENTAL.md) before using it. macOS source is retained but not built or released; iOS, store distribution, and a public CLI are outside the current release scope.

## Default network settings

| Setting | Default |
| --- | --- |
| Consumer endpoint IPv4 | `162.159.198.2` |
| Consumer endpoint IPv6 | `2606:4700:103::2` |
| Port / SNI | `443` / `speed.cloudflare.com` |
| Transport | Auto: HTTP/3, then HTTP/2 |
| HTTP/3 congestion control | `cubic`; BBRv2, experimental BBRv3, and `reno` are selectable |
| TUN MTU | `1280` |
| Fallback DNS | `1.1.1.1`, `2606:4700:4700::1111` |
| SOCKS5 | `127.0.0.1:1080`, `[::1]:1080` |
| HTTP proxy | `127.0.0.1:8080`, `[::1]:8080` |

Proxy address, port, and DNS edits are drafts until applied. Advanced settings reset loads defaults into the draft; it does not apply them immediately. Zero Trust endpoint addresses come from registration and are not editable.

Congestion-control changes are saved for the next manual connection or retry,
not applied to the current session or its automatic reconnections. HTTP/2 uses
system TCP. See [HTTP/3 congestion control](docs/congestion-control.md).

## Documentation and development

Start with the [documentation index](docs/README.md). Most technical documents are in English.

| Need | Read |
| --- | --- |
| Install, update, uninstall, or recover | [Installation](docs/INSTALLATION.md) |
| Understand local quality checks | [Network Doctor](docs/network-doctor.md) |
| Build and test changes safely | [Contributing](CONTRIBUTING.md) |
| Understand implementation and verification status | [Implementation](docs/IMPLEMENTATION.md) |
| Maintain an official release | [Release process](docs/RELEASE.md) |

Use the pinned toolchains and change-scoped checks in the contribution guide. Compile-only builds and deterministic tests are safe workstation checks; installing development packages or exercising VPN lifecycle requires the designated isolated environments. A successful build is not installation, leak, or performance evidence.

## Upstream and license

Protocol behavior follows [Diniboy1123/usque](https://github.com/Diniboy1123/usque). This repository keeps a snapshot of that client in `oracle/go` for interoperability tests. The Flutter UI and Rust engine are new code. Upstream copyright stays in the license.

Source is [MIT](LICENSE.md). Third-party components keep their own licenses.
