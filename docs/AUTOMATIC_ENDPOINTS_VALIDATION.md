# Automatic endpoint development validation

Validation date: 2026-09-30. Scope: automatic/custom endpoint selection for
Consumer CONNECT-IP and experimental L4, including Windows exact egress leases,
Android control, configuration migration and Flutter settings.

The tested source is the uncommitted working tree based on
`9d4cd8fdf7076ff1e2f4171593af931d177d28a4`. This record does not identify a
complete immutable source snapshot. Its results apply to this development run
and cannot be assigned to a later commit or used as signed release evidence.
The behavior contract is in [Network settings](NETWORK_SETTINGS.md).

## Environment and commands

Checks ran on the Windows x64 development machine. Rust 1.97.1 used locked
dependencies and the supported Windows native helper. Flutter was resolved
from `apps/usque_gui/android/local.properties` and verified as 3.44.7, full
revision `84fc5cbb223bc12f83d65b647ff8a56caf779ffd`. Android used NDK
29.0.14206865, SDK CMake 3.22.1 and process-local Temurin 17.0.20+8.

Flutter and Dart commands used
`C:\Users\George\.local\share\flutter-3.44.7\bin`. Python commands used
`C:\Users\George\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe`.
PowerShell helpers used PowerShell 7; `JAVA_HOME` and SDK/runtime PATH additions
were process-local. No global toolchain or security settings were changed.

Commands start at the repository root except where another directory is named.

| Exact command | Result |
| --- | --- |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed; locked workspace/all-targets, warnings denied |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | 1429 passed, zero failed, eight explicitly ignored |
| `cargo fmt --all --check` | Passed through the aggregate check |
| `pwsh -NoProfile -File tool/check_source.ps1` | Passed; Rust/Dart/Kotlin, Ruff 0.16.0, PSScriptAnalyzer 1.25.0 and Buf 1.72.0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | Passed; locked x64-v2 release compile, no application launch |
| `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed against final production sources |
| `buf lint` | Passed |
| `buf format --exit-code --diff` | Passed |
| `buf breaking --against '.git#ref=HEAD'` | Passed against the local base; not a PR-target CI result |
| `python tool/check_repository_policy.py` | Passed |
| `git diff --check` | Passed |

In `apps/usque_gui`:

| Exact command | Result |
| --- | --- |
| `flutter pub get --enforce-lockfile` | Passed |
| `dart format --output=none --set-exit-if-changed lib test` | Passed; 195 files, zero changes |
| `flutter analyze --no-pub` | Passed; no issues |
| `flutter test --no-pub` | 731 passed, including exact Windows goldens |
| `flutter build apk --debug --config-only --no-pub` | Passed; configuration only |
| `& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .` | Passed |
| `flutter build windows --release --no-pub --split-debug-info=build/symbols/windows` | Passed; compile only, application not launched |

In `apps/usque_gui/android`:

| Exact command | Result |
| --- | --- |
| `.\gradlew.bat --no-daemon :app:ktlintCheck` | Passed |
| `.\gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` | Passed; 276 JVM tests, zero failures/errors/skips |

Gradle's debug checks also compiled JNI for arm64-v8a, armeabi-v7a and x86_64
through the pinned Rust helper. This is compilation coverage, not device
validation. Generated JNI, build outputs, logs and symbol files are excluded
from source changes. No MSI or release APK was created or installed.

Earlier gate attempts identified a Kotlin capability allowlist omission and
Rust test type/import errors; these were corrected before successful reruns.
Only completed successful commands are counted as passes.

## Deterministic acceptance coverage

The core, transport, engine, IPC, Agent and Android Rust suites cover:

- Schema 20 migration, Automatic defaults, legacy absent fields decoding as
  Custom, rejection of unknown values, appended wire fields and cold-change
  masks. Managed identities preserve the shared Consumer mode.
- Authenticated Free/Plus/unknown pool selection, exact H3 members, all H2 IPv4
  members, 256 unique full-host-space IPv6 samples per prefix, fixed seeds,
  family/DNS exclusions and fresh sampling for subsequent cycles.
- Concurrent H3 startup, mixed H2 batches capped at ten, preferred-family
  delay and early release, one absolute two-second deadline, loser shutdown,
  terminal-error priority and continued traversal of the finite candidate set.
- Socket-before-lease shutdown, cancellation when the outer race is dropped,
  stale generation rejection, accepted-connection reuse, bounded pin refresh
  and existing compatibility retry handling.
- L4 session admission, eight startup actors, cumulative 256 KiB input limits
  before QUIC parsing, SETTINGS readiness, promotion after loser cleanup and
  absence of business streams on unpromoted candidates.
- Exact Agent address/port/protocol/family checks, Prepared/Active authorization,
  purpose isolation, lease limits, generation ownership, journaled metadata
  recovery and cleanup failure retention. These use inert backends or anonymous
  memory; they do not install WFP rules or create Wintun sessions.
- Profile-derived native/JNI/IPC wait budgets, controlled reconfiguration,
  request cancellation and existing non-replay behavior.

The Flutter and Kotlin suites cover mode-switch drafts, invalid Custom drafts
switched to Automatic, retained saved addresses, later Custom draft submission,
restore defaults, failed saves, managed accounts, old-engine capability refusal,
strict Android capability decoding, all 21 translations and responsive layouts.
The new Automatic English/light and Custom Chinese/dark goldens and affected
L4/TV baselines were visually reviewed on the pinned Windows renderer.

Additional direct regressions exercise these acceptance boundaries:

| Test | Evidence |
| --- | --- |
| `final_h2_batch_succeeds_after_complete_traversal_and_reuses_winner` | 103 actual batch races, 1024 candidate requests, success in the final batch and unchanged winner object identity |
| `automatic_h3_all_eight_requests_overlap_with_family_delay` | Actual selector, eight overlapping protected startup requests and the 250 ms family head start; no public packets sent |
| `automatic_h3_filters_forced_and_unavailable_families_before_requests` | Forced single families and unavailable preferred families filtered before requests |
| `automatic_pin_refresh_has_one_global_sixty_second_deadline` | Actual refresh wrapper, virtual-time expiry and dropped pending refresh |
| `authenticated_l4_peer_without_settings_never_publishes_readiness` | Pinned/mTLS QUIC loopback peer completes authentication but sends no SETTINGS; startup stays unready, then cancels cleanly |

The orchestration tests use fake ready connections or deferred socket protection.
Native protocol tests use loopback Custom endpoints. Live Automatic native
winner reuse and complete reconnect-cycle IPv6 resampling have constituent
coverage, not live end-to-end evidence; their production paths share the tested
winner-return and fresh candidate-generation helpers.

Eight Rust tests remain explicitly ignored: two Wintun-loading tests requiring
an isolated Windows VM, one optional supplied OpenVPN profile test, four
credential-dependent live loopback proxy/VPN Gate/chain-DNS tests, and one
controlled memory benchmark. Their ignored status is not a pass.

## Network evidence and unavailable validation

The IPv6 spelling is supported by the
[Cloudflare IPv6 allocation list](https://www.cloudflare.com/ips-v6/).
[Published H2 experiments](https://github.com/vernette/warpscout/blob/master/masque.go#L65-L82)
support cross-prefix sampling of `2606:4700:103::/48` and
`2606:4700:104::/48`; neither source proves every address works or establishes
Consumer account eligibility. Free/Plus asymmetry follows the requested policy.

Limited TCP/TLS sampling before implementation tested 18 IPv4 targets across
198/199, including host values 0, 1, 2, 17, 63, 127, 193, 254 and 255, plus two
out-of-pool controls. TCP/TLS responses, certificate rejection and lack of H2
ALPN do not establish authenticated WARP CONNECT-IP availability. The local
machine had no public IPv6 route; IPv6 protocol measurement is `not_run`.

| Validation | Status and limit |
| --- | --- |
| Live Free/Plus authenticated H2/H3/L4 connections and measured winning endpoint | `not_run`; no enrolled-account protocol measurement in this implementation run |
| Public IPv6 H2/H3 protocol availability and entire-/48 usability | `not_run`; no public IPv6 route, exhaustive usability is not established |
| Windows snapshot-VM Wintun, Prepared/Active WFP, reconnect, crash recovery and platform restoration | `not_run`; isolated infrastructure unavailable |
| Dedicated Android device/emulator VPN lifecycle, Doze, Always-on, Lockdown, reboot and TV | `not_run`; isolated infrastructure unavailable |
| External IPv4/IPv6/DNS/Kill Switch/route/endpoint leak observation | `not_run`; network-observer infrastructure unavailable |
| Controlled repeated resource and performance measurements | `not_run`; performance-lab infrastructure unavailable |

No live VPN, route, interface-DNS, system-proxy or WFP mutation was used for
workstation validation. Candidate authorization stays exact and bounded;
prefixes are validation data, not broad route/firewall grants. The new policy
and capability fields contain no credentials. Cleanup errors retain recovery
ownership and fail closed. Deterministic mocks and builds cannot prove native
platform cleanup or leak prevention; the unavailable checks remain `not_run`.

## UDP blackhole regression follow-up

Follow-up date: 2026-09-30. The user reported that Android cannot complete
H3-to-H2 fallback with Automatic endpoints, while Custom endpoints work.
Baseline: `678bc680e4076f5e5ba363c15663bdc32388b083`, plus the uncommitted
regression test and this record. This follow-up has not identified the Android
failure's root cause and makes no production behavior change.

The new
`endpoint_race::tests::native_h3_blackhole_race_times_out_and_preserves_h2_fallback`
test races four real QUIC startups against bound, silent loopback UDP peers.
It finishes within ten seconds, returns an H3 failure eligible for H2 fallback,
and observes zero retained exact socket leases after all candidates exit.
The scoped transport suite completed in about eight seconds. This exercises
the shared native race and timeout/cleanup code on Windows; it does not run
Android JNI socket binding or Android's Unix UDP batch backend.

The test verifies H3 timeout cleanup and fallback eligibility. It does not
establish that an authenticated H2 connection succeeds on the reported Android
network.

| Exact command | Follow-up result |
| --- | --- |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test -Package usque-transport` | 573 passed, zero failed; includes the real loopback blackhole test |
| `cargo fmt --all --check` | Passed |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | 1430 passed, zero failed, eight existing ignored tests |
| `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | Passed; compile only |
| `python tool/check_repository_policy.py` | Passed |
| `git diff --check` | Passed |

Android device reproduction, JNI/batch-backend runtime verification and live
authenticated H2 candidate measurements remain `not_run`. The Android
connection timeline/error code is still needed to determine its failure stage.
