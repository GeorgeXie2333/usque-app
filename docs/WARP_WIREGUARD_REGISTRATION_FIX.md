# WARP WireGuard registration fix / 注册修复验证

Date: 2026-09-24. Environment: Windows x64 development workstation.
Baseline: `fb66778d4369a411a343a9839fd0e1b2343f3773`; candidate: that commit plus
the uncommitted registration, headless-session and error-reporting fixes.
This record does not identify an immutable complete source snapshot or a
release candidate. Earlier feature and icon results remain in the
[original validation record](WARP_WIREGUARD_VALIDATION.md).

The reported black-box symptom was failure to generate a configuration or
start an endpoint scan on Android while disconnected. Configuration import
worked. The old generic error did not identify which stage failed, so these
source fixes do not establish the cause observed on that particular device.

## Changes and deterministic evidence

- Registration now follows the pinned [wgcf reference](WARP_WIREGUARD_UPSTREAM.md):
  API `v0a5641`, matching Android headers/body, a fresh Curve25519 key, then
  authenticated `GET /reg/{id}`. POST is allowed to omit the configuration.
  The fixture verifies both requests and that each generated private key
  matches its registered public key; identities remain independent.
- The endpoint parser no longer imports `:0` from the API's IPv4/IPv6 address
  entries. It uses the returned ports (default 2408), or the explicit `host`
  endpoint. IPv4, IPv6, hostname, malformed address and invalid port cases
  are covered. Generation and initial scan-identity registration share this
  implementation, so the parser defect affected both operations.
- The API-only TLS profile reuses existing BoringSSL and matches wgcf's
  initial ClientHello fixture after removing its random bytes. In-memory
  client/server handshakes accept a trusted valid certificate and reject
  an untrusted issuer, wrong hostname or expired certificate. WebPKI still
  verifies the fixed API hostname, validity, server usage and CA trust.
  Other private HTTPS requests retain their existing Rustls profile.
- Enrollment has a 15-second request budget, including up to 10 seconds for
  connection setup. Virtual-clock tests cover six-second responses, timeout
  and prompt cancellation. Endpoint probes retain their five-second HTTPS
  budget. HTTP status and transport failures carry bounded stage codes;
  registration does not automatically retry or replace keys after failure.
- Headless MASQUE profiles clear local-listener authentication fields because
  they start no listener and do not load listener passwords. A real userspace
  runtime over in-memory packet queues verifies startup and shutdown with a
  saved username and absent password, without changing the saved profile.
- Android preserves allowlisted native failure codes across a cold service
  binding. Flutter distinguishes missing credentials, temporary outer-session
  failure and registration failure in all supported language catalogs.
  Kotlin and widget tests verify delivery and suppression of arbitrary
  exception text. The JVM binding helper now flushes pending WARP requests
  as the production `onServiceConnected` callback already did.

Registration still runs through the explicit MASQUE underlay; it creates no
OS tunnel or local proxy listener and has no physical-network HTTP/DNS
fallback. Cancellation retains the existing worker and temporary-session
cleanup path. Tokens, keys and response bodies are absent from user-visible
errors and diagnostic codes. No dependency or lockfile changed; the wgcf MIT
notice is bundled in the license screen. No Go runtime or additional network
stack was introduced. This follow-up did not repeat artifact size comparisons.

## Completed checks

| Check | Result |
| --- | --- |
| Rust format; Windows helper locked Clippy | Passed |
| Windows helper locked workspace tests | 1,255 passed; 8 ignored under their existing conditions |
| Windows helper x64-v2 release build | Passed |
| Android arm64-v8a Rust Clippy | Passed with pinned NDK/CMake |
| Flutter locked resolution, format and analysis | Passed |
| Complete Flutter Windows widget/golden suite | 654 passed; no golden baselines changed |
| Windows plugin junction preparation and Flutter release build | Passed |
| Android debug configuration only | Passed; no APK assembled or installed |
| Kotlin ktlint and unit tests | Passed; 223 tests, no failures/errors/skips |
| Android lint | Passed |
| Aggregate source checks, including Buf lint/format | Passed; final Kotlin formatting check repeated after its test-helper correction |
| Repository policy and diff whitespace | Passed |

The ignored Rust tests require external profiles/credentials, an explicit live
endpoint or isolated Wintun loading. They are excluded from the passed count.
Gradle's debug test dependencies also compiled JNI libraries for its three
configured ABIs; those generated libraries remain untracked.

## Commands

Commands follow [Contributing](../CONTRIBUTING.md). Rust is 1.97.1; Flutter is
3.44.7 (`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`), resolved from
`local.properties`. `flutter` and `dart` below mean the absolute executables
under the Flutter 3.44.7 SDK's `<flutter-sdk>/bin`. Python denotes the
bundled verified runtime, invoked with `-X utf8`. Logs and artifacts remain
under ignored build paths.

From the repository root:

```powershell
cargo fmt --all --check
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2
& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy
```

From `apps/usque_gui`:

```powershell
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub
& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .
flutter build windows --release --no-pub
flutter build apk --debug --config-only --no-pub
```

From `apps/usque_gui/android`:

```powershell
.\gradlew.bat --no-daemon :app:ktlintCheck
.\gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug
```

The aggregate ran from the root with the pinned tools on PATH after the
Windows helper initialized its native environment in the same shell:

```powershell
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy -Package usque-transport
pwsh -NoProfile -File tool/check_source.ps1
python -X utf8 tool/check_repository_policy.py
git diff --check
```

## Not run / 未运行

| Validation | Status and reason |
| --- | --- |
| Live Cloudflare registration, generated-profile traffic and endpoint scanning | `not_run`: no dedicated MASQUE test configuration supplied through `USQUE_LIVE_CONFIG`; fixtures do not prove live service compatibility |
| Android/TV disconnected registration and scanning on device, process/Doze/Lockdown recovery | `not_run`: no dedicated device or isolated emulator |
| Windows VPN/Wintun, platform changes and restoration | `not_run`: requires the isolated snapshot VM and independent management channel |
| Externally observed IPv4/IPv6/DNS leak behavior | `not_run`: requires the network-observer environment |
| Full-scan performance and country diversity | `not_run`: requires live network/performance infrastructure; multiple countries are not an acceptance requirement |
| Package creation, installation, signing or publication | `not_run`: outside this fix's scope |

已修复注册响应与端口解析、临时会话的凭据依赖，并补充可区分失败阶段的错误
提示。真实 Cloudflare 注册和 Android 真机复测尚未运行，不能据此断言用户设备
上的问题已经通过黑盒验证。此次仅执行安全的源码检查、内存测试和编译验证。
