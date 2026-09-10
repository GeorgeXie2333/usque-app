# L4 download optimization: round one

This change starts from `92eeb6ea116e874c0c3e763abea88e72697ba372`.
It removes avoidable adapter copies and receive allocations, not the necessary
copy into smoltcp's TCP ring or the operating system. Offline assertions are
cost/correctness evidence, **not throughput measurements**.

Follow-up: the later [QUIC UDP receive-buffer policy](UDP_RECEIVE_BUFFER.md)
promotes a 2 MiB socket receive target on Windows/Android after separate A/B
feedback. That is not a result of this first-round copy/allocation change; the
historical checks below apply only to their stated candidate.

## Unchanged production behavior

The local stack, QUIC/TLS libraries, MTU 1280 default, QUIC windows, TCP preferred
and fallback tiers, worker count, and total application buffer budgets are
unchanged. Auto, identity-derived SNI, pin checks, DNS, UDP rejection, precise
socket authorization, and Kill Switch behavior are unchanged. Config version
remains 15. No new setting, production environment override, or public CLI was
added. The performance submessage uses new `L4Snapshot` protobuf field 23.

## Ownership and scheduling

For tunneled L4 TCP, HTTP/3 DATA becomes an owned `Bytes` chunk. A single-owner
duplex relay passes a slice handle to the local TCP command interface and only
advances after the stack reports how many bytes it accepted. Partial writes do
not recopy the remaining payload. At most one download chunk and the existing
bounded upload allocation are held by this relay. EOF closes a direction only
after its remainder drains; errors and cancellation do not replay accepted data.
Direct and non-owned stream implementations retain the generic relay.

The runtime shares one on-demand 32 KiB receive pool between main/draining QUIC
sessions. Idle capacity is 2 MiB on desktop/Android64 and 512 KiB on Android32.
The allocation's original budget lease survives all slices and idle caching.
Admission/allocation pressure evicts idle cache before rejecting work. Weak
return handles prevent cycles; close clears idle data, and late returns release
their leases without entering another runtime. Evicted allocations are cleared.

Budget notifications target registered waiters. A waiter registers before
rechecking capacity, covering release-before-registration. Returning memory with
no waiter does not wake a data actor. H3's existing read limit and fairness work
budget are retained. Android drains only already-ready L4 egress packets, at most
16 packets / 64 KiB / a 200 microsecond soft batch budget. Every packet checks
cancellation; WouldBlock retains the same pending packet and returns control.
The batch adds no packet array or separate cache: the already-budgeted single
pending slot is reused. Each IP packet is still one write, never concatenated.

## Reading diagnostics

`l4.performance` is optional. Missing messages and unavailable socket/platform
observations remain unknown. Counters are atomic and summarized once per second;
wait histograms have 32 logarithmic microsecond buckets and sample 1/64 events.
No samples means no measured percentile. Concurrent counters are approximate
snapshots, not a transactionally consistent packet trace.

- `h3_read_*`: HTTP/3 body read calls, successful bytes, empty/Done reads.
- `receive_pool_*`: allocations, reuse, evictions, live/idle bytes and peaks.
  Live includes idle; do not sum them. They are **not process RSS**.
- `adapter_copied_bytes`: explicit L4 AsyncRead/TUN adapter payload copies.
  It excludes mandatory stack/kernel copies and may include proxy/DNS reads.
- `tcp_accepted_bytes`, `tcp_write_calls`, `tcp_partial_writes`: local TUN TCP
  command acceptance; `command_wait` measures sampled command round trips.
- Queue snapshots: occupancy/high water and sampled residence, including the
  packet currently staged by the stack. Reservation precedes reliable enqueue.
- TUN ingress counts successful bridge enqueue; egress counts bridge dequeue,
  **not confirmed kernel delivery**. Android `tun_write_calls` counts actual
  write syscalls, `tun_write_would_block` counts their WouldBlock results, and
  `tun_write_wait` measures completed packet writes, including pending time.
- Actor polls/wakes/no-progress polls, no-progress wakeups, and budget wake counts
  describe local stream scheduling, not a QUIC congestion signal. A no-progress
  wakeup is a registered actor wake followed by a poll with no stream progress;
  network-driven no-progress polls are counted separately.
- TCP tier counts/bytes describe the live local TCP allocator.

These are different stages of the same data. Do not add their byte counters into
"total traffic". Existing application/outer-transport traffic semantics remain.
Socket buffer observations are raw OS `getsockopt` values; Linux may report
accounting-adjusted sizes. Requested values are not measurements. MTU source is
the applied runtime profile, not a not-yet-applied saved preference.

Android diagnostics record `app_debuggable` separately from native version,
architecture, and compiled `debug_assertions`. An old JNI library may lack native
build information; absence remains unknown. Windows includes native build info.
The comparison manifest records the exact commit and artifact hash; neither JNI
build grade nor artifact identity is inferred from a displayed version string.
No targets, DNS contents, connection IDs, credentials, or raw headers are added.

## Validation and controlled comparison

The offline suite checks 32 KiB delivered through 1 KiB partial accepts, byte
identity/backing allocation, warm reuse, cache eviction, cancellation and waiter
races. It retains two-thread duplex pressure and two-second cleanup, adds
16/32-connection background loads, continuous downloads and MTU variants, and
keeps the five-second native stop-confirmation boundary unchanged.

Internal `cfg(test)` runtime options select **one** factor: TUN MTU
1280/1500/4096/9000, UDP receive buffer OS default/2/4/8 MiB, or QUIC initial
stream window unchanged/1/2 MiB. They compile out of production, use ordinary
socket options (no force/sysctl/privilege changes), and check the effective OS
buffer. These entry points are not a device performance harness or report.

[The sampling manifest](l4-performance-scenarios.json) retains the H3 baseline
and adds an independent before/after L4 comparison. Real release throughput,
CPU/GB, RSS, tail latency, Android device lifecycle and external leak observations
are `not_run` on this workstation. Later measurements require matched build
grade/configuration/targets, at least seven repetitions, and exact candidate
metadata. No claim of matching Bettbox or measured Mbps improvement is made.
Protected evidence remains supplemental, not a new publication prerequisite.

## Workstation verification, 2026-09-10

This records the working-tree implementation above the stated baseline, not a
signed release or a protected performance report. Tool versions and full command
requirements remain in [CONTRIBUTING.md](../CONTRIBUTING.md). Flutter commands
used the verified pinned SDK (3.44.7, revision
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`) and locked dependency resolution.

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| `tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed, locked workspace/all-targets, warnings denied |
| Same helper with `-CargoAction test` | 916 passed; 2 existing live checks ignored, not passed |
| Same helper's default release build | Passed, compile-only |
| Locked helper-built transport test executable with filter `l4::`, repeated 10 times | 31 passed per repetition, including two-thread pressure, owned downloads, cancellation and cleanup |
| `cargo test --manifest-path third_party/ts_netstack_smoltcp_core/Cargo.toml --lib --locked` | 6 library contract tests passed |
| `flutter pub get --enforce-lockfile`; Dart format check; `flutter analyze --no-pub` | Passed |
| `flutter test --no-pub` | 413 passed, including Windows golden tests and the new performance codec/model tests |
| `prepare_windows_plugin_junctions.ps1 -FlutterProject .`; `flutter build windows --release --no-pub` | Passed, compile-only |
| `tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed |
| Same Android helper with `-AbiFilter all -CargoAction build` | arm64-v8a, armeabi-v7a, x86_64 debug JNI compiled; generated libraries remain ignored |
| `flutter build apk --debug --config-only --no-pub` | Passed; configuration only |
| Gradle `:app:ktlintCheck`, `:app:testDebugUnitTest`, `:app:lintDebug` | Passed; 169 JVM tests, no failures/errors |
| Buf 1.72.0 lint, format check, FILE breaking against baseline with `--against-config buf.yaml` | Passed |
| Ruff 0.16.0 check/format, `py -3 -m unittest discover -s tool -p 'test_*.py' -v` | Passed; 71 Python tests |
| Both individually invoked PSScriptAnalyzer 1.25.0 passes | Completed with zero findings |
| `actionlint -no-color` (1.7.12) | Passed |
| Frozen oracle `go mod verify`, `go test ./...`, `tool/verify_oracle_archive.py` | Passed; 41 archived files verified, oracle unchanged |
| Repository policy and `git diff --check` | Passed |

Two unsuccessful broader check attempts are **not** represented as passes:

- `tool/check_source.ps1`, after helper initialization and adding the pinned SDK
  to that shell's PATH, stops at its forced PSScriptAnalyzer import. Windows
  Software Restriction Policy rejects `ScriptAnalyzer.format.ps1xml`. The
  aggregate check is **blocked**, despite the separately completed checks. No
  policy, signature, module file or check was weakened to bypass this result.
- Running the vendored core's entire standalone test target (without `--lib`)
  cannot compile its existing `tests/udp.rs`, which references the undeclared
  `ts_cli_util` helper. The library tests and first-party stack/QUIC contracts
  above pass. No dependency or frozen/vendor source was changed to hide this
  unrelated integration-fixture limitation.

Deterministic cost evidence: the borrowed TUN write-adapter control copies
528 KiB while delivering a 32 KiB payload in 1 KiB accepts; the owned adapter
copies **0** payload bytes and preserves the backing address of every remaining
slice. Both deliver exactly 32 KiB in 32 commands (31 partial accepts). In the
serial H3 owned-read fixture, 64 chunks require at most two data allocations and
at least 63 pool hits. These bounds and byte identities are asserted in tests;
they do not measure allocator metadata, process RSS, CPU/GB, or Mbps.

No MSI/APK installation, real Windows TUN/WFP/route/DNS mutation, Android device
exercise, protected-runner job, external leak observation, or release throughput
measurement was performed. Build output and local logs are not committed.
