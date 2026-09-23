# MASQUE performance candidates

Base: `d00dafb9f9d4e2e86ecd2ffb89d4d9b91f446543`. Started 2026-09-24.
The candidate sequence is A (measurement), B (duplex progress), C1 (H2
ownership/framing), C2 (Android ready writes), D (H3 bounded sending).
Each candidate is a separate commit; the complete commit containing a stage's
record identifies its tested source. Later records list previous full SHAs.

## A — measurement baseline

Changes: append-only queue wait/performance metrics, typed non-failure timeline
event, bounded Windows/Android bridges and local diagnostic export. The original
packet scheduling and copying behavior is retained. MTU, windows, queue/pool
capacities, congestion algorithms, and send quantum are unchanged.

Correctness coverage includes actual-Pending detection, repeated polling,
immediate admission, cancellation/drop/receiver close, permit release,
exponential event sampling, old/unknown wire fields, missing groups, and
bounded numeric-only JSON export. Wait cleanup is RAII and does not mutate
network settings. No destination, payload, credential, or free-form error is
added to performance export.

Validation on Windows x64, pinned Rust 1.97.1, Flutter 3.44.7
(`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`), NDK 29.0.14206865:

| Command (repository root unless noted) | Result |
| --- | --- |
| `cargo fmt --all --check` | exit 0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | exit 0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | exit 0; 1,263 passed, 8 ignored, 0 failed in the full workspace |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | exit 0; compile only |
| `& .\tool\build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | exit 0 |
| `buf lint` | exit 0 |
| `buf format --exit-code --diff` | exit 0 |
| `buf breaking --against '.git#ref=d00dafb9f9d4e2e86ecd2ffb89d4d9b91f446543'` | exit 0 |
| `flutter pub get --enforce-lockfile` in `apps/usque_gui` | exit 0 |
| `dart format --output=none --set-exit-if-changed lib test` in GUI | exit 0 |
| `flutter analyze --no-pub` in GUI | exit 0 |
| `flutter test --no-pub` in GUI | exit 0; 657 tests including goldens |
| `& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .` in GUI | exit 0 |
| `flutter build windows --release --no-pub` in GUI | exit 0; compile only |
| `flutter build apk --debug --config-only --no-pub` in GUI | exit 0; configuration only |
| `.\gradlew.bat --no-daemon :app:ktlintCheck` in GUI/android | exit 0 |
| `.\gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` in GUI/android | exit 0 |
| `pwsh -NoProfile -File tool/check_source.ps1` after Windows helper in same session | exit 0 |
| `python tool/check_repository_policy.py` using the verified executable below | exit 0 |
| `git diff --check` | exit 0 |

Python uses the verified 3.12.14 runtime executable at
`C:/Users/George/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe`.
SDK paths resolve from uncommitted `local.properties`; Flutter commands use that
SDK explicitly. Logs stay in the local temporary directory. Initial failures
(an exception-type test expectation and Kotlin line length) were fixed and the
full affected suites rerun. Vendor linker/Gradle deprecation notices remain;
no check or warning policy was weakened.

## B — duplex queue admission

A source: `e43493e47bb081faa351090bdb532d7d5687d1d1`.
The mux retains one pinned outgoing admission future, pauses both input queues
until it resolves, and continues tunnel/direct replies and priority cancellation.
Routing and NAT complete before the future is created. Queue accounting is
owned by the future and released on error/cancellation. Flow expiry scans pause
while an admission is outstanding. One separately bounded proxy delivery batch
preserves proxy order; synchronous classification delivers its TUN subset first.
Only further tunnel batches pause while proxy delivery waits. Closed proxy
receivers drop their bounded delivery tail and release accounting.

The original implementation failed the new full-uplink and blocked-proxy
reverse-progress tests (502 passed, 2 failed in the library test run). The
regression asserts replies while capacity remains full; it does not drain the
uplink first. Additional tests cover colliding TUN/proxy ports, restored reply
bytes/checksums, direct replies, global cancellation, attachment replacement,
receiver close, exact-once admission order, capacity recovery, expired NAT
mapping retention during a wait, and cancellation of a proxy delivery tail.
GEO/internal DNS asynchronous direct-flow creation retains its existing behavior;
this stage does not eliminate that independent wait.

B checks completed with exit 0:

- `cargo fmt --all --check`
- `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy`
- `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test`
  (1,270 passed, 8 ignored, 0 failed)
- `& .\tool\build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy`
- `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2`
- `python tool/check_repository_policy.py` (verified executable listed above)
- `git diff --check`

The GUI and wire schema are unchanged from A; their checks were not repeated
for this Rust-only scheduling change. No device throughput result is available.

## C1 — H2 framing and conditional inbound rewriting

B source: `ff48bef4590e12ab7ad8c6642d63200c9deb9e96`.
A private H2 framer retains the current DATA as `Bytes` and copies only an
incomplete capsule into a bounded partial buffer (at most 65,552 bytes). It
finishes that capsule before interpreting the DATA tail, so the size check is
per capsule instead of aggregate buffered bytes. The mux first classifies an
immutable packet and obtains mutable storage only for an actual NAT identifier
or quoted ICMP rewrite. Copy counters are accumulated at drain/batch boundaries.
The larger connection state is boxed once when creating the private transport
enum; no per-packet task/boxing is introduced.

Before the fix, the three new storage/boundary tests failed (509 passed,
3 failed in the transport library): complete-DATA pointer retention, maximum
capsule tail plus following capsule, and no-rewrite inbound pointer retention.
The small-window test already passed and remains a regression guard: a 65,535-byte
IP packet traverses a 1,024-byte stream receive window, with used capacity back
to zero. Every DATA still returns its capacity exactly once. The 128-packet
ready-frame regression, control order, bounded rejection replies, incomplete
receive cancellation, deferred lookahead error, EOF and batch bounds remain.

New property tests use the locked proptest dependency for arbitrary bytes and
random chunking. Deterministic tests split every position across all four
varint widths and verify that the following complete capsule retains its DATA
pointer. Owned inbound tests cover IPv4, IPv6, fragments, ICMP quotes, policy
rejection, shared-source immutability and unique-allocation reuse.

C1 checks completed with exit 0:

- `cargo fmt --all --check`
- `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy`
- `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test`
  (1,280 passed, 8 ignored, 0 failed)
- `& .\tool\build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy`
- `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2`
- `python tool/check_repository_policy.py` (verified executable listed above)
- `git diff --check`

The unchanged protobuf/GUI/Kotlin checks retain A's results. No device
throughput or allocation/CPU improvement is claimed from unit tests alone.

## C2 — Android ready TUN writes for MASQUE

C1 source: `262f136fb29f3bdaeadc0580105bbcb128c5dbff`.
The ready write loop now requires a TUN and packet I/O, with an optional L4
observer. The same bounded loop is shared with host tests: one pending packet,
at most 16 packets / 64 KiB / 200 microseconds, cancellation before each packet,
and a retained pending packet on WouldBlock or budget exhaustion. Each TUN
write remains one IP packet. Transport/write errors return to the session owner
for existing stop handling. The main asynchronous readiness branches remain.

Moving the former L4-only eligibility into the testable loop first reproduced
four failures without an observer (50 Android crate tests passed, 4 failed).
Removing that eligibility passed the same tests for both observer modes,
WouldBlock recovery/order, packet/byte/time budgets, errors, per-packet
cancellation, and pending-slot cleanup before a new simulated session. These
are deterministic pump tests, not actual Android VPN lifecycle evidence.

C2 checks completed with exit 0:

- `cargo fmt --all --check`
- `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy`
- `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test`
  (1,284 passed, 8 ignored, 0 failed)
- `& .\tool\build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy`
  (compiles the real Android caller and its optional observer)
- `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2`
- `python tool/check_repository_policy.py` (verified executable listed above)
- `git diff --check`

The unchanged protobuf/GUI/Kotlin checks retain A's results. No device
throughput improvement is claimed from host tests.

## D — bounded H3 send work

C2 source: `f3469b245791b069875797bdcd724fd4d937e0bf`.
Each synchronous application step prepares one fixed-size stack DATAGRAM header
and queries the effective payload limit once. The private encoder accepts only
packets validated by the batch admission boundary; public send methods still
reject malformed IP input. Every next actor round recalculates the limits after
network/path events. Header-copy and PMTU-defer counters are aggregated per step.
A guarded try_recv claims at most one ready batch without waiting, while the
existing recv branch still wakes an empty actor. Startup, pending-batch and
migration injection barriers remain in force.

Wait-group availability is also tightened: manually accounted queues without
an instrumented admission publish no wait group. For A–C2 comparisons, ignore
those queues' placeholder zero waits and use the measured TransportOutgoing
admission group and actual queue-depth/drop counters.

Application and wire steps now return typed progress and stop reasons, used by
metrics and tests. QUIC Done with backlog remains an observation, not a diagnosis
of insufficient congestion window. No queue/pool capacity, MTU, congestion
algorithm, send quantum, pacing deadline, path generation, UDP partial-prefix
handling, GSO, kernel parameter or vendored quiche algorithm is changed.

A test-only thread-local query counter first reproduced 64 payload-limit lookups
for one 64-packet batch (518 passed, 1 failed in the library run); it passes with
one lookup after the change. Additional tests cover one-batch ready admission,
startup/migration barriers, producer closure, pool exhaustion with zero/one/all
buffers returned, DATAGRAM full/recovery, public malformed input, bounded wire
progress, small quantum and Done with backlog. Existing PMTU regressions now
also assert deferred/cancelled stop reasons. The full suite retains four-algorithm
handshakes, future send times, migration generations, partial UDP completion
(0/1/N), WouldBlock, portable fallback and EMSGSIZE coverage.

D validation results:

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | exit 0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | exit 0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | exit 0; 1,291 passed, 8 ignored, 0 failed |
| `& .\tool\build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | exit 0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | exit 0; compile only |
| `buf lint` | exit 0 |
| `buf format --exit-code --diff` | exit 0 |
| `buf breaking --against '.git#ref=d00dafb9f9d4e2e86ecd2ffb89d4d9b91f446543'` | exit 0 |
| `pwsh -NoProfile -File tool/check_source.ps1` after Windows helper in same session | exit 0 |
| `python tool/check_repository_policy.py` using the verified executable above | exit 0 |
| `git diff --check` | exit 0 |

The aggregate reruns Flutter analysis, Dart formatting, Kotlin ktlint, Ruff,
PSScriptAnalyzer and Buf checks against the final source. The unchanged GUI's
657 widget/golden tests, Windows Flutter release build, and Kotlin unit/lint
suites retain A's results. The eight ignored Rust tests consist of two isolated
Wintun-load tests, one optional supplied-profile test, four credential-dependent
live proxy tests, and one controlled WireGuard memory benchmark; none is a pass.
No benchmark or device performance result is inferred from these checks.

Candidates are local source commits. No CI APK artifact has been produced by
this task; candidate packages still come from the existing build workflow for
the selected full SHA. These are mechanism/correctness results; real throughput
gains remain unmeasured.

## Device and isolated evidence

All candidate throughput, CPU, RSS, device lifecycle, and protected-runner
results are `not_run`. Existing user screenshots are problem reports, not
controlled baselines. Local unit tests and compile-only builds do not establish
native VPN/TUN cleanup or leak behavior. No VPN/TUN session, system networking
mutation, APK/MSI installation, release signing, or publication is performed.

Use the existing build workflow for each full candidate SHA. Alternate each
candidate with its predecessor on the same dedicated test device/network,
SmarTone node, account, endpoint, MTU, routing and disabled chain proxy. Reconnect
for every protocol/algorithm choice. Start with three alternating pairs, then
at least seven pairs for confirmation. Export diagnostics before disconnect;
include candidate SHA, order and screenshots. Stop a set if heat or direct
baseline drift invalidates comparison. Compare same-instance counter deltas.
Unknown CPU/RSS stays unknown. Throughput median must reach 95% of control;
other measured budgets follow the existing performance policy. Claim a speedup
only when repeated improvement exceeds variability. Keep correctness fixes;
revert a performance subcommit with stable regression.
