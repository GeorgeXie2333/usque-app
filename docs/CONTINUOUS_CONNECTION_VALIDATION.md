# Continuous connection validation

The implementation keeps the H3 idle keepalive at **30 seconds**. The initial validation below
covers the local working tree based on
`a54ca536fc3a18a5afd645ed7f6b6220ae19c215`, validated on 2026-10-21–22 (Asia/Taipei).

The initial implementation's 72 changed Rust, Kotlin, Dart and protobuf source files have combined SHA-256
`473de5df7cd7dcbc4240df63784f985aa5bd37eb830d7d76323e3bc1a5091396`.
The digest concatenates each repository-relative UTF-8 path, a NUL byte, its
working-tree bytes with CRLF normalized to LF, and a NUL byte, in sorted path
order. It includes new source files and excludes documentation, generated files, logs and binaries.

## Workstation checks

Rust is pinned to 1.97.1. Flutter is 3.44.7 at
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`. Android uses NDK 29.0.14206865 and
SDK CMake 3.22.1. Windows Rust checks use the repository helper and x64-v2.
Linux checks run on Ubuntu 26.04 amd64 in a temporary WSL source snapshot.

Commands below run from the repository root unless a directory is specified.
`python` denotes the verified local Python 3.13 executable. Flutter/Dart resolve
to the pinned SDK, and every Cargo operation uses locked dependencies.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | passed |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | passed: 2,037 tests, 13 ignored optional/live/platform/benchmark fixtures |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2` | passed |
| `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| `flutter build apk --debug --config-only --no-pub` in `apps/usque_gui` | passed |
| `.\gradlew.bat --no-daemon :app:ktlintCheck` in `apps/usque_gui/android` | passed |
| `.\gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` in that directory | passed: 488 tests; lint passed |
| `flutter pub get --enforce-lockfile` in `apps/usque_gui` | passed on Windows and Linux |
| `dart format --output=none --set-exit-if-changed lib test` in that directory | passed: 265 files, no changes |
| `flutter analyze --no-pub` in that directory | passed on Windows and Linux |
| `flutter test --no-pub` on Windows | passed: 1,278 tests, 19 platform skips |
| `flutter test --no-pub --tags golden` on Windows | passed: 91 exact goldens, 19 Linux skips |
| `& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .` | passed |
| `flutter build windows --release --no-pub --split-debug-info=build/symbols/windows` | passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` on Linux | passed |
| `cargo test --workspace --all-targets --locked` on Linux | passed: 1,879 tests, 3 ignored optional/live/benchmark fixtures |
| `cargo build --release --locked -p usque-engine -p usque-agent -p usque-update` on Linux | passed |
| `python -m unittest discover -s tool -p "test_linux_package.py" -v` on Linux | passed: 20 tests, 1 platform skip |
| `flutter test --no-pub --exclude-tags golden` on Linux | passed: 1,186 tests, 1 skip |
| `flutter test --no-pub test/linux_desktop_golden_test.dart` on Linux | passed: 19 exact goldens |
| `flutter build linux --debug --no-pub -t lib/main_preview.dart` | passed |
| `flutter build linux --release --no-pub -t lib/main.dart` | passed |
| `cmake --build apps/usque_gui/build/linux/x64/release --target usque_linux_socket_test`, then `apps/usque_gui/build/linux/x64/release/runner/usque_linux_socket_test` | passed |
| `cmake --build apps/usque_gui/build/linux/x64/release --target usque_proton_verification_test`, then `apps/usque_gui/build/linux/x64/release/runner/usque_proton_verification_test` | passed |
| `cargo test --manifest-path third_party/smoltcp-0.14.0/Cargo.toml --locked --lib` | passed on Windows: 676 tests |
| `cargo test --manifest-path third_party/ts_netstack_smoltcp_core/Cargo.toml --locked --all-targets` | passed on Windows: 15 tests |
| `python tool/check_openvpn_sources.py` | passed: 5 native source locks and license notices |
| `cargo clippy -p usque-openvpn --all-targets --features interop-test --locked -- -D warnings` | passed on Windows |
| `cargo test -p usque-openvpn --features interop-test --locked` | passed on Windows: 15 memory-only tests |
| `buf lint` | passed |
| `buf format --exit-code --diff` | passed |
| `buf breaking --against '.git#ref=HEAD'` | passed |
| `python tool/check_repository_policy.py` | passed |
| `git diff --check` | passed |
| `go mod verify` in `oracle/go` | passed |
| `go test ./...` in `oracle/go` | passed: all six frozen packages compile; those packages contain no Go test files |
| `python tool/verify_oracle_archive.py` | passed: 41 files match frozen commit `6aa03fc97d12848dce34eedbd187fb1077b5d1ea` |

A parallel Windows test compile exhausted available commit memory (OS error
1455). Validation was restarted with `CARGO_BUILD_JOBS=1`. No toolchain, page-file
or system networking settings were changed. Buf's edited source was normalized
to LF before its final format check. Linux tmpfs filled during a supplemental
compile; this task's build cache was relocated to the disk-backed `/var/tmp`
before resuming. Intermediate Kotlin assertions were updated
to the new initial-connection and L4 recovery behavior before the passing suite.

The Linux standalone smoltcp run could not fetch its additional test dependencies
from crates.io; the supplemental dependency and interoperability checks use the
Windows cache and native helper environment.

## Deterministic recovery evidence

Timing tests use Tokio's paused monotonic clock and fake transports. Proxy tests
add loopback client connections. Values below describe those fixtures.

| Scenario | Observed result |
| --- | --- |
| Two hours of failed recovery with no usable physical network | 7,200 consecutive intervals of exactly 1,000 ms; scheduling-wait P50/P95 = 1,000/1,000 ms |
| New usable network event | recovery advances after the 100 ms settling window |
| H3 candidates blackholed, H2 ready | H2 starts and wins at 250 ms |
| Automatic IPv4 H2 traversal | all 255 eligible Plus addresses are attempted once before a round restarts; excluded `.1` stays excluded |
| Candidate admission and shutdown | maximum 14 total, H3 at most 8, H2 at most 10; completed losing handshakes retain slots through cleanup |
| HTTP POST held through recovery | waits 1,801 seconds, then sends its body once |
| SOCKS TCP held through recovery | waits 1,801 seconds, then connects; established relay survives health-state changes |
| SOCKS UDP recovery | control connection and bound UDP port survive 1,801 seconds; new datagrams resume afterward |
| Interrupted remote setup budget | 4 seconds used, 2 hours recovering, 6 seconds remaining |
| Concurrent waiting requests | 128 observed waiting; peak 128; cancellation returns the count to zero |
| Repeated recovery reason | 7,200 occurrences merge into one timeline entry with repetition count 7,200 |
| User stop racing readiness | cancellation wins; waiting work does not resume |
| H3 idle/loss observation | keepalive remains 30 seconds; idle time does not consume a new probe's ACK deadline; ACK progress and resume reset observation |
| Control protocol compatibility | new flags and diagnostic fields have wire snapshots; omitted flags keep legacy defaults; lost acknowledgement queries status without resubmitting |

Authentication, pin mismatch/refresh, identity, socket protection, obsolete
network generations, DIRECT/REJECT routing, proxy authentication and cancellation
remain covered by the workspace suites. Frontend admission and bounded early
reads apply while clients wait. Candidate and waiter telemetry contains counts
and existing reason codes; no new credential or address fields are recorded.

## Isolated validation

| Measurement or environment | Result |
| --- | --- |
| Windows live network switch, sleep/wake, Wintun and platform cleanup | `not_run`: snapshot VM and independent management channel unavailable |
| Android device network switch, Doze, retained TUN and service lifecycle | `not_run`: dedicated device or isolated emulator unavailable |
| Linux TUN, firewall, route, DNS and session restoration | `not_run`: Linux snapshot VM unavailable |
| External endpoint, DIRECT, IPv4/IPv6, DNS and Kill Switch traffic observation | `not_run`: network observer unavailable |
| First successful business request after outage: P50/P95 | `not_run`: controlled network/performance lab unavailable |
| Live proxy error count, existing-flow survival rate and memory/resource peaks | `not_run`: controlled network/performance lab unavailable |
| Ubuntu 22.04 amd64/arm64 packaging and ABI/signature gates | `not_run`: matching native build hosts unavailable |
| Linux arm64 goldens | `not_run`: native arm64 host unavailable |

No package was installed, no release APK was produced, and no system VPN,
routing, DNS, firewall or proxy mutation was exercised during these checks.

## Additional subagent review - 2026-10-22

Three read-only subagents independently reviewed transport recovery, proxy
continuity and platform connection intent. The parent agent traced the reported
paths, applied the confirmed fixes and added regressions.

The fixes cover stop/start races and unpublished startup cleanup; preservation
of candidate authentication and pin failures; L4 generation cancellation and
QUIC lease cleanup; H2 scheduling-pause observation; Windows automatic endpoint
lease release; Linux missing-route/snapshot recovery; Android control-flag
inheritance and readiness; packet-chain listener adoption and business policy;
OpenVPN/WireGuard/Proton recovery; replaceable SOCKS UDP upstreams; and bounded
SOCKS pipelining with write-half-close and immediate prefix-buffer release.

H3 keepalive remains 30 seconds. Endpoint pin checks, authentication, socket
protection and exact lease validation retain their existing enforcement.
Transient platform retries use explicit error codes or validated Linux details.
The review source contains 82 changed Rust, Kotlin, Dart and protobuf files,
with combined SHA-256
`1cd5afc57c557aca5ae3df9a7e9b53bd1b3653182ccd50dcfedd8eeaa6353b26`
using the normalization above. The Windows and Linux validation copies match
all 82 per-file hashes. The initial matrix above belongs to its stated digest.

The review reruns Rust, Kotlin, repository policy and whitespace gates. Dart,
Flutter runner, protobuf wire shape, Go oracle and embedded dependency sources
are unchanged by this review; their initial validation remains listed above.

| Review command | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | passed |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | passed: 2,049 tests, 13 ignored |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2` | passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` on Linux | passed |
| `cargo test --workspace --all-targets --locked` on Linux | passed: 1,893 tests, 3 ignored |
| `cargo build --release --locked -p usque-engine -p usque-agent -p usque-update` on Linux | passed |
| `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| `.\gradlew.bat --no-daemon :app:ktlintCheck` in `apps/usque_gui/android` | passed |
| `.\gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` in that directory | passed: 490 tests; lint passed |
| `python3 tool/check_repository_policy.py` on Linux (Python 3.14.4) | passed |
| `git diff --check` | passed |

The new loopback L4 regression reaches QUIC establishment while the peer holds
HTTP/3 SETTINGS, changes the physical-network generation, and checks that the
old receiver's authorization has been released before the replacement dial.
The virtual-clock variant starts the replacement after exactly 100 ms.
Packet-chain tests keep DIRECT forwarding and the same HTTP/SOCKS ports across
frontend adoption; SOCKS half-close tests deliver pipelined bytes once after
recovery. Linux fake-Agent tests retain the active monitor through unavailable
physical snapshots and resume on a later snapshot.

The first full runs found an invalid fixture: its stub health publisher had
already closed before frontend adoption. The regression now retains a real
in-memory tunnel health publisher. Production cancellation behavior was retained.

Linux release validation rebuilt the OpenVPN package cache after CMake detected
the earlier cache relocation. Only that package's release cache was removed.
