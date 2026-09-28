# WARP WireGuard validation / 实现验证

Date: 2026-09-23. Environment: Windows x64 development workstation.
Baseline: `93ba595a98953df81343b86b663bbfcf32729f4e`; candidate: that commit plus
the uncommitted WARP WireGuard implementation. This is local working-tree
evidence, not an immutable release candidate or package certification.

The [guide](WARP_WIREGUARD.md) describes the workflow and implementation
contract. The [measurement manifest](WARP_WIREGUARD_MEASUREMENTS.json) identifies
changed source/test files and the measured final artifacts by SHA-256.

## Completed checks

| Check | Result |
| --- | --- |
| Windows helper Rust Clippy, workspace/all targets, locked | Passed |
| Windows helper Rust tests, workspace/all targets, locked | 1,248 passed; 8 explicitly ignored |
| Windows helper x64-v2 release build | Passed |
| Android arm64-v8a Rust Clippy, pinned NDK/CMake, locked | Passed |
| Flutter locked resolution, format and analysis | Passed |
| Flutter complete Windows widget/golden suite | 653 passed |
| Windows Flutter release build and plugin junction preparation | Passed |
| Android debug configuration only | Passed; no APK assembled or installed |
| Kotlin ktlint, unit tests and Android lint | Passed; 221 tests, no failures/errors/skips |
| Aggregate source checks | Passed, including Ruff 0.16.0 and PSScriptAnalyzer 1.25.0 |
| Buf 1.72.0 lint and format | Passed; appended wire fields also covered by Rust/Dart snapshots |
| Repository policy and diff whitespace | Passed |

The eight ignored Rust tests require isolated Wintun loading, supplied external
profiles/enrolled live credentials, or an explicit live endpoint. They are not
included in the passed count and are not evidence of live interoperability.

New deterministic coverage exercises source separation and old encrypted record
reading; endpoint overrides without rewriting credentials/revisions; bounded
request parsing; distinct IP/port enumeration; encrypted pagination and country
filtering; partial-block reuse, failed-attempt storage and interrupted commits;
manual recovery/cancellation and outer reconnection invalidation; Android
metadata bounds and secret filtering; appended capabilities; draft editing,
generation without automatic selection, locale completeness, 200% text and TV
remote selection. Existing WireGuard authentication/data/AllowedIPs/rekey,
userspace UDP/DNS and lifecycle regression suites also passed. These deterministic
tests do not simulate every behavior of Cloudflare's live service.

All changed golden images were visually reviewed on Windows with the pinned SDK
and fonts. Coverage includes the four-source picker, icons, retained VPN Gate
layouts, the WARP phone page, and English/light plus Chinese/dark discovery
results. Screenshot baselines were updated only for the new source and intended
layout changes, then the complete suite passed without `--update-goldens`.

## Artifact size

Before editing, the clean baseline was built with the same Windows helper,
target, default WireGuard feature and release settings as the candidate. Flutter
used the same pinned SDK and locked dependencies. These are uncompressed files,
not compressed MSI/APK sizes; no numeric size threshold is imposed.

| Artifact | Baseline bytes | Candidate bytes | Increment |
| --- | ---: | ---: | ---: |
| Windows x64-v2 `usque-engine.exe` | 15,646,720 | 16,544,768 | 898,048 bytes (5.74%) |
| Windows GUI `usque.exe` | 262,144 | 262,144 | 0 |
| Windows Flutter `data/app.so` | 10,503,056 | 10,584,976 | 81,920 bytes (0.78%) |

The three measured files increase by 979,968 bytes together. This excludes the
small license asset/manifest and packaging overhead. `base64` reuses an existing
locked workspace dependency; `tempfile` is added only as a transport test
dependency. No new dependency versions, Go runtime, additional network stack,
AmneziaWG, terminal interface or download-speed module were introduced.
Android JNI/AOT and compressed installer size comparisons are `not_run`.

## Commands

Rust 1.97.1 and Flutter 3.44.7
(`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`) were verified against the pins.
Android uses NDK 29.0.14206865 and SDK CMake 3.22.1. `flutter` and `dart` below
denote the absolute executables under the SDK resolved from `local.properties`;
the actual host used a Flutter 3.44.7 SDK at `<flutter-sdk>/bin`.
Python was the verified bundled runtime. Commands are scoped as in
[Contributing](../CONTRIBUTING.md); build outputs and logs remain ignored.

From the repository root, in PowerShell:

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

From the root, the aggregate was run in the same PowerShell session after one
successful Windows helper initialization and with the pinned tools on PATH:

```powershell
pwsh -NoProfile -File tool/check_source.ps1
buf lint
buf format --exit-code --diff
python tool/check_repository_policy.py
git diff --check
```

## Not run / 未运行

| Validation | Status and reason |
| --- | --- |
| Live Cloudflare registration and endpoint scanning | `not_run`: no dedicated MASQUE test configuration supplied through `USQUE_LIVE_CONFIG` |
| Live verification of selected-user versus scan-identity countries | `not_run`: requires live enrollment and two real tunnels; multiple countries are not a success requirement |
| Windows VPN/Wintun, routes, DNS/WFP mutations and restoration | `not_run`: requires the isolated snapshot VM and independent management channel |
| Externally observed IPv4/IPv6/DNS leak and fail-closed behavior | `not_run`: requires the network-observer environment |
| Android/TV process, Doze, Always-on, Lockdown and recovery on device | `not_run`: requires a dedicated device or isolated emulator |
| Controlled long-run full-scan resource sampling | `not_run`: requires the performance environment and live network |
| Windows ARM64, all-three-ABI JNI builds, Android release AOT | `not_run`: outside this change's required local compile matrix |
| MSI/APK production, installation, signing or publication | `not_run`: outside this delivery's scope |

真实网络和隔离环境验收未运行，不视为通过。此次交付包含源码、确定性测试、
用户指南和编译验证；没有制作安装包、安装程序、修改系统网络状态或发布版本。

## User-supplied icon follow-up / 用户提供图标（2026-09-23）

The implementation record and measurement manifest above describe the earlier
snapshot with the cloud icon. This follow-up uses the user's supplied SVG in
[`warp-wireguard.svg`](../apps/usque_gui/assets/icons/warp-wireguard.svg),
preserving its path and `viewBox="120 80 250 330"`. The existing `ChainSourceIcon`
applies theme/explicit colors and proportional sizing through `flutter_svg`.
The asset is 335 bytes and requires no additional dependency.

Eight affected golden images were reviewed at their original dimensions. The
pixel changes are confined to the WARP source icon regions, including 18/20/24/32
pixel sizes, selection, disabled/focus colors, phone/desktop and light/dark
themes. The earlier feature-size measurements remain historical and were not
rewritten for this icon change.
