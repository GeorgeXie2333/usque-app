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

This checkout describes **v0.2.7**. Check [GitHub Releases](https://github.com/GeorgeXie2333/usque-app/releases) for published versions. The package set has six installers; two additional Windows MSI files are reserved for in-app updates:

| Platform | Minimum OS | Packages |
| --- | --- | --- |
| Windows | Windows 10 22H2, build 19045 | x64-v2 installer EXE or ARM64 installer EXE |
| Android / Android TV | Android 8.0, API 26 | arm64-v8a, x86_64, or armeabi-v7a APK |
| Android / Android TV | Android 8.0, API 26 | Universal APK containing all three ABIs |

Choose the package matching your device architecture. Use the larger universal APK when the Android ABI is unknown. Before installing, compare the package SHA-256 with `SHA256SUMS` and GitHub's asset digest, then verify the signer fingerprint published in the release notes. Stop if any value differs.

Pre-1.0 packages use fixed, project-controlled self-signed certificates. Windows may show an unknown-publisher warning; Android packages are installed outside Google Play. Do not disable antivirus or the firewall, or import certificates from unofficial packages, to bypass a warning.

See [Installation and removal](docs/INSTALLATION.md) for upgrades, uninstall, recovery, and Android developer-verification details, and [Code signing](docs/CODE_SIGNING.md) for official identities. Updates require confirmation before downloading and use the platform installer; there is no unattended installation.

## First connection

1. Install a [verified official package](docs/INSTALLATION.md#verify-before-installing) and open Usque.
2. Complete the first-run permissions and terms steps. Register a Consumer WARP account, optionally with a WARP License Key. Usque does not accept new WARP Secret imports.
3. Choose how applications should connect under **VPN and local proxies**, then connect from Home. Android requests VPN consent when VPN is first enabled; SOCKS5/HTTP-only use does not require it.

| Connection option | When to use it |
| --- | --- |
| VPN/TUN | Route system traffic through the tunnel, with your bypass and Android per-app rules. |
| SOCKS5 | Give compatible applications a local TCP/UDP proxy; remote DNS is the default. |
| HTTP proxy | Give compatible applications a local HTTP proxy, including HTTPS through CONNECT. |
| Windows system proxy | Point Windows proxy settings at Usque's HTTP proxy; HTTP output must be enabled. |

VPN, SOCKS5 and HTTP are enabled by default; Windows system proxy is off.
They share one WARP connection and can run together. Disabling all outputs keeps
the transport connection but stops providing these application connection options.
Each account stores its own credentials; only one account connects at a time.
Network settings are shared by all accounts.

## Features

- Optional [chain proxy](docs/CHAIN_PROXY.md): open **Proxy → Chain proxy** for
  **OpenVPN**, **WireGuard**, [**WARP via WireGuard**](docs/WARP_WIREGUARD.md),
  or **VPN Gate**, in that order.
  Import files or paste configurations, save several, then select and apply one.
  VPN, SOCKS5 and HTTP share the exit; explicit direct rules still apply. Off by default.
- Opt-in [experimental L4 mode](docs/L4_PROXY.md) proxies TCP over HTTP/3.
  Without an OpenVPN TCP chain, it does not forward ordinary UDP; applications that need UDP
  may not work. Auto does not select L4.
- Automatic HTTP/3 connections with HTTP/2 fallback. IPv4 and IPv6 connection
  attempts help find a reachable endpoint; supported H3 network changes can
  migrate the connection. See [path behavior](docs/h3-path-infrastructure.md).
- Full-tunnel VPN, tunneled DNS, Kill Switch, LAN access and custom CIDR bypass rules.
- Optional country-based direct routing. Download the selected countries' GeoIP
  data and the global GeoSite catalog separately. Usque uses domain rules when
  the name is visible, otherwise IP rules; unknown destinations stay in the tunnel.
- Local [network diagnostics](docs/network-doctor.md) and a Network Quality page
  showing latency, packet-loss readings and availability, queues and 60-second
  trends. Standard checks read local state; Deep checks send test requests only
  after confirmation.
- Windows tray, single-instance activation, start on boot and close-to-tray;
  Android Quick Settings tile, launcher shortcuts, boot recovery and TV navigation.
  Twenty-one languages, with light and dark themes.
- Consumer WARP Secret export to a file you choose, after confirmation. Usque
  cannot import that file to restore the account after reinstalling.

Android **Per-app proxy** applies to the whole app, across accounts. When off,
all apps use the VPN. When on, only selected apps do; newly installed apps must
be selected. With Android **Block connections without VPN**, unselected apps
are blocked instead of bypassing the tunnel.

## Privacy and limits

- Usque requires the WARP server's public key to match the registered key.
  There is no option to skip this check. Credentials stay in Windows Credential
  Manager or Android Keystore. Windows uses a separate Agent for privileged
  network operations; Android runs the VPN in a dedicated process.
- Proxies listen on loopback by default. SOCKS5 and HTTP support optional
  username/password authentication; without configured credentials they require
  no authentication. Proxy-only mode does not provide a system-wide VPN Kill Switch.
- Diagnostics are generated locally and redacted. There is no usage analytics
  or automatic upload, and quality history stays in memory. Logs default to INFO
  and are limited to 7 days or 20 MiB. Do not post credentials or raw diagnostic
  bundles in public Issues; report vulnerabilities through [SECURITY.md](SECURITY.md).
- Android's in-app Kill Switch cannot protect traffic after the VPN process dies.
  VPN Gate terminal failures also end the connection. To keep apps blocked after
  the VPN ends, enable both system **Always-on VPN** and **Block connections
  without VPN**. See [Android setup](docs/INSTALLATION.md#android-and-android-tv).
- Usque does not combine several paths for extra bandwidth. Some quality readings,
  including HTTP/2 packet loss and PMTU, are unavailable. A local diagnostic pass
  does not establish that no traffic leaked or that performance improved.

### DNS privacy

Country-based direct rules use **System** DNS by default: matching domain queries
go to the DNS servers on your current network, outside the VPN. You can instead
choose **DoH** or **DoT** and supply an encrypted resolver's name and IP addresses.
That resolver receives the queries; connection failures do not switch them to
plaintext DNS. See [configuration steps and examples](docs/encrypted-direct-dns.md).

Other remote VPN queries use the final tunnel's DNS: WARP normally, or the selected chain exit
when enabled. Explicit local and proxy DNS settings still apply. Apps that use
their own encrypted DNS hide domain names from Usque, so direct routing uses IP
rules. Rule downloads also respect Android Lockdown and any remaining Windows
Kill Switch while disconnected.

### Experimental and unsupported features

[Zero Trust enrollment](docs/ZERO_TRUST_EXPERIMENTAL.md) is experimental. It uses
an organization identity for the MASQUE Internet tunnel and does not provide full
Cloudflare One Client compatibility. macOS source is retained but not built or
released. iOS, store distribution and a public CLI are outside this release's scope.

## Default network settings

| Setting | Default |
| --- | --- |
| Consumer endpoint IPv4 | `162.159.198.2` |
| Consumer endpoint IPv6 | `2606:4700:103::2` |
| Port / SNI | `443` / `speed.cloudflare.com` |
| Transport | Auto: HTTP/3, then HTTP/2 |
| HTTP/3 congestion control | `cubic`; BBRv2, experimental BBRv3, and `reno` are selectable |
| QUIC UDP receive buffer | [2 MiB target](docs/UDP_RECEIVE_BUFFER.md) on Windows/Android for H3 and L4; actual capacity is OS-dependent |
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

First-party source is [MIT](LICENSE.md). Third-party components keep their own
licenses. The optional [chain proxy](docs/CHAIN_PROXY.md) embeds OpenVPN 3
Core under MPL-2.0 and Mbed TLS under Apache-2.0. Corresponding source, reviewed
patches and license texts are included in `third_party`; the application exposes
the notices from its VPN Gate page. WireGuard uses BoringTun 0.7.1 (BSD-3-Clause),
and local SVG icons use flutter_svg 2.3.0 (MIT); their notices are in the app's license registry.
