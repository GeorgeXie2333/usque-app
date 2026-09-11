# VPN Gate implementation validation

This is the local development record for `codex/vpngate-exit`, based on
`80ef61ad66b9f65f92f2a326e1ecf1f01f058096`, with uncommitted implementation
changes, on 2026-09-12 (Asia/Singapore). It is not a signed candidate report,
protected-runner result, public-server availability test, or release approval.
Later source changes require their own applicable checks.

## Environment and commands

The host was a normal Windows development machine. Rust 1.97.1, Flutter
3.44.7 at `84fc5cbb223bc12f83d65b647ff8a56caf779ffd`, Android NDK
29.0.14206865 and SDK CMake 3.22.1 matched the repository pins. A verified
Python 3.10+ executable from the bundled runtime was used where `python` was
not on PATH. No release signing identity, installed VPN, route, WFP filter,
interface DNS setting or system proxy was exercised.

Commands below ran from the repository root unless a directory is stated.
Toolchain executables were resolved from the pinned local SDKs, as required
by [CONTRIBUTING.md](../CONTRIBUTING.md).

| Check | Command | Result |
| --- | --- | --- |
| Rust formatting | `cargo fmt --all --check` | Passed |
| Windows Rust lint | `./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed, workspace and all targets, locked dependencies |
| Windows Rust tests | `./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | Passed: 963 tests; 2 existing credential-dependent live proxy tests ignored |
| Native source and notice lock | `python tool/check_openvpn_sources.py` | Passed: all 5 packages, inventories, reviewed patch hashes and application notice |
| Native test-feature lint | `cargo clippy -p usque-openvpn --all-targets --features interop-test --locked -- -D warnings` | Passed in the helper-initialized Windows native environment |
| Native memory interoperability | `cargo test -p usque-openvpn --features interop-test --locked` | Passed: 6 tests |
| Android Rust lint | `./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed with the pinned NDK |
| Python lint | `python -m ruff check tool` | Passed with Ruff 0.16.0 |
| Python formatting | `python -m ruff format --check tool` | Passed |
| Python tests | `python -m unittest discover -s tool -p "test_*.py" -v` | Passed: 76 tests |
| Protobuf lint | `buf lint` | Passed with Buf 1.72.0 |
| Protobuf formatting | `buf format --exit-code --diff` | Passed |
| Protobuf compatibility | `buf breaking --against '.git#branch=main'` | Passed against local `main` |
| Actions validation | `actionlint -no-color` | Passed with actionlint 1.7.12 |
| Go dependencies | `go mod verify` in `oracle/go` | Passed |
| Frozen Go source | `go test ./...` in `oracle/go`; `python tool/verify_oracle_archive.py` at root | Passed; archived packages have no Go test files and all 41 frozen source files match |
| Repository policy | `python tool/check_repository_policy.py` | Passed |
| Patch whitespace | `git diff --check` | Passed |

The complete Flutter sequence ran in `apps/usque_gui`:

```powershell
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub
& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .
flutter build windows --release --no-pub
```

Locked resolution, format, analysis and all 440 tests passed, including Windows
goldens. The two Proxy baselines and two new VPN Gate baselines were visually
reviewed. Tests cover draft selection, failed saves, refresh cancellation,
country filtering, paging, session status, keyboard/TV focus and Chinese text
at 200% scale. The Windows Flutter release build passed.

Android configuration and Kotlin checks ran without installing an APK:

```powershell
# In apps/usque_gui, after the locked Flutter resolve above:
flutter build apk --debug --config-only --no-pub
# In apps/usque_gui/android:
./gradlew.bat --no-daemon :app:ktlintCheck
./gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug
```

Configuration, ktlint and Android lint passed; 187 Kotlin unit tests passed
with no failures or skips. Gradle invoked the pinned Rust helper to rebuild
debug JNI libraries for arm64-v8a, armeabi-v7a and x86_64. These generated
libraries remain ignored. This is not release APK validation or a device run.

The Windows Rust release command also passed:

```powershell
./tool/build_windows_rust_release.ps1 -Variant x64-v2
```

The fresh engine, Agent, update and uninstall executables and the checked-in
Wintun DLL were assembled into the Flutter build directory according to
`.github/workflows/build.yml`. The required output inventory and
`./tool/verify_pe_architecture.ps1 -Root <assembled-directory> -Architecture x64`
passed for all 8 PE files. None of these executables was installed or used to
change networking. No MSI or release APK was built.

## What the deterministic coverage establishes

- The downloaded mirror snapshot was also checked offline using
  `cargo run -p usque-transport --example vpngate_validate --locked -- target/vpngate-live-directory.json`
  in the initialized native environment. All 100 records validated; 83 TCP
  configurations reached the native dial request and 17 UDP configurations were
  classified as unsupported. The validator stops before dialing, so this result
  says nothing about those servers' reachability or handshake success. The
  downloaded snapshot remains an ignored local input, not a source fixture.
- Downloader fixtures exercise Raw priority, timeout, complete-response CDN
  racing, invalid winners, bounded responses, cancellation, coalescing, the one
  WARP fallback and temporary-session close. Directory tests cover nullable
  required fields, IDs, hashes, limits, supported TCP profiles, country counts,
  pinned snapshots and stale selection rejection. Two 256-case property tests
  exercise arbitrary and mutated parser input.
- The memory peer negotiates real TLS 1.2 and AES-128-CBC/SHA1. It relays 72
  UDP-shaped IP packets while client-initiated rekeying completes, observes at
  least two TLS handshakes and data key IDs, and checks cancellation plus CA and
  authentication rejection. It uses no OS socket or TUN. This Core-to-Core peer
  is not evidence of interoperability with an OpenVPN 2 server or VPN Gate's
  public volunteers.
- Rust transport tests exercise bounded framing, partial reads/writes, flow
  admission, stale generations, unavailable address families, final DNS and
  explicit direct routing. A memory-only DNS test confirms a VPN-pushed private
  resolver stays on the final channel with LAN bypass enabled and no physical
  socket protection call.
- Agent doubles check early transition protection, deferred final addresses,
  failure retention, authenticated lease takeover, stale-owner cancellation
  and rollback journal behavior. Kotlin tests check final network conversion,
  DNS host routes and interface restart decisions. Neither establishes the
  behavior of a real OS network stack or device lifecycle.

## Blocked local static check

The aggregate `pwsh -NoProfile -File tool/check_source.ps1` was attempted after
initializing the Windows native environment. It reached PSScriptAnalyzer after
the preceding checks passed, but could not import the installed, pinned
PSScriptAnalyzer 1.25.0 module. The local software restriction policy rejected
`ScriptAnalyzer.format.ps1xml`. Its Microsoft signature was valid; a separate
import attempt hit the same policy error.

Therefore both required PSScriptAnalyzer passes remain **blocked / not run**:

```powershell
Invoke-ScriptAnalyzer -Path tool -Recurse -Settings tool/PSScriptAnalyzerSettings.psd1
Invoke-ScriptAnalyzer -Path tool -Recurse -IncludeRule PSUseCorrectCasing
```

The aggregate gate did not pass. No execution policy, software restriction,
module content or gate was weakened to bypass the error. The two passes and
aggregate check still need a host where the pinned module loads normally;
existing CI continues to require them. Buf was run separately and passed.

## Isolated and other checks not run

| Environment / scope | Status | Remaining evidence |
| --- | --- | --- |
| `usque-snapshot-vm` | `not_run` | Real Wintun/WFP, routes, DNS, connected crashes, Engine/Agent restart, cleanup and interface handoff |
| `usque-android-device` | `not_run` | Device/emulator TUN handoff, process death, Always-on, Lockdown, network switch, reboot and Android TV lifecycle |
| `usque-network-observer` | `not_run` | Same selected exit for proxied TUN/SOCKS5/HTTP, IPv4/IPv6 and DNS observation, explicit direct exceptions, Gate/WARP loss, failed switching and address changes |
| `usque-performance-lab` | `not_run` | Sustained throughput, resource limits and latency under TCP loss |
| Public VPN Gate and controlled OpenVPN 2 server connections | `not_run` | Live server compatibility, availability and long-running renegotiation |
| Hosted CI, Windows ARM64 and release packaging | `not_run` locally | Hosted target builds and package-specific verification |

The controlled exit matrix must cover CONNECT-IP over H3/H2 and L4, each
frontend and their combinations. Capture independent observations for ordinary
proxy traffic and explicit direct exceptions; include unsupported IPv6,
resolver changes, same-node retries, failed node changes and explicit disconnect.
Use the isolation and sanitized-evidence contracts in
[RELIABILITY_TESTING.md](RELIABILITY_TESTING.md). Missing protected-runner
evidence is never a pass and is not a publication prerequisite.
