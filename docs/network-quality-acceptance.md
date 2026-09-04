# Network-quality integration acceptance

## Later dependency-contract correction

The historical matrix below describes its original candidate and tests. The
review of `423d99c` later found runtime-path PMTUD inheritance and DATAGRAM
admission defects that the original app-level state tests did not cover. See
[the confirmed issue list and wire-level regression coverage](pmtu-path-fixes.md)
for the subsequent correction; the historical pass entries are not proof that
those dependency contracts were already satisfied.

## Candidate and evidence rules

The sequence begins at `3a032e7a56ed3078c58470790893590edbf2062b`.
PR-00 froze the baseline in `9293af5`. Implementations are PR-01 `d45c8dd`,
PR-02 `98ecc1e`, PR-03 `d7bec07`, PR-04 `f5cf93d`, PR-05 `c7718fe`,
PR-06 `91e0b22`, PR-07 `c5e50d3`, PR-08 `4561896`, PR-09 `75cdf49`,
PR-10 `121abf0`, and PR-11 `1558ebf`. **PR-12** below means the integration
commit containing this document, flags, fault hooks, native timeline and final
cross-language checks. The execution report records its full final SHA.

“Ordinary” means unit/property/in-memory/loopback tests or compile-only checks,
not a device claim. Every protected artifact in this run is `not_run`: no
snapshot VM, dedicated Android device, independent network observer or
controlled performance lab was invoked. No 7-run improvement, high-BDP budget,
one-second migration p95 or zero physical port-53 result is claimed. Absence
does not gate publication; a supplied report must still validate strictly.
There was no installation, live VPN, platform-network mutation, release APK,
MSI packaging, official signing, upload, tag movement or publication.

## Final ordinary matrix

| Check | Result / scope |
| --- | --- |
| Windows Rust format, locked Clippy and workspace tests | Passed; transport 273, core 103, Engine 89 with 2 protected tests ignored, Android JNI host tests 30, Agent 88, IPC 24; all other workspace suites passed |
| Windows x64-v2 Rust release | Passed using `tool/build_windows_rust_release.ps1`; no executable launched |
| Linux Rust 1.97.1 locked workspace Clippy/tests | Passed in Debian WSL; transport 276 including real Linux batch loopback/address-layout branches, core 103, Engine 66, Android host 30, Agent 41, IPC 24; other suites passed |
| Explicit debug fault-injection feature | Linux workspace Clippy passed; production-style no-debug-assertions build is rejected by the compile guard |
| Android Rust | Pinned helper arm64 Clippy passed; Gradle compiled arm64-v8a, armeabi-v7a and x86_64 JNI |
| Kotlin | ktlint and lint passed; 149 tests across 19 suites, zero failures/errors |
| Flutter | Locked pub resolve, 83-file format check, analyze, 227 tests passed; eight synthetic Quality golden states retained |
| Windows Flutter release | Plugin-junction preparation and `build windows --release --no-pub` passed |
| Python | Ruff lint/format and 57 unit tests passed, including exact 39-check bilingual/export catalog and legacy performance-alias rejection |
| Source/protocol/workflow | Aggregate check, both PSScriptAnalyzer passes, Buf lint/format and breaking against the complete plan baseline, actionlint, repository policy and diff whitespace checks |
| Frozen Go oracle | `go mod verify`, `go test ./...` and archive verification; Go packages report no test files, so this is dependency/compile/archive evidence, not live H2/H3 interoperability |

Linux emits five upstream-generated BoringSSL binding `unnecessary_transmute`
warnings. Workspace `-D warnings` gates pass; generated/vendor sources and
lint policy were not edited or suppressed to hide them. WSL is ordinary local
validation, not a protected snapshot VM, observer or performance laboratory.

## Definition of Done: all 40 planned items

Paths in the test column are repository-relative. Short test names refer to
the named file's test module. A protected `not_run` cell is explicitly missing
evidence, never pass; numeric performance rows are not measured here.

| ID / planned item | Code commit / source | Ordinary evidence | Protected artifact / status | Documentation |
| --- | --- | --- | --- | --- |
| H2-1 Explicit CONNECT-IP Builder | `d7bec07`, PR-12; `crates/usque-transport/src/h2.rs` | Builder/push/32 flag-combination window tests | `not_run`: observer | [metrics](network-quality-metrics.md), [rollback](network-quality-rollback.md) |
| H2-2 Reviewed production windows, missing lab data explicit | `d7bec07`, `f5cf93d`, PR-12 | Large receive, capacity, cancellation tests; production 4/8 MiB, rollback 65,535 | `not_run`: high-BDP lab | [testing](RELIABILITY_TESTING.md), [rollback](network-quality-rollback.md) |
| H2-3 PING RTT, windows and capacity stall visible | `d7bec07`, `98ecc1e`, `1558ebf` | `h2.rs` PING timeout/EWMA/stall and telemetry tests; codec/UI tests | `not_run`: device/lab | [metrics](network-quality-metrics.md) |
| H2-4 High-BDP within budget | `f5cf93d`; `tool/performance_budget.json` | `tool/test_performance_gate.py` validates boundaries, not measured performance | `not_run`: high-BDP lab | [testing](RELIABILITY_TESTING.md) |
| MET-1 H3/H2/UDP/pool/migration/DNS metrics connected | `d45c8dd` through `1558ebf`, PR-12 | `network_quality.rs`, Engine codec, Kotlin fields/native timeline and Flutter model tests | `not_run`: platform/lab | [metrics](network-quality-metrics.md), [IPC](network-quality-ipc.md) |
| MET-2 Unsupported/not-ready/stale are not zero | `d45c8dd`, `98ecc1e`, `1558ebf` | H2 QUIC-only Unsupported, pending PMTU, stale PING and missing-field tests | `not_run`: device | [metrics](network-quality-metrics.md) |
| MET-3 Bounded 1 Hz local events | `d45c8dd`, `98ecc1e`, PR-12 | Sampler cancel/rollback, latest-value watch/event coalescing, GUI single-flight tests | `not_run`: device | [IPC](network-quality-ipc.md) |
| MET-4 Strict v2 reports without publication-policy change | `f5cf93d`, PR-12 | `test_performance_gate.py`, `test_reliability_gate.py`, release-workflow policy tests | `not_run`: all seven lab gates | [testing](RELIABILITY_TESTING.md) |
| MET-5 Missing/not_run/unstable cannot pass | `f5cf93d`, PR-12 | Missing/hash/environment/sample-count/MAD/legacy-alias rejection tests | `not_run`: lab | [testing](RELIABILITY_TESTING.md) |
| MIG-1 Prefer same-family migration | `4561896`, `75cdf49`, PR-12 | `netstack.rs::migration_policy_keeps_h2_and_cross_family_changes_on_full_reconnect`; actor tests | `not_run`: adapter/device | [paths](h3-path-infrastructure.md) |
| MIG-2 No client application send before validation/promotion | `4561896`, `75cdf49` | `h3.rs` migration barrier; full H3 loopback marker assertions | `not_run`: independent candidate observer | [paths](h3-path-infrastructure.md) |
| MIG-3 Atomic promotion | `75cdf49` | `h3/migration.rs::inv_migration_candidate_no_app_send_validated_atomic_promotion_and_grace` | `not_run`: observer | [paths](h3-path-infrastructure.md) |
| MIG-4 No old-path application send after promotion | `75cdf49` | Same loopback verifies active/retiring ownership and grace cleanup | `not_run`: independent old-path observer | [paths](h3-path-infrastructure.md) |
| MIG-5 Failure returns to safe full reconnect | `75cdf49`, PR-12 | CID/validation/setup/cancel/connection-close faults, disabled-handle dispatch and supervisor policy | `not_run`: adapter/device | [rollback](network-quality-rollback.md) |
| MIG-6 p95 interruption <=1000 ms | `f5cf93d`; performance budget/scenario | Migration-threshold and raw-sample evaluator tests | `not_run`: migration lab | [testing](RELIABILITY_TESTING.md) |
| UDP-1 Linux/Android batch or one stable fallback | `c7718fe`, PR-12 | Linux real batch loopback; unsupported/partial fallback tests; three Android ABI builds | `not_run`: Android device/lab | [metrics](network-quality-metrics.md) |
| UDP-2 Portable behavior on Windows/other targets | `c7718fe`, PR-12 | Portable order, oversize rejection, cancel and forced-rollback tests | `not_run`: protected device | [rollback](network-quality-rollback.md) |
| UDP-3 Syscall/datagram and allocation acceptance | `f5cf93d`, `c7718fe`, `91e0b22` | Counter/ownership/microbenchmark correctness and evaluator boundary tests | `not_run`: batch/allocation lab | [testing](RELIABILITY_TESTING.md) |
| UDP-4 Bounded pools/queues/unsafe | `c7718fe`, `91e0b22`, `4561896`, PR-12 | 192-buffer pool pressure/cancel/reclaim, byte budgets, sockaddr bounds, partial-send properties | `not_run`: device/lab | [paths](h3-path-infrastructure.md), audit below |
| PMTU-1 H3 DPLPMTUD default enabled | `c5e50d3`, PR-12 | Locked quiche config and PMTU state tests; rollback fixed1350 | `not_run`: PMTU lab | [metrics](network-quality-metrics.md), [rollback](network-quality-rollback.md) |
| PMTU-2 Per-path state and migration revalidation | `c5e50d3`, `75cdf49` | `pmtu.rs` promoted-path reset and migration PMTU-once tests | `not_run`: adapter/device | [paths](h3-path-infrastructure.md) |
| PMTU-3 Drop without truncation or spin | `c5e50d3`, PR-12 and follow-up fixes | MTU1500-to1280, separate discovery/revalidation budgets, typed startup failure, burst/revalidation-during-migration and receive-metadata tests | `not_run`: PMTU lab | [invariants](reliability-invariants.md) |
| PMTU-4 Oversized inner packet preserves PTB | `c5e50d3`, `91e0b22` | `icmp.rs`, `h3.rs`, packet-batch size/encode boundary tests | `not_run`: observer | [metrics](network-quality-metrics.md) |
| PMTU-5 H2 Unsupported | `d45c8dd`, `98ecc1e`, `1558ebf` | `h2_marks_quic_only_metrics_as_unsupported`; H2 golden/model | `not_run`: device | [metrics](network-quality-metrics.md) |
| UI-1 Complete page and 60-second trends | `1558ebf` | `apps/usque_gui/test/network_quality_screen_test.dart`, controller tests/goldens | `not_run`: dedicated device | [Doctor](network-doctor.md) |
| UI-2 Standard read-only, <=2 s target | `1558ebf` | Engine before/after equality, runner deadline, Kotlin snapshot/budget tests | `not_run`: real-device timing | [Doctor](network-doctor.md) |
| UI-3 Explicit Deep, <=15 s, cancel and cleanup | `1558ebf`, PR-12 | Session/group/dependency/deadline tests; loopback DNS/QUIC socket-permit cleanup; confirmation UI | `not_run`: protected device | [Doctor](network-doctor.md) |
| UI-4 Local results do not claim external leak proof | `1558ebf`, PR-12 | Finding/export code allowlists, UI text and observer separation | `not_run`: external observer | [Doctor](network-doctor.md), [threat model](direct-dns-threat-model.md) |
| UI-5 EN/ZH, themes, 200%, keyboard/TV/screen reader | `1558ebf`, PR-12 | 227 Flutter tests incl accessibility/goldens; exact bilingual 39-check catalog test | `not_run`: physical TV/device | [Doctor](network-doctor.md) |
| DNS-1 Profile System/DoH/DoT | `121abf0`, `1558ebf`, PR-12 | Core schema12-to13/default/canonical tests; Dart editor; Android startup-mode test | `not_run`: device | [DNS/schema](encrypted-direct-dns.md) |
| DNS-2 Protected sockets, bootstrap and strict TLS | `121abf0`, `1558ebf` | `encrypted_dns/tests.rs` strict chain/name/expiry/ALPN, spy protection and bootstrap tests | `not_run`: platform binding | [DNS](encrypted-direct-dns.md), [deployment trust](direct-dns-threat-model.md) |
| DNS-3 No plaintext fallback | `121abf0`, PR-12 | Fake protector rejects system lookup; SERVFAIL and post-answer IP-reuse tests; capability rejection | `not_run`: external port53 observer | [DNS](encrypted-direct-dns.md) |
| DNS-4 Bounded pool/timeout/retry/generation | `121abf0`, PR-12 | Pool64/four-I/O, idle/recycle, Happy Eyeballs, deadline/cancel/generation/fault tests | `not_run`: device/lab | [DNS](encrypted-direct-dns.md) |
| DNS-5 Observer confirms encrypted physical port53=0 | `f5cf93d`, `121abf0` | Strict external evidence/report validator only, not a packet observation | `not_run`: external DNS observer | [testing](RELIABILITY_TESTING.md) |
| DNS-6 System mode preserved | `121abf0`, PR-12 | Physical UDP truncation-to-TCP, stale-response, default-profile and system-fallback tests | `not_run`: platform regression | [DNS](encrypted-direct-dns.md) |
| ALL-1 New invariants have automated evidence | PR-01–PR-12 | State/codec/property/loopback/fault/cleanup/export tests mapped above | `not_run`: actual platform observations | [invariants](reliability-invariants.md) |
| ALL-2 All ordinary checks | PR-12 | Exact matrix above; command/results retained in local execution report | `not_run`: no protected substitute | [contributing](../CONTRIBUTING.md) |
| ALL-3 Exact-candidate artifacts or explicit not_run | `f5cf93d`, PR-12 | Forged/wrong candidate/environment/artifact/hash rejected by evaluator tests | `not_run`: all four protected environments | [testing](RELIABILITY_TESTING.md) |
| ALL-4 No out-of-plan product expansion | PR-00–PR-12 | Baseline-to-candidate scope review; no platform mutation or publication | `not_run`: not a protected claim | [implementation](IMPLEMENTATION.md) |
| ALL-5 README/implementation/reliability/testing/privacy/rollback docs | PR-12 | Repository link/policy check; scoped threat-model source-reference check | `not_run`: not a protected claim | [README](../README.md), [rollback](network-quality-rollback.md) |

## Composed fault matrix

| Planned scenario | Ordinary evidence | External/performance status |
| --- | --- | --- |
| Stable H3 + batch + PMTUD | H3 loopback, Linux batch loopback, shared-pool/counter and PMTU tests | Stability/zero-drop/performance gate measurements `not_run` |
| PMTU drop then network switch | Per-path drop/revalidation plus promotion invalidation and wire bounds | Adapter transition `not_run` |
| Candidate validation timeout/rejection | Canonical fault hook consumes candidate, releases lease/task, leaves active path | External path observer `not_run` |
| EMSGSIZE during migration | Revalidation-failure hook/circuit breaker, candidate abort and resource reclamation | No-spin lab measurement `not_run` |
| Encrypted query during generation change | Same authoritative generation invalidates DNS pool and migration candidate; queued reply SERVFAIL | Device/observer `not_run` |
| H2 fallback | H2 real PING, QUIC fields Unsupported, Builder/capacity/cancel tests | High-BDP performance `not_run` |
| DoH bad certificate / HTTP / body | Strict loopback PKI and canonical hooks fail without physical lookup | Physical port53 observer `not_run` |
| Full DoT pool | 4 live I/O / 64 query permits, bounded wait/deadline/cancel tests | Device/lab `not_run` |
| Slow TUN sink | Existing bounded drop policy, queue byte/item pressure and independent proxy routing tests | Real TUN/lab `not_run` |
| GUI disconnect/reconnect | Latest-value watch, no backlog, 60-slot gaps, late-reply epoch rejection | Device `not_run` |
| Old Engine / GUI fields | Append-only wire fixtures, missing capability, unknown fields, optional JNI timeline | Installed-version matrix `not_run` |
| Android background / Doze | Compile/JVM generation, cancellation and bounded bridges | Actual Doze/device lifecycle `not_run` |
| Windows adapter change | Physical-route source selection, same-family actor or full reconnect, Agent lease tests | Snapshot VM/Kill Switch observer `not_run` |

## Unsafe, privacy and cancellation review

New native unsafe code is limited to the Unix batch syscall/address boundary,
Windows read-only IP Helper route observation, and exported JNI ABI symbols.
The batch review verified initialized bounded mmsghdr/iovec/address arrays,
stable disjoint receive buffers, borrowed send payload lifetime, nonblocking
flags, syscall return-count bounds, MSG_TRUNC rejection, native alignment and
family-length checks. Linux executes these branches; all three Android ABIs
compile them. Windows route-table ownership uses one FreeMibTable guard,
bounded NumEntries, an allocation-tied slice and initialized read-only output
structures. No platform mutation is added by observation. JNI entry points use
the existing environment/error wrapper and bounded numeric/enum serialization;
the timeline bridge introduces no raw-pointer dereference.

Cancellation review covered H2 writer/PING abort ownership; bounded UDP pool
waits; one H3 actor and atomic non-awaiting role swap; candidate supersession,
deadline and retiring-socket joins; DNS generation tokens, I/O-owned permits,
partial DoT discard and DoH reset; Deep probe permit drain; sampler cancellation;
GUI epoch/single-flight handling; and native timeline timeout/destroy/late reply.
No new unbounded channel was introduced in the plan diff.

The selected H3 encoder remains the single `ownership_transfer` runtime
strategy; alternative measurements are test fixtures, not selectable runtime
branches. The performance legacy alias is absent from the executable catalog
and explicitly rejected by test. Diagnostic logs use fixed reason codes, numeric
counts and address-family labels, not raw errors or endpoints. Historical
baseline docs and test-only malicious strings are not live logging paths.
All protected assertions still require their real isolated evidence.
