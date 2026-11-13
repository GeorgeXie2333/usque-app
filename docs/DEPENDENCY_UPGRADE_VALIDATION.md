# Dependency upgrade validation, 2026-11-12

These checks used the `dev` working tree based on
`0451d50de7e4eec9ff5e79f43f3571e965f65d84`, including the preceding
[quiche upgrade](QUICHE_UPGRADE_VALIDATION.md) and the five changes below.
The tree was not committed, so this is local validation, not an immutable
release-candidate attestation. Historical validation records retain their
original versions, hashes and results.

## Versions and integration

| Dependency | Previous resolved version | New version |
| --- | --- | --- |
| webpki-roots | 1.0.11 | 1.0.12 |
| ts_netstack_smoltcp | 0.4.0 | 0.7.1 |
| OpenVPN 3 Core | 3.11.7 | 3.11.8 |
| flutter_svg | 2.3.2 | 2.3.4 |
| shared_preferences_android | 2.4.27 | 2.5.5 |

The netstack upgrade consolidates upstream's former core/socket crates. The
local buffer limits, CUBIC selection, per-family TCP packet budgets, listener
ownership and cancellation cleanup were rebased onto the complete published
crate. Upstream's half-close fix and request-completion policy are retained.
See [patch provenance and standalone tests](../third_party/ts_netstack_smoltcp-0.7.1/PATCHES.md).

The OpenVPN archive and source subset were verified, and the existing memory
transport, TLS, CBC and MSS patches were rebased. Upstream's certificate key
usage handling replaces the overlapping local workaround. The memory-only
interop suite now also rejects invalid certificate-usage overrides without
publishing connection or packet events. See the
[source lock](../third_party/openvpn3-3.11.8/SOURCE.md) and
[patch inventory](../third_party/openvpn3-3.11.8/USQUE-PATCH.md).

The Android preferences plugin now uses JNI for both legacy and async APIs.
The custom plugin registry includes `JniPlugin`. Preferences work leases wrap
all 24 public suspend methods before the delegate queues IO, and remain held
through completion, failure or cancellation. Detach rejects new calls while
accepted calls finish. The existing synchronous `apply` adapter still covers
legacy migrations and the optional SharedPreferences backend; persisted keys
and formats remain upstream-compatible. Obsolete BinaryMessenger tracking and
its tests were replaced with JNI admission and completion regressions.

`UsqueFlutterFragment` supplies an unstarted engine so MainActivity finishes
plugin registration before Dart can resolve a JNI registrar. It retains the
embedding's arguments and restoration policy and explicitly destroys its owned
engine with the Fragment. Device lifecycle behavior is still `not_run` below.
The Activity's restoration factory also maps the former default FlutterFragment
class to this host when restoring state saved by an older app version.

The targeted Pub update adds jni 1.1.2, jni_util 1.0.0 and package_config 3.0.0;
the Android runtime graph includes DataStore 1.2.1. The JNI module is explicitly
bound to NDK 29.0.14206865 and CMake 3.22.1. Its final CMake caches confirmed
both pins. An initial Gradle resolution provisioned the plugin's default NDK
28.2.13676358 and SDK platform 35 before the override was added; shared SDK
directories were not removed. Gradle now tracks the Rust native sources and
vendored dependencies as build inputs.

All 2,189 pre-existing Gradle artifact checksum entries remained identical.
The 82 added artifacts were reviewed and their SHA-256 values checked against
the resolved cache. Lock generation was explicit; no CI verification policy
was relaxed. The frozen Go oracle and signing configuration were unchanged.

## Workstation checks

The host was Windows x64 with Rust 1.97.1 and the CI-pinned Flutter 3.44.7
(`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`). Flutter was resolved from
`android/local.properties`. Python commands used a verified Python 3.12.14
executable because the global command is a Windows app alias. Windows Cargo
checks used the supported environment initialized by the repository helper.

Commands start at the repository root unless their working directory is noted.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | 2,339 passed; 13 existing opt-in tests ignored |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2` | Release compile passed |
| `cargo test --manifest-path third_party/ts_netstack_smoltcp-0.7.1/Cargo.toml --locked --lib --tests --features std,tokio` | 19 passed, including migrated core and upstream socket tests |
| `cargo test --manifest-path third_party/smoltcp-0.14.0/Cargo.toml --locked --lib` | 676 passed |
| `python tool/check_openvpn_sources.py` | Five native source locks and notices verified |
| `cargo clippy -p usque-openvpn --all-targets --features interop-test --locked -- -D warnings` | Passed |
| `cargo test -p usque-openvpn --features interop-test --locked` | 16 passed; no OS socket or TUN opened |
| `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed |
| `& ./tool/build_android_rust.ps1 -CargoAction build -AbiFilter all` | Debug JNI compiled for arm64-v8a, armeabi-v7a and x86_64 |
| `python -m unittest discover -s tool -p "test_*.py" -v` | 360 run: 222 passed, 138 platform/tool-dependent skips |
| `pwsh -NoProfile -File tool/check_source.ps1` | Passed: Rust, Dart, Kotlin, Ruff, PSScriptAnalyzer and Buf |
| `actionlint -no-color` | Passed |

From `apps/usque_gui`:

| Command | Result |
| --- | --- |
| `flutter pub get --enforce-lockfile` | Passed |
| `dart format --output=none --set-exit-if-changed lib test` | Passed; 377 files unchanged |
| `flutter analyze --no-pub` | Passed |
| `flutter test --no-pub` | 1,711 passed; 49 skipped; includes applicable Windows goldens |
| `flutter build apk --debug --config-only --no-pub` | Passed |
| `flutter build apk --debug --no-pub` | Passed; archive contains both `libdartjni.so` and `libusque_android.so` for all three supported ABIs |

From `apps/usque_gui/android`:

| Command | Result |
| --- | --- |
| `./gradlew.bat --no-daemon :app:dependencies --write-locks` | Passed |
| `./gradlew.bat --no-daemon --write-verification-metadata sha256 help` | Passed |
| `./gradlew.bat --no-daemon :app:ktlintCheck` | Passed |
| `./gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` | Passed; 590 tests, zero failures or skips after replacing channel tests with JNI regressions |

Windows Flutter also completed the full
[required sequence](../CONTRIBUTING.md#windows-flutter-and-runner). The reviewed
engine was restored with:

```powershell
python tool/flutter_windows_release_engine.py restore --architecture x64 --destination target/v034-release/engine-x64 --commit 0451d50de7e4eec9ff5e79f43f3571e965f65d84
```

From `apps/usque_gui`, the local-engine arguments were:

```powershell
$engineRoot = (Resolve-Path ../../target/v034-release/engine-x64).Path
$engineSource = (Resolve-Path "$engineRoot/flutter/engine/src").Path
$engineArguments = @("--local-engine-src-path=$engineSource", "--local-engine=host_release", "--local-engine-host=host_release")
```

| Command | Result |
| --- | --- |
| `flutter @engineArguments pub get --enforce-lockfile` | Passed |
| `& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .` | Passed |
| `flutter @engineArguments build windows --release --no-pub --split-debug-info=build/symbols/windows` | Passed |
| `python ../../tool/flutter_windows_release_engine.py verify-staged --dll build/windows/x64/runner/Release/flutter_windows.dll --build-root $engineRoot --architecture x64 --commit (git rev-parse HEAD)` | Passed |

Initial sandboxed Python tests could not create/use some temporary directories;
the complete suite was rerun successfully with normal local tool access.
An initial engine restore refused an existing destination; that directory was
preserved under the ignored upgrade scratch directory before restoring fresh.
No failed attempt was counted as a pass.

The first repository policy run followed generated desktop JNI plugin links
into the external Pub cache. Markdown selection now excludes only the ignored
`apps/usque_gui/linux/flutter/ephemeral` and
`apps/usque_gui/windows/flutter/ephemeral` trees. Regression tests retain checks
for neighboring first-party Flutter documents and other `ephemeral` directories.

OSV's batch API returned zero matches for 525 version queries from the updated
Cargo lock, hosted Pub lock and Gradle release runtime lock on 2026-11-12.
This database lookup does not cover every native vendor or establish absence
of vulnerabilities in local patches.

## Lock identities

| File | SHA-256 |
| --- | --- |
| `Cargo.lock` | `8a6eb627c629146cd24c03e13cb0f2274309046b5c33df37f8f270542ae7071b` |
| `third_party/ts_netstack_smoltcp-0.7.1/Cargo.lock` | `0d73d1918466168870a55f65de8b9113604b3189acc75693be56fbe706fd34ab` |
| `apps/usque_gui/pubspec.lock` | `f1b215f8cfe85750f0c3787d841c5ca51bbb6a50425e467eadf213a5344b61b4` |
| `apps/usque_gui/android/app/gradle.lockfile` | `bf1d1e0e86df955915909cee19415492749cf21942a04017df790f3ff39c9f48` |

## Not run

- Linux, macOS and Windows ARM64 native builds/tests and hosted CI.
- Dedicated Android device/emulator startup, persistence across engine teardown,
  process lifecycle, Doze, Always-on, Lockdown, reboot, upgrade and TV checks.
- Isolated Windows installation, upgrade, uninstall, recovery and Wintun tests.
- External leak observation, live VPN interoperability and controlled performance
  measurements. Memory/loopback tests establish neither leak nor throughput claims.
- Release APK/R8 packaging, MSI creation, official signing and publication. The
  debug APK is compile-only evidence and was not installed; its debug assets do
  not satisfy release-package requirements.

The source, fail-closed cleanup and privacy contracts in
[Contributing](../CONTRIBUTING.md) still apply. No workstation VPN, system
network change or protected-runner workaround was used. Build outputs, archives,
source backups and logs remain under ignored build/scratch directories.
