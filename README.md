<p align="center">
  <img src="assets/branding/usque-readme-banner.png" alt="Usque — unofficial client compatible with Cloudflare WARP" width="100%">
</p>

<p align="center">
  <a href="README.zh-CN.md">简体中文</a>
</p>

# Usque

Usque is an open-source, native GUI client for Consumer Cloudflare WARP. Flutter renders the interface and a memory-safe Rust engine owns MASQUE, CONNECT-IP, DNS, proxying, and connection state. It does not use a WebView.

> [!IMPORTANT]
> Usque GUI is under active development. No public beta binary is available yet. The shared Rust data channel, Android VPN slice, Windows Agent, and MSI authoring now exist, but real-device, clean-install, performance, signing, and isolated leak-test gates are still incomplete. Do not treat the repository head as a production VPN.

Usque is an independent project. It is not affiliated with, sponsored by, or endorsed by Cloudflare. Cloudflare and WARP are trademarks of Cloudflare, Inc. Use of Consumer WARP remains subject to Cloudflare's applicable terms and privacy policy.

## Beta target

The first public beta will be released only when every target below passes installation, interoperability, and leak-prevention tests.

| Platform | Native packages | Minimum OS | Compatibility package |
| --- | --- | --- | --- |
| Windows | ARM64, x86-64-v2 | Windows 10 22H2 (build 19045) | x86-64-v1 for older CPUs |
| Android / Android TV | arm64-v8a, armeabi-v7a, x86_64, Universal APK | Android 8.0 / API 26 | Universal APK |

macOS remains a future source target, but it is not built, tested, packaged, or used as a release gate for `v0.1.0-beta.1`. Linux, iOS, Zero Trust, application stores, a public CLI, and multipath bandwidth aggregation are not in the first-beta scope.

## What the beta will provide

- Consumer WARP registration or manual WARP Secret entry.
- Native VPN mode with full-tunnel routing, DNS through the tunnel, Kill Switch, LAN access, and CIDR bypass rules.
- Mutually exclusive SOCKS5 and HTTP Proxy modes.
- Automatic HTTP/3 over QUIC with HTTP/2 over TLS fallback.
- IPv4/IPv6 Happy Eyeballs selection with one active path, not bandwidth aggregation.
- Multiple profiles with one active profile.
- Exit IPv4, IPv6, and location checks through the tunnel using [IP.SB](https://ip.sb/api).
- Local diagnostics with secret redaction and no automatic telemetry or upload.
- English and Simplified Chinese, light/dark themes, keyboard and screen-reader support, and Android TV D-pad navigation.

## Default network settings

| Setting | Default |
| --- | --- |
| Endpoint IPv4 | `162.159.198.2` |
| Endpoint IPv6 | `2606:4700:103::2` |
| Port | `443` |
| SNI | `www.visa.cn` |
| Transport | Auto: HTTP/3, then HTTP/2 |
| MTU | `1280` |
| Fallback DNS | `1.1.1.1`, `2606:4700:4700::1111` |
| SOCKS5 | `127.0.0.1:1080`, `[::1]:1080` |
| HTTP Proxy | `127.0.0.1:8080`, `[::1]:8080` |

Advanced users can change these values and restore them with one action. A non-loopback proxy listener is intentionally unauthenticated and always displays a prominent security warning.

## Installation

There are no installable releases yet. When the public beta is ready, packages will appear only on this repository's GitHub Releases page.

Windows packages will use a stable self-signed identity. Verify the published SHA-256 checksum and certificate fingerprint before installing, then follow the operating-system warning flow. Android APKs will be signed by one fixed release keystore but will not initially be registered with Android Developer Console. Advanced sideloading or ADB may therefore be required as Android developer verification expands. Usque will not be distributed through Microsoft Store or Google Play.

Updates are never installed automatically. The app can check GitHub prereleases at most once every 24 hours, show a prompt, and open the download page; this check can be disabled.

## Modes

Only one mode can run at a time:

- **VPN** creates a system tunnel and manages routes, DNS, and Kill Switch rules.
- **SOCKS5** supports TCP and UDP, with remote DNS by default.
- **HTTP Proxy** supports CONNECT and ordinary HTTP forwarding.

VPN is the default. Proxy listeners bind only to IPv4 and IPv6 loopback unless an advanced user changes them.

## Security and privacy

- The GUI cannot enable an insecure TLS mode. Endpoint pinning is mandatory.
- WARP Secret, private keys, tokens, device identifiers, license data, and endpoint pins belong in Windows Credential Manager or Android Keystore.
- The first beta treats saved identity material as write-only: it can be replaced or erased, but not revealed, copied, or exported as plaintext.
- Desktop privilege is split between an unprivileged engine and a narrow agent that manages only TUN, routes, DNS, firewall rules, and system proxy state.
- Android runs `VpnService` and the Rust library in an isolated `:vpn` application process.
- The Kill Switch is designed to remain active during reconnects and engine/agent recovery. Beta release is blocked until failure-injection leak tests pass.
- Logs default to INFO and are capped at 7 days or 20 MiB. There is no analytics, telemetry, automatic diagnostic upload, or automatic flag download outside the tunnel.

See [SECURITY.md](SECURITY.md) for vulnerability reporting and the current security maturity statement.

## Repository layout

```text
apps/usque_gui/       Flutter UI and Windows/Android hosts; macOS source is deferred
crates/usque-core/    Profiles, state machine, transport orchestration, probes
crates/usque-protocol RFC 9484 CONNECT-IP codec
crates/usque-ipc/     Versioned protobuf framing
crates/usque-platform Least-privilege platform boundary
crates/usque-engine/  Unprivileged desktop engine process
crates/usque-android/ Android JNI boundary
proto/usque/v1/       Public control and event contract
oracle/go/            Archived upstream Go behavior oracle; never released
tool/                 Reproducible asset and packaging helpers
```

The original Go implementation is preserved under `oracle/go` for protocol comparison, packet-capture reproduction, and regression tests. It is not a shipping CLI.

## Build from source

Pinned toolchains:

- Rust `1.97.1` from `rust-toolchain.toml`
- Flutter `3.44.7` / Dart `3.12.2`
- A platform compiler: Visual Studio Build Tools or Android SDK/NDK

Run the platform-independent checks:

```shell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked

cd apps/usque_gui
flutter pub get --enforce-lockfile
flutter analyze --no-pub
flutter test --no-pub
```

On Windows, Developer Mode can provide Flutter's normal symbolic-link support. If it is intentionally disabled, run `tool/prepare_windows_plugin_junctions.ps1` after `flutter pub get`; it creates only project-local plugin junctions. Android builds invoke `tool/build_android_rust.ps1` from Gradle and require NDK `29.0.14206865`; release builds additionally require a protected release keystore.

The current source tree is a development vertical slice. Windows can establish strictly pinned Cloudflare HTTP/3/QUIC and HTTP/2/TLS data paths and serve SOCKS5 TCP/UDP plus HTTP CONNECT/Forward without creating a TUN. Its least-privilege Agent now has authenticated IPC, Wintun packet rings, transactional routes/DNS/system proxy, a persistent WFP Kill Switch, crash reattachment, and fail-closed WiX MSI authoring. Android owns a real Rust runtime, MASQUE channel, retained-TUN reconnect path, route-exclusion planner, Binder events, Keystore identity, and proxy-only service path. These implementations have unit, mock, compile-time, and loopback proxy coverage, but neither platform has passed the required clean-machine, hardware, 24-hour, performance, and independent leak laboratories. No beta is publishable until those gates pass. Track the milestones in [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md) and the protected release chain in [docs/RELEASE.md](docs/RELEASE.md).

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a change. Any transport change must include oracle interoperability coverage; any route, DNS, TUN, firewall, or lifecycle change must include leak-prevention tests.

## Upstream and license

Usque GUI is forked from [Diniboy1123/usque](https://github.com/Diniboy1123/usque). The Android compatibility behavior is informed by [Abobo7/usque-android](https://github.com/Abobo7/usque-android). Upstream copyright and attribution are retained.

Released source is licensed under the [MIT License](LICENSE.md). Third-party components retain their own licenses.
