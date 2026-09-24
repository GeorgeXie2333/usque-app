# MASQUE performance candidates

Base: `d00dafb9f9d4e2e86ecd2ffb89d4d9b91f446543`. Started 2026-09-24.
The candidate sequence is A (measurement), B (duplex progress), C1 (H2
ownership/framing), C2 (Android ready writes), D (H3 bounded sending).
Each candidate is a separate commit; the complete commit containing a stage's
record identifies its tested source. Later records list previous full SHAs.

Status update: the post-`b5eb520` transport changes were withdrawn after the
Android regression reported on 2026-09-24. The workstation retention decision
below is historical, superseded by the [device regression record](#device-regression-and-baseline-restoration-2026-09-24).

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

## Follow-up — device feedback and cooperative-yield accounting

Recorded 2026-09-24, based on source
`4a57f9f650969281e74ee5766c444a9a659e805a`. Its transport source is D;
the intervening commit changes the GUI disconnected headline. The user reports
building the tested package from current HEAD; this is not independent artifact
or device verification. The commit containing this follow-up identifies the
subsequent diagnostic correction, not the package in the screenshots.

| User observation | Download (Mbps) | Upload (Mbps) |
| --- | ---: | ---: |
| Earlier H2 | 308.7 | 586.2 |
| Earlier H3/Cubic | 902.1 | 380.4 |
| Current H2 | 539.6 | 395.5 |
| Current H3/Cubic | 1,008.1 | 346.0 |

These single measurements show higher download and lower upload than the
earlier screenshots. They do not establish a causal gain or regression: there
are no alternating repeated samples, CPU/RSS observations or paired diagnostic
deltas, and the earlier and current pairs show different exits. The current
H2/H3 screenshots use the same displayed exit and SmarTone test node. The user
reports that H3's upload ramp is most pronounced after reconnect, less pronounced
on subsequent tests, but still plateaus in the 300 Mbps range. All four tested
H3 congestion choices previously gave similar upload rates. Treat startup and
the sustained ceiling as separate unresolved observations.

The new upload timeline shows `QueueBackpressured` on `transport_outgoing`,
mostly `0 ms` and once `1 ms`. This is successful admission after a sampled
wait, not `SEND_QUEUE_FULL` or evidence of packet loss. Integer milliseconds
truncate sub-millisecond waits. Power-of-two sampling explains why visible
events become further apart without proving that queue pressure subsided.

Source inspection found an additional measurement defect. Locked Tokio 1.53.1
checks its cooperative budget before polling a semaphore. Previously, the
admission wrapper interpreted every Pending as exhausted queue capacity. An
empty-queue regression first failed with one recorded wait instead of zero
(525 library tests passed, one failed). The correction preserves the underlying
future and its cooperative yield, and only starts timing when the poll retains
budget to reach capacity acquisition. Tests cover exhaustion before each of
the channel, item and byte permits, and a real capacity wait following an
unrelated cooperative yield. Only the real wait contributes its duration;
existing cancellation, closure, ordering and permit cleanup tests still pass.

This correction changes diagnostics only. It does not remove cooperative
fairness, enlarge buffers, alter sending or receiving, change congestion
control, or establish a fix for the upload ceiling. No new fields or sensitive
data are exported; the existing RAII settlement and failure semantics remain.

Follow-up validation on the same pinned workstation toolchains:

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | exit 0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | exit 0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | exit 0; 1,293 passed, 8 ignored, 0 failed |
| `& .\tool\build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | exit 0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | exit 0; compile only |
| `python tool/check_repository_policy.py` using the verified executable above | exit 0 |
| `git diff --check` | exit 0 |

Only transport Rust and these references change in this follow-up. Flutter,
Kotlin, protobuf and aggregate multi-language checks are not rerun for it;
historical results above remain attached to their original candidates. Device,
isolated lifecycle, controlled performance, APK creation and installation are
`not_run`. The screenshots remain user feedback, not a performance-lab pass.

### Send-only mihomo reference

The user reports higher mihomo MASQUE upload with HEAD and default settings.
The source review is pinned to mihomo's Meta source branch at
[`ab405bad5beeeac8b003bb01f60f134f6df54471`](https://github.com/MetaCubeX/mihomo/tree/ab405bad5beeeac8b003bb01f60f134f6df54471),
whose module lock selects connect-ip-go `67ccdb0cf771` and quic-go
`2548683b76f4`. The exact user binary is not independently identified. The
comparison concerns sending only; no receive implementation is adopted.

- The [MASQUE sender](https://github.com/MetaCubeX/mihomo/blob/ab405bad5beeeac8b003bb01f60f134f6df54471/adapter/outbound/masque.go)
  uses one long-lived goroutine and a reused read buffer, forwarding packets
  directly to WritePacket. Its IP stack also originates proxied TCP connections;
  this differs from forwarding Android TUN packets. The documented default
  [IP stack mode](https://github.com/MetaCubeX/Meta-Docs/blob/main/docs/config/proxies/masque.md)
  is auto, selecting gVisor when compiled in and MIPS otherwise. It is not an
  identical inner TCP stack comparison.
- The locked [DATAGRAM send queue](https://github.com/MetaCubeX/quic-go/blob/2548683b76f4/datagram_queue.go)
  holds at most 32 frames. A full queue blocks admission until capacity or close;
  enqueue wakes the QUIC sender. The reference offers no evidence that Usque
  needs a larger application queue or encoding pool. Its
  [CONNECT-IP encoder](https://github.com/MetaCubeX/connect-ip-go/blob/67ccdb0cf771/conn.go)
  and [QUIC admission](https://github.com/MetaCubeX/quic-go/blob/2548683b76f4/connection.go)
  still allocate and copy packet data, so its speed does not demonstrate an
  entirely zero-copy send path.
- With no outer congestion-controller override, mihomo's
  [selection function](https://github.com/MetaCubeX/mihomo/blob/ab405bad5beeeac8b003bb01f60f134f6df54471/transport/tuic/common/congestion.go)
  leaves the QUIC default in place. The locked
  [packet handler](https://github.com/MetaCubeX/quic-go/blob/2548683b76f4/internal/ackhandler/sent_packet_handler.go)
  constructs its CubicSender with `use Reno = true`. Default therefore does not
  imply the same outer Cubic algorithm used in this Usque test.
- quic-go separates packet construction from socket writes with a bounded
  [send worker](https://github.com/MetaCubeX/quic-go/blob/2548683b76f4/send_queue.go)
  and supports GSO where the socket/kernel/path allows it. Usque already has
  Linux/Android sendmmsg batching; GSO is a different optimization, not proof
  that the user's mihomo run used it. Keep it as a separately measured follow-up
  if syscall/batch evidence points there, outside the initial no-GSO plan.

The first candidate for further investigation is the cost of handing each
uplink packet through Android, mux and transport admission. Check application
batch sizes, queue wait/depth deltas, QUIC stop observations and UDP datagrams
per syscall from the same connection instance before selecting a change. A
bounded ready-send/drain path could amortize repeated select/admission work,
but must preserve reverse progress, cancellation and packet order. Entering a
new select round does not by itself prove a thread switch or an extra wakeup.
No send scheduling change is made solely on this source comparison. The current
snapshot's congestion window is not a historical per-second series, and bytes
in flight remains unavailable; do not infer either from the timeline screenshot.

## Workstation investigation after b5eb520 (2026-09-24)

The follow-up baseline is
`b5eb52047eade9ff7b62f2b7e873f0af0e42cd58`. Tests use a temporary,
IPv4-loopback SOCKS listener, the existing account in a temporary configuration,
and isolated Chrome sessions on the user's Speedtest Custom page with SmarTone
Hong Kong selected. MTU is 1280; chain and direct splitting are disabled. No
system proxy, TUN, WFP, route or DNS setting is changed. Each run stops its
listener and browser and verifies that the original configuration is unchanged.
Probe sources, binary hashes, per-second numeric observations and browser
results stay in the ignored local evidence directory; no credentials or packet
contents are recorded or committed.

Earlier third-party HTTP CLI results were not a valid absolute-throughput
baseline. The official Ookla CLI and the same browser page both demonstrated
substantially faster direct upload. The follow-up uses the browser page for
the comparisons below. Different WARP exits and an uncontrolled workstation
remain confounders: these results are not Android TUN or performance-lab
certification.

### Reproduced defects and changes

- The H2 framer copied a whole IP payload when the peer put its capsule header
  in a separate DATA frame. A pointer-retention regression failed on the old
  implementation. The candidate retains a contiguous DATAGRAM payload in its
  DATA storage and reuses only the small header assembly buffer. Fragmented
  payloads and control capsules keep the existing bounded parser; IP validation,
  control ordering and per-DATA capacity return are unchanged. Tests include
  non-minimal varint widths, every header split, randomized DATAGRAM chunking,
  cancellation/resume and exact flow-control return.
- quiche notified BBR of application-limited sending while a DATAGRAM was still
  queued but could not fit the remaining window or output buffer. Four memory
  tests reproduced the notification with BBRv2/BBRv3. Requiring an empty DATAGRAM
  queue fixes that classification; this change alone did not remove the
  workstation's low BBRv2 upload plateau.
- BBR's byte-exact window-limited check did not account for an indivisible
  DATAGRAM leaving a sub-packet tail. The old BBRv2 PROBE_UP regression could
  not increase `inflight_hi` even with just one unused byte; BBRv3 also failed
  to record that round as window-limited. Both now use the current path MSS for
  this classification, including after PMTU changes. Exactly one MSS of free
  space remains non-limiting. Actual packet admission, pacing, quantum, buffer
  capacities and algorithm gains are unchanged.
- Both quiche recovery backends left per-path lost bytes at zero. Loss tests
  failed for all four algorithms while the connection counter was nonzero.
  The counters now accumulate once per declared loss; repeat detection does
  not double count, and PMTU probe losses remain excluded. Older byte-loss
  observations are unavailable for comparisons, not evidence of no loss.
- Temporary actor timing identified portable UDP sends as the largest measured
  section during Cubic upload. These were wall-clock section timings, not a CPU
  stack profile. The portable send callback nested Tokio `try_send_to` inside
  `try_io`, contrary to that API's raw-I/O contract. An IPv4/IPv6 test first
  failed with a cached `WouldBlock` although the kernel socket was writable.
  Borrowing the same protected socket through `SockRef` leaves readiness to
  the outer batch operation. Datagram boundaries, send order, partial prefixes,
  cancellation, fallback and EMSGSIZE handling are retained. No GSO is added.

### Follow-up validation

The normal source includes no actor timing prints or temporary probe entrypoint.
The changes do not add a privileged operation, weaken pinning or authorization,
alter protocol field numbers, or upload diagnostics. The same socket ownership
and bounded cleanup paths remain in use. Complete safe checks apply to the
Rust changes; Flutter, Kotlin, protobuf and aggregate multi-language checks are
not applicable to this follow-up. Their historical results above retain their
original scope. Android device tests, controlled performance sampling and all
isolated lifecycle/leak tests are `not_run`.

The command results and completed browser comparisons are recorded below.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | exit 0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | exit 0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | exit 0; 1,297 passed, 8 ignored |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | exit 0; compile only |
| `& .\tool\build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | exit 0 |
| `cargo test --manifest-path third_party/quiche-0.29.3/Cargo.toml --locked --lib --config profile.dev.package.boring-sys.opt-level=1 --config profile.dev.package.boring-sys.debug=false` | exit 0; 1,076 passed |
| `cargo test --manifest-path third_party/quiche-0.29.3/Cargo.toml --locked --lib --features qlog --config profile.dev.package.boring-sys.opt-level=1 --config profile.dev.package.boring-sys.debug=false recovery::gcongestion::bbr3::` | exit 0; 21 passed |
| `cargo test --manifest-path third_party/quiche-0.29.3/Cargo.toml --locked --lib --features qlog --config profile.dev.package.boring-sys.opt-level=1 --config profile.dev.package.boring-sys.debug=false queued_datagram_is_not_application_limited` | exit 0; 4 passed |
| `python tool/check_repository_policy.py` using the verified executable above | exit 0 |
| `git diff --check` | exit 0 |

The standalone checks ran after the supported Windows helper initialized the
native toolchain. The `profile.dev` overrides are the pinned dependency's
documented Windows BoringSSL CRT requirements, not a relaxation of TLS checks.

### Browser comparison and retention

| Protocol / direction | Runs per version | Baseline median Mbps | Candidate median Mbps | Baseline / candidate MAD divided by median |
| --- | ---: | ---: | ---: | ---: |
| H2 download | 7 | 298.2 | 325.4 | 21.9% / 15.7% |
| H2 upload | 7 | 571.7 | 570.2 | 9.0% / 12.0% |
| H3 Cubic download | 3 | 583.0 | 559.8 | 0.4% / 0.1% |
| H3 Cubic upload | 3 | 222.7 | 228.1 | 1.2% / 0.2% |
| H3 BBRv2 download | 7 | 646.9 | 663.9 | 2.0% / 1.0% |
| H3 BBRv2 upload | 7 | 37.3 | 271.7 | 16.9% / 3.5% |

Direct browser calibration moved from 1497.9/2165.5 Mbps before the investigation
to 1238.9/1927.7 Mbps afterward (download/upload). Measurement stopped after
detecting this drift. This and the unstable H2 and BBR baseline samples prevent
a controlled throughput acceptance claim or a precise speedup multiplier.
Each BBRv2 candidate upload nevertheless exceeded every baseline upload in the
seven adjacent comparisons: 239.5–294.0 versus 18.9–47.4 Mbps. Together with the
memory regressions and restored window growth, this supports repairing the
low-window defect; it does not establish the Android improvement or lab fairness.

The H2 candidate reduced assembly copies from essentially all DATA bytes to
less than 0.4%, while retaining payload ownership and flow-control correctness.
Keep this copy reduction as a candidate with **unconfirmed throughput benefit**;
the noisy measurements do not show a stable regression. Cubic's small observed
change also remains unconfirmed, and its upload plateau is **not resolved**.
Retain the independent UDP readiness and QUIC correctness fixes. No default
algorithm, queue capacity, MTU, H2 window or encoding-pool capacity was changed.

Recorded queue drops, send timeouts and mux incoming-copy bytes remained zero
in these sessions. This does not mean the network was lossless: QUIC packet and
byte losses were observed. Portable Windows UDP still uses one syscall per
datagram. Exact CPU per bit, latency p95, true RSS peak, bytes in flight and
thermal state were not obtained; process CPU time and sampled RSS cannot stand
in for those acceptance metrics. No performance-lab report or passing budget
status is synthesized from this workstation evidence.

All 44 proxy test sessions in this follow-up reported successful shutdown,
closed listeners and unchanged original configuration hashes; no probe process
remained. The two direct browser sessions were also closed. Existing user
browser sessions were not modified.

## Device regression and baseline restoration (2026-09-24)

The user reported lower throughput in both directions with Cubic after
`1122daa36735509badd6113d464da3ec23e3ad88`,
`add8b6811215d5bff389bf01b873cbb53c56384b`, and
`197b1f215e9c06a939055a2f0a6c05f766630305`:

| Device feedback | Download Mbps | Upload Mbps |
| --- | ---: | ---: |
| H2 | 202.3 | 352.7 |
| H3 / Cubic | 727.8 | 248.9 |

The user measured approximately 1600 Mbps in both directions without the
proxy, reinstalled the previous package, and observed the previous performance
again. The user also reported that H3 upload no longer showed the earlier
gradual ramp or obvious stalls. Smoothness and throughput are separate
observations; the former does not offset the reported regression. These are
device feedback, not an independently executed Android or performance-lab gate.

Restore the complete transport implementation from
`b5eb52047eade9ff7b62f2b7e873f0af0e42cd58` as a recovery candidate. This removes
the three-commit experiment as a group rather than claiming a particular line
has been proved responsible. Keep the earlier A-D work and cooperative-yield
diagnostic correction, including mux reverse progress and cancellation,
bounded H2 framing, whole-capsule zero-copy, and Android ready-write batching.
The unrelated GUI changes in
`d13616fafadc4fb850a7fe6132068290ca559a46` are preserved.

This deliberately restores the older split-capsule assembly, DATAGRAM/BBR
classification and portable UDP send behavior, including their known
limitations. The per-path lost-byte accumulation change is also withdrawn:
zero byte-loss values are not evidence of a lossless connection. Earlier BBRv2
workstation gains and the reported smoother H3 upload cannot be promised for
this recovery candidate. Reintroducing any part requires a separate candidate
and relevant device comparison; the previous workstation results do not
establish Android/Cubic acceptance.

Source inspection did not identify one changed hot path common to H2 and
H3/Cubic. The BBR model predicates are not used by Cubic, legacy Cubic's
`on_app_limited` callback is empty, and Android normally uses sendmmsg rather
than the portable send fallback. The H2 slice path can retain a DATA backing
allocation longer, but its effect on the reported device is unmeasured. These
facts bound the investigation; they do not invalidate the package rollback
comparison or establish a root cause. A baseline restoration is a recovery
step, not a claim that the source of the Android regression is resolved.

### Repeated workstation comparison

Run the same browser page and SmarTone node through fresh loopback SOCKS
sessions with MTU 1280, chain disabled and Cubic selected. Each protocol has
three adjacent old/new pairs; order is old/new, new/old, old/new. Reuse the
previously recorded `b5eb520` and post-three-commit probe binaries, verifying
their SHA-256 hashes before each run. The former's production Rust sources
match this restoration; these tests do not execute a rebuilt Android package.

| Protocol / direction | Runs per version | b5eb520 median Mbps | Three-commit median Mbps |
| --- | ---: | ---: | ---: |
| H2 download | 3 | 335.1 | 340.3 |
| H2 upload | 3 | 798.2 | 767.4 |
| H3 / Cubic download | 3 | 535.2 | 522.8 |
| H3 / Cubic upload | 3 | 208.6 | 204.6 |

H2 download ranges overlap (236.4–408.6 and 219.6–356.6 Mbps). The last H3
pair's uploads fall to 150.8/146.9 Mbps; the new version's ping/jitter also
reach 213/268 ms. Keep these observations rather than removing unfavorable
samples. Only two of the six pairs report the same WARP exit. Direct download
moves from 1153.7 to 1074.8 Mbps and upload from 1747.2 to 1764.5 Mbps. Stop
performance comparisons after this screening: these data neither reproduce
the device's four-direction regression reliably nor establish a passing
throughput/latency/resource budget. No seven-pair speedup claim is made.

All twelve proxy sessions report zero recorded queue drops and send timeouts,
normal shutdown and unchanged original configurations. QUIC packet loss is
nonzero; the old byte-loss counter is excluded from comparison. Queue waiting
alone is not a packet-drop count. Raw diagnostics, browser snapshots and
binary hashes remain in ignored local evidence. The two direct browser
sessions are also closed. No workstation TUN/VPN or system-network mutation
is performed.

### Restoration checks

The complete commit containing this record identifies the recovery candidate.
The following safe checks run on its production source, not on an installed
package:

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | exit 0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | exit 0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | exit 0; 1,293 passed, 8 ignored |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | exit 0; compile only |
| `& .\tool\build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | exit 0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -Package usque-core -CargoAction clippy` | first initialization exit 1; fresh-shell retry exit 0 |
| `cargo test --manifest-path third_party/quiche-0.29.3/Cargo.toml --locked --lib --config profile.dev.package.boring-sys.opt-level=1 --config profile.dev.package.boring-sys.debug=false` | exit 0; 1,068 passed |
| `cargo test --manifest-path third_party/quiche-0.29.3/Cargo.toml --locked --lib --features qlog --config profile.dev.package.boring-sys.opt-level=1 --config profile.dev.package.boring-sys.debug=false recovery::gcongestion::bbr3::` | exit 0; 20 passed |
| `python tool/check_repository_policy.py` using the verified executable above | exit 0 |
| `git diff --check` | exit 0 |
| `git diff --exit-code b5eb52047eade9ff7b62f2b7e873f0af0e42cd58 -- crates third_party/quiche-0.29.3/src Cargo.toml Cargo.lock rust-toolchain.toml tool/build_android_rust.ps1` | exit 0; no source difference |

Repeated `vcvars64.bat` initialization in one validation process reached the
Windows command-line length limit. Run the scoped helper in a fresh shell
before the standalone tests; it succeeds without changing the helper or
relaxing any check. Preserve the initial failure in the local command log.

The tests added with these three changes are reverted together with their
implementations. Earlier correctness, malformed-input, property,
queue/cancellation and PMTU tests remain in the full suites. No parser bound,
authorization, TLS verification, cleanup or fail-closed policy is relaxed.
This rollback changes Rust and technical records only; Flutter/Kotlin/protobuf
and aggregate multi-language checks are not applicable. Android device runs,
isolated lifecycle/leak testing and performance-lab validation remain
`not_run`. No package is installed, signed, published or uploaded.

## Post-restoration investigation and PTO correction — 2026-09-24

Starting revision: `b33d59fa8d865116ddd9c2beb7162e78f9364dd3`.
The complete commit containing this record identifies the retained source.
The earlier Android package rollback remains evidence of a device regression;
these Windows experiments do not establish Android recovery. A smoother H3
upload curve and a higher upload rate are separate outcomes.

### Retained correctness change

The vendored QUIC path counter named `total_pto_count` previously incremented
on every recovery timeout callback, including ordinary time-threshold loss
detection. Both recovery implementations now identify actual PTO expiration;
only that branch increments the cumulative PTO counter. ACK progress does not
reset it. A separate cumulative `loss_detection_timeout_count` preserves the
callback count, excluding ACK-driven loss detection. Rust path statistics gain
that field; C FFI layout, protobuf and application settings are unchanged.

CONNECT-IP PMTU revalidation now uses actual PTOs. A trigger probe recorded
1,942 declared lost packets among 83,907 sent and four increments of the old
counter in about three seconds. The 25% loss condition was false; the inflated
PTO alternative admitted revalidation. The writable QUIC DATAGRAM payload then
fell from 1,428 to 1,156 bytes before HTTP/3/context headers. These counters are
not physical packet-loss rates, and the four increments do not prove four PTOs.
This accounting defect is supported independently of WAN throughput results.

L4 reliable-stream PMTU detection retains its previous callback-based trigger
set through the separate counter. Changing that policy incidentally would have
made low-loss timeout cases behave differently. Three additional policy tests
cover L4 compatibility, DATAGRAM use of true PTOs, and counter reset; the
compatibility red run had 17 passes and one expected failure before the fix.
Twenty vendor cases cover Cubic, Reno, BBRv2 and BBRv3 across loss-only timers,
actual PTO/ACK reset, mixed timers, PMTU probe loss and ACK-driven loss.

A separate eight-second, socket-free STREAM probe dropped packets above 1,300
bytes while allowing a small stream to progress. Both PTO-only and legacy-count
variants delivered zero bulk bytes and performed zero revalidations: small
ACKs detected loss before timeout callbacks. This exposes an existing L4
policy blind spot, not a newly introduced regression or a blackhole-recovery
pass. The final change preserves that policy; broader L4 detection is deferred.

No congestion-control formula, MTU, receive window, queue/pool capacity,
quantum, GSO setting or platform network setting changes in the retained fix.

### Measurement controls and corrected route scope

Workstation comparisons use the supplied Speedtest Custom page and SmarTone
Hong Kong node, fresh loopback SOCKS sessions, Cubic, MTU 1280 and chain disabled.
Later cohorts fix outer IPv4, two runtime workers and child-only CPU affinity.
Builds and network workloads run serially; each completed session checks probe
cleanup and preservation of the original application configuration.

A later live idle H2 audit found the endpoint route selecting a pre-existing
virtual Meta Tunnel interface with MTU 4064. The earlier H2 TCP_INFO diagnostic
cohort reported MSS 4024 and minimum RTT as low as 19 microseconds. Application
`chain=false` therefore did not establish an unmediated outer connection.
Historical runs were not individually route-audited: do not assign that route
to every old H3 session, or use their ratios as pure-MASQUE acceptance. Earlier
“direct” browser results mean no temporary Usque SOCKS, not verified physical
egress. The old numeric observations and failed runs remain preserved.

A separate temporary probe binds only its own MASQUE socket and source to the
physical 2.5GbE interface. Option readback and source membership are verified
per session. An independent idle H2 source lookup agrees; its post-connect
TCP_INFO reports MSS 1400 and RTT 3093 microseconds. These checks establish the
socket configuration, not proof against all interception or per-packet egress.
They do not change global routing or the user's existing tunnel.

Windows SOCKS includes a userspace TCP stack absent from ordinary Android TUN.
Browser goodput and transport IP-byte counters use different byte domains.
The baseline path lost-byte counter is unreliable; zero is not evidence of no
loss. Sampled RSS is not peak RSS, browser ping is not p95 latency, and absent
CPU-per-accepted-byte measurements remain unknown.

### Performance candidates not retained

The WAN rows below precede the physical-socket cohort and retain that route
limitation. They justify not shipping an unproved change, not a universal
causal performance claim. Memory and loopback results have their own scopes.

| Experiment | Observation | Decision |
| --- | --- | --- |
| Two versus sixteen runtime workers | Reversed H3 upload comparisons showed no repeatable gain | Keep runtime defaults |
| Immediate mux admission polling | Memory TUN +0.4%, proxy +6.1%; both far above observed WAN rates | No demonstrated WAN bottleneck |
| Connected UDP send | 2.6 million verified loopback datagrams; about 1–2% packet-rate gain, mixed CPU | No socket/migration rewrite |
| H2 TLS ciphertext buffering | Fewer reads, inconsistent paired throughput | Restored |
| H2 lookahead/release aggregation | Three-pair down/up medians 283.1/530.0 to 265.3/521.1 Mbps | Restored; no repeatable benefit |
| H2 ready-read fairness | Paired download/upload median ratios 0.96235/0.85684; download CPU/GiB +16.9% | Rejected and restored |
| Cubic loss-restoration heuristic | Seven-pair download/upload median ratios 0.9972/0.9962 | Rejected and restored |
| Larger inner MTU | Both guarded 1280 runs aborted during PMTU revalidation; 1400 never ran | No MTU comparison or default change |
| H3 memory send service | Verified pair throughput about 4.8 Gbit/s, sender service about 7.3 Gbit/s | Excludes actor, socket, WAN and device paths |

The earlier seven-pair PTO-only cohort had paired down/up median ratios
0.9662/1.0659 and sampled post-stable revalidation in 6/7 controls versus 0/7
candidates. Its receive DATAGRAM local-drop totals were 931 versus 1373;
unfavorable samples remain included. It predates explicit L4 compatibility
and verified socket binding. It is historical attribution, not acceptance of
the final candidate or proof of Android improvement.

### Final compatibility candidate: separate physical-socket comparison

Seven alternating pairs (501–507) complete, excluding calibration 401 and
unbound pairs 101–107. Control restores the old unconditional PTO count while
retaining the same L4 callback field and temporary binding; candidate uses true
PTO. Neither instrumented executable is a stock APK. All 14 sessions verify
binding, the expected settings, one connection, normal exit, listener closure
and original-configuration SHA-256 preservation. All low results remain.

| Direction | Control median (MAD), Mbps | Candidate median (MAD), Mbps | Median paired ratio (MAD) |
| --- | ---: | ---: | ---: |
| Download | 914.5 (31.1) | 935.0 (47.7) | 1.03207 (0.06772) |
| Upload | 522.7 (16.0) | 509.4 (35.8) | 1.02028 (0.06029) |

Five download pairs and four upload pairs improve. Group medians and paired
ratios differ; these results do not establish a repeatable throughput gain.
Neither group shows a sampled post-stability PMTU revalidation. Upload/download
CPU paired medians are 1.00604/0.93370; normalized CPU/GiB remains unknown.
Sampled RSS is not peak RSS, so no complete resource/latency budget passes.

The exported combined DATAGRAM-drop delta totals 3,707 versus 5,638. Available
send-queue drop deltas are zero throughout the covered fresh sample windows,
allowing these totals to be attributed to the receive-overflow counter there.
The overflow occurs in quiche's 64-entry receive queue; a full actor-to-mux
channel preserves its application batch but can indirectly prevent draining.
Telemetry-inferred upload intervals account for subtotals 2,886 versus 4,593;
exact browser-phase totals are unknown because phase timestamps were not saved.
This does not identify the dropped payloads as TCP ACKs or prove a speed limit.

Control SHA-256: `8fcececa0b6249a51fc414cd3de5eafbd2d98aa678d068168c473f7f5debb921`.
Candidate SHA-256: `62894f4d1914fae92bff45a580058d53308ad80cd903e04036ff0ee853b66b30`.
An independent H2 diagnostic run (601) observes actual stream credit immediately
after existing capacity releases, without changing the window or release count.
All 37 samples over 51.227 seconds have available credit 4,186,609–4,194,304
bytes and available-plus-used 4,194,304 bytes. There is one observer, verified
physical-socket binding, MSS 1400, post-connect RTT 2884 microseconds, normal
cleanup and unchanged configuration. Browser down/up is 177.1/635.1 Mbps.
Thus the observed local stream credit exceeds the default 65,535-byte window;
it does not measure peer receipt of WINDOW_UPDATE or connection-level credit.
Separate getter reads are not atomic. Low-frequency observation can perturb
timing, so this is a diagnostic run, not an uninstrumented performance result.

### Validation of the retained PTO and L4 compatibility source

Native tests use the supported Windows helper environment. The standalone
vendor commands restore default Rust flags; all Cargo operations are locked.
Counts below belong to the final compatibility source, not the older PTO-only
run (1,293 workspace passes and 1,084 vendor passes). Ignored tests are not run.

| Exact command from repository root | Result |
| --- | --- |
| `cargo fmt --all --check` | exit 0 after correcting one test-format discrepancy |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | exit 0 |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | exit 0; 1,296 passed, 8 ignored across 22 suites |
| `cargo test --manifest-path third_party/quiche-0.29.3/Cargo.toml --locked --lib --config profile.dev.package.boring-sys.opt-level=1 --config profile.dev.package.boring-sys.debug=false` | exit 0; 1,088 passed, including 20 new accounting cases |
| `cargo test --manifest-path third_party/quiche-0.29.3/Cargo.toml --locked --lib --features qlog --config profile.dev.package.boring-sys.opt-level=1 --config profile.dev.package.boring-sys.debug=false recovery::gcongestion::bbr3::` | exit 0; 20 passed |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | exit 0; compile only |
| `& .\tool\build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | exit 0 on final compatibility source |
| `& 'C:/Users/George/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe' tool/check_repository_policy.py` | exit 0; verified Python 3.12.14 |
| `git diff --check` | exit 0 |

Temporary probes and the engine example are removed; guarded restores verify
the original source bytes before the production release build. Rust and technical records are
the only retained changes; Flutter, Kotlin, protobuf and aggregate multi-language
checks are not applicable. No parser, TLS, authorization or cleanup rule is
relaxed. No packet contents, addresses or credentials enter the retained change;
raw diagnostics and test binaries stay in ignored local evidence.

No package installation, VPN/TUN creation, system proxy, DNS, route or WFP
mutation was performed. Android device, isolated lifecycle/leak and controlled
performance-lab validation remain `not_run`. No optimal configuration, blanket
performance-budget pass or proven phone throughput improvement is claimed.
