# Network quality implementation baseline

This document freezes the implementation and evidence baseline required by
`plan.md` PR-00. It describes commit
`3a032e7a56ed3078c58470790893590edbf2062b` on branch `dev`. No runtime or
product behavior is changed by this baseline.

Baseline ID: `usque-pr00-3a032e7a56ed-20260902`

## Repository and toolchains

The worktree was clean before this document was added. `git status --short`
returned no paths; Git only warned that the sandbox could not read the user's
global ignore file.

```text
git rev-parse HEAD
3a032e7a56ed3078c58470790893590edbf2062b

rustc -Vv
rustc 1.97.1 (8bab26f4f 2026-07-14)
binary: rustc
commit-hash: 8bab26f4f68e0e26f0bb7960be334d5b520ea452
commit-date: 2026-07-14
host: x86_64-pc-windows-msvc
release: 1.97.1
LLVM version: 22.1.6

cargo -V
cargo 1.97.1 (c980f4866 2026-06-30)

flutter --version
Flutter 3.44.7
Framework revision 84fc5cbb223bc12f83d65b647ff8a56caf779ffd
Engine hash 7076f47b1d1a3a0edfd8837b17dc15be6abab661
Dart 3.12.2
DevTools 2.57.0

buf --version
1.72.0

python --version
not available on PATH

verified Python 3.10+ executable --version
Python 3.12.13
```

The Flutter SDK path came from the ignored Android `local.properties`, and its
full framework commit matches the CI pin. The path itself is intentionally not
recorded here. The Android SDK installation has NDK `29.0.14206865` (r29) and
CMake `3.22.1`; both match `CONTRIBUTING.md`, Gradle, the Android Rust helper,
and CI. The host is an ordinary x86-64 Windows development workstation, not a
snapshot VM. No dedicated Android test device or isolated emulator was made
available for this baseline.

`cargo metadata --locked --format-version 1` resolves the transport's relevant
locked dependencies as follows:

| Package | Locked version |
| --- | --- |
| `quiche` | 0.29.3 |
| `h2` | 0.4.19 |
| `tokio` | 1.53.1 |
| `rustls` | 0.23.43 |

## Configuration and protocol field map

`CURRENT_SCHEMA_VERSION` is `12`. The live maxima below were read from
`proto/usque/v1/control.proto`; reserved fields remain unavailable and are not
counted as reusable gaps.

| Type | Highest live field/value | Reserved fields | Next append-only value |
| --- | ---: | --- | ---: |
| `Profile` | 16 | none | 17 |
| `Capabilities` | 19 | none | 20 |
| `ConnectionSnapshot` | 16 | 14 | 17 |
| `ConnectionMetrics` | 12 | none | 13 |
| `ControlRequest` | 39 | 19 | 40 |
| `ControlResponse` | 20 | none | 21 |
| `EventEnvelope` | 22 | none | 23 |
| `ConnectionEventType` | 21 | none | 22 |

The plan's proposed append-only numbers are therefore unoccupied at this
commit. A later protocol PR must re-read this table from its actual HEAD before
editing and must raise the configuration schema from 12 to 13 only if 12 is
still current.

The desktop Dart codec is hand-written in
`apps/usque_gui/lib/services/control_codec.dart`; its custom writer and reader
skip wire fields explicitly rather than using generated Dart protobuf types.
Fixed Dart wire goldens are in
`apps/usque_gui/test/desktop_engine_client_test.dart`. Rust v1 wire snapshots
are fixed byte constants and tests in `crates/usque-ipc/src/lib.rs`; there is no
snapshot regeneration command, so new append-only fixtures must be reviewed
and added explicitly.

The compatibility commands are:

```text
cargo test -p usque-ipc --all-targets --locked
buf lint
buf format --exit-code --diff
buf breaking --against ".git#ref=<PR base SHA>" --against-config buf.yaml
cd oracle/go
go mod verify
go test ./...
cd ../..
python tool/verify_oracle_archive.py
```

`oracle/go` is a frozen source and archive reference. It has no update command
and must not be modified for the planned protocol additions.

## Runtime symbol and ownership map

The required impact searches in plan section 3.1 were run against this commit.
The important conclusions are:

- H2 CONNECT-IP calls `h2::client::handshake` directly in
  `crates/usque-transport/src/h2.rs`. Its send path already uses
  `reserve_capacity` and `poll_capacity`; it has no PING owner or explicit
  receive-window builder. `pin_refresh.rs` owns a separate H2 client that must
  not inherit future MASQUE flow-control tuning.
- The H3 setup binds a standard UDP socket, calls `SocketProtector::protect`,
  marks it nonblocking, converts it to Tokio `UdpSocket`, and transfers both
  that socket and the `quiche::Connection` into one H3 actor. The actor is the
  sole quiche owner. It uses `recv_from`/`try_recv_from` and
  `try_send_to`, a 65,535-byte receive buffer, and `SendInfo.to`; active
  migration is disabled. There is no batch-I/O abstraction yet.
- Current H3 endpoint setup does not call `protect_for_target` and therefore
  does not retain a `DirectEgressLease`. This is an explicit baseline
  difference from the plan's future PR-08 target, not an assumed capability.
- `SocketProtector::protect_for_target` currently takes
  `(SocketHandle, SocketAddr, DirectProtocol)` asynchronously and returns a
  `DirectEgressLease`. The lease owns an opaque platform resource, releases it
  on drop, and is documented to outlive all socket I/O. Existing direct TCP and
  UDP paths retain it next to their socket or flow.
- `SocketProtector::network_generation` is the transport-facing generation
  source. Android seeds it from the selected underlying `Network`, then the
  Kotlin physical-network monitor calls JNI `nativeNotifyNetworkChanged`, which
  stores the new generation in the Rust protector. Windows increments its
  protector generation when the Agent's physical-network state or physical DNS
  snapshot changes or becomes unavailable.
- Netstack polls generation every 100 ms while connecting, driving an active
  tunnel, waiting between retries, and reconnecting. Any change currently
  returns the existing physical-network-changed outcome and rebuilds the whole
  channel; there is no same-connection H3 migration command.
- Split DNS obtains numeric physical servers from the protector, uses protected
  UDP/TCP sockets, and rejects responses from an old generation. No DoH or DoT
  resolver or encrypted connection pool exists.
- Diagnostics already use the single engine framework under
  `crates/usque-engine/src/diagnostics`. Flutter has reusable diagnostics
  models/controller/screen and a `Sparkline`, but `AppSection` currently has
  only `home`, `profiles`, `proxy`, and `settings`; Diagnostics is opened from
  Settings rather than being a top-level section.

## Reliability gate baseline

`tool/reliability_gate.py` uses report schema version `1`. Its aggregator is
fail-closed: `failed`, `not_run`, missing, duplicate, unknown, wrong-runner, or
wrong-candidate evidence cannot produce a validated summary. Protected results
remain supplemental and do not gate publication.

The current required gate IDs are:

```text
windows.clean_install
windows.coverage_upgrade
windows.connected_uninstall
windows.engine_crash_recovery
windows.agent_crash_fail_closed
windows.sleep_network_change
windows.route_dns_wfp_proxy_restore
windows.wintun_residual
android.wifi_to_cellular
android.cellular_to_wifi
android.airplane_mode
android.doze_lock_screen
android.flutter_process_reclaim
android.vpn_process_reclaim
android.always_on_lockdown
android.reboot_upgrade_tv_background
network.real_endpoint_h3_h2
network.ipv4_protection
network.ipv6_protection
network.dns_leak
network.kill_switch_leak
network.route_leak
network.direct_rule_scope
performance.informational_baseline
```

## Pre-change performance evidence

No `usque-performance-lab`, protected Android device, external network
observer, or Windows snapshot VM is available on this development workstation.
The development-machine safety contract forbids manufacturing those
environments with labels or environment variables. No raw sample or artifact
was fabricated. Each required pre-change scenario is therefore explicitly
`not_run` with reason code `protected_performance_lab_unavailable`:

| Scenario | Required runs | Status | Artifact |
| --- | ---: | --- | --- |
| `h2-high-bdp-100` | 7 | `not_run` | none |
| `h2-high-bdp-500` | 7 | `not_run` | none |
| `h3-bulk-500` | 7 | `not_run` | none |
| `h3-small-dgram` | 7 | `not_run` | none |
| `h3-lossy` | 7 | `not_run` | none |
| `queue-overload` | 7 | `not_run` | none |
| `pmtu-transition` | 7 | `not_run` | none |
| `network-change` | 7 | `not_run` | none |
| `direct-dns-system` | 7 | `not_run` | none |

Because no performance artifact exists, there is nothing to submit to the
current reliability verifier. These states are non-passes and may not be cited
as performance acceptance evidence by a later PR. Later protected artifacts
must bind to this baseline ID and exact commit, contain every raw sample, and
exclude device identifiers, user paths, SSIDs, IPs, and recognizable endpoints.

## PR-00 validation

The following safe workstation checks completed successfully:

| Command | Result |
| --- | --- |
| `python tool/check_repository_policy.py` | passed with verified Python 3.12.13 |
| `git diff --check` | passed |
| `pwsh -NoProfile -File tool/check_source.ps1` | passed after the required Windows helper initialized MSVC/Ninja |
| `tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | passed |
| `tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | passed for the full locked workspace/all-targets suite |
| `tool/build_android_rust.ps1 -CargoAction clippy` | passed for `arm64-v8a` |
| `flutter pub get --enforce-lockfile` | passed; lockfile unchanged |
| `dart format --output=none --set-exit-if-changed lib test` | 74 files checked, 0 changed |
| `flutter analyze --no-pub` | passed, no issues |
| `flutter test --no-pub` | 176 tests passed |
| `flutter build apk --debug --config-only --no-pub` | passed; no APK was installed |
| `gradlew --no-daemon :app:ktlintCheck` | passed |
| `gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed |
| `ruff check tool` | passed |
| `ruff format --check tool` | 13 files checked, all formatted |
| `python -m unittest discover -s tool -p "test_*.py" -v` | 39 tests passed |
| both required PSScriptAnalyzer passes | 0 findings |
| `buf lint` | passed |
| `buf format --exit-code --diff` | passed |
| `buf breaking --against ".git#ref=f1c9b49d1f452bc597ca40f94cd2d75659f4a5dc" --against-config buf.yaml` | passed against the local `origin/main` merge-base |
| `actionlint -no-color` | passed |
| `go mod verify` in `oracle/go` | all modules verified |
| `go test ./...` in `oracle/go` | passed |
| `python tool/verify_oracle_archive.py` | 41 files matched frozen commit `6aa03fc97d12848dce34eedbd187fb1077b5d1ea` |

The first direct aggregate attempt reproduced the documented Windows native
build trap: plain Cargo selected a Visual Studio 18 generator that CMake 3.22
cannot name. Only the affected `boring-sys` dev build cache was removed, after
confirming the target directory was inside this repository; the prescribed
Windows helper then passed. In the elevated tool environment, Windows software
restriction policy also blocked PSScriptAnalyzer's cosmetic format-data XML.
The final aggregate run loaded the same installed 1.25.0 analyzer and rules from
a task-temporary module copy with only `FormatsToProcess` disabled; both rule
passes and the aggregate script then completed with zero findings. Neither the
repository nor the installed module was modified.

Gradle emitted existing AGP/Kotlin deprecation notices while its tasks returned
`BUILD SUCCESSFUL`; no warning baseline or suppression was added. Protected and
destructive tests remain `not_run` on this workstation and were not replaced
with local simulations. After validation, `git status --short --untracked-files=all`
showed only this document.
