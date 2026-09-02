# Reliability test environments

Usque separates deterministic pull-request evidence from destructive or
network-observer release evidence. Missing infrastructure is never converted
to a pass, and protected-runner availability does not gate publication.

## Pull-request gate

The normal CI workflow runs locked Rust tests and Clippy, protobuf wire
fixtures, the diagnostic runner and fault-injection tests, Android JVM tests,
Flutter analysis, controller tests, responsive widget tests, and diagnostic
bundle privacy tests. It also runs the performance-v2 parser, JSON-schema
contract, fixture reproduction, budget-boundary arithmetic, and deterministic
microbenchmark correctness tests. Shared-runner wall-clock timings are never
treated as performance truth. These jobs do not require a public endpoint and
do not change host routes, DNS, firewall, proxy, or TUN state.

## Protected release runners

After the exact signed candidate has been staged, the public release workflow
selects four explicitly labelled self-hosted runners only when repository
variable `RUN_PROTECTED_RELEASE_VALIDATION` is exactly `true`. Publication does
not wait for these supplemental jobs:

| Runner label | Required isolation | Scope |
| --- | --- | --- |
| `usque-snapshot-vm` | Windows snapshot VM with an independent management channel | Clean install, upgrade, connected uninstall, Engine/Agent termination, sleep/network change, route/DNS/WFP/proxy restoration, Wintun residual checks |
| `usque-android-device` | Dedicated physical Android device controlled by ADB | Wi-Fi/cellular changes, airplane mode, Doze, lock/unlock, UI/VPN process reclamation, Always-on, Lockdown, reboot, upgrade, TV background/foreground |
| `usque-network-observer` | Controlled gateway outside the Engine process | H3/H2 endpoint behavior, separate IPv4/IPv6 assertions, DNS/Kill Switch/route/direct-rule packet observation |
| `usque-performance-lab` | Stable host, endpoint, topology, load and thermal policy | Seven raw baseline and candidate samples for H2 high-BDP, H3 batch I/O and allocations, queue pressure, PMTU convergence, QUIC migration and encrypted DNS |

Each runner supplies a protected `usque-reliability-runner` executable. The
repository workflow passes it the exact candidate directory, commit and output
directory. The Windows command is additionally guarded by
`USQUE_ISOLATED_SNAPSHOT_VM=1`. Do not provision these runner labels on a
developer workstation.

## Reliability report contract

Every runner produces `report.json` with:

- schema version, exact commit and SHA-256 of `release-manifest.json`;
- an allowlisted environment class without a device identifier, SSID or user
  path;
- one result per required gate with `passed`, `failed` or `not_run`;
- JUnit, connection-timeline and platform-diff evidence references. Every
  reference contains a relative `path` and SHA-256, and the file must be a
  non-empty regular file below that runner class's evidence namespace;
- for independent leak gates, an external-observer marker, a zero-unexpected-
  packets assertion and a restricted PCAP path.

Each report is downloaded under its fixed artifact name and is accepted only
for the matching protected runner class: Windows, Android, independent network
observer, or performance lab. Evidence paths must start with the same
environment kind (for example `windows_snapshot_vm/`) and are resolved below a
separate restricted evidence root; traversal, symlinks, missing files, digest
mismatches, empty files, and oversized files fail closed.

`tool/reliability_gate.py` rejects unknown gates, wrong environments,
duplicates, missing or forged evidence, candidate digest mismatches, `failed`,
`unstable`, and `not_run`. It emits the validated `reliability-report.json` and
`device-matrix.md` only when all required gates pass. The release workflow keeps
that validated summary as a protected Actions artifact; missing or failed
optional runs produce no summary and do not block publication. PCAPs stay in
restricted CI artifacts and are never copied into the public diagnostic bundle
or GitHub release.

The performance-lab report replaces the old
`performance.informational_baseline` result with these required results:

- `performance.h2_high_bdp`
- `performance.h3_batch_io`
- `performance.h3_allocation_rate`
- `performance.queue_pressure`
- `performance.pmtu_convergence`
- `performance.quic_migration`
- `performance.direct_dns`

There is no compatibility alias: the legacy result is rejected as an unknown
gate, including if marked passed. Historical PR-00 baseline documentation is
not an active gate definition. The final [acceptance matrix](network-quality-acceptance.md)
records ordinary checks separately from unavailable protected measurements.

Each result binds five artifacts: JUnit, timeline, platform diff, a v3
comparison report, and its raw-sample bundle. The reliability aggregator checks
both artifact SHA-256 values, verifies the hashes of the embedded baseline and
candidate reports, and recomputes the comparison from the checked-in scenario
and budget contracts. A runner-authored summary cannot substitute for raw
evidence.

## Instance-local fault injection

`usque-transport` extends its existing `FaultScript` (256 events) with H2
PING/capacity, batch partial/unsupported/truncated/WouldBlock, pool exhaustion,
CID/candidate setup/validation, EMSGSIZE/PMTU, DoH TLS/HTTP/body, DoT prefix/EOF
and DNS-pool cancellation points. Each due event is consumed once at the real
component boundary. Scripts are per telemetry/runtime instance, not global;
capacity-delay injection is at most ten seconds and partial counts are 1–64.
Unit tests may inject scripts; explicit `fault-injection` lab builds must
retain debug assertions. Non-test release builds reject that feature at
compile time. There is no Profile, environment, IPC or remote fault interface.
Synthetic faults verify cleanup/state logic, not external packet observations
or performance targets. The seven v2 gates still require actual lab samples.

## Performance samples v2 and evidence bundles v3

`tool/schemas/performance_report.schema.json` is the wire contract. A measured
baseline or candidate report contains exactly seven ordered raw samples. Units
are part of field names (`goodput_bps`, `latency_p95_us`, `cpu_time_ms`, and
`rss_peak_bytes`); unknown or unit-substituted fields fail closed. Environment
data is limited to an allowlisted network-profile ID, stable thermal and battery
policies, and numeric tool versions. Hostname, username, SSID, IP address, and
device-serial fields are forbidden. A report that could not be measured has
status `not_run`, an allowlisted reason code, and no samples; it is never a
pass.

Baseline and candidate must use the same scenario, platform class, network
profile, and major runner/Rust toolchain. `tool/performance_gate.py` calculates
the median, minimum, maximum, and median absolute deviation (MAD) over the seven
runs. Latency is the median of the seven per-run p95 values. Throughput
MAD/median above 10% or latency MAD/median above 15% yields `unstable`, never a
best-run selection.

`tool/performance_budget.json` records the numeric contract. Steady-state
limits are inclusive: throughput at least 95% of baseline, median p95 latency at
most 110%, CPU per bit at most 105%, and RSS at most 110% with no more than a
32 MiB absolute increase. The scenario-specific checks additionally require
zero reference queue drops, no more than 0.5 UDP syscalls per datagram,
allocation and feature-acceptance limits, PMTU stability within 30 seconds with
no send-error spin or silent truncation, same-family migration p95 within one
second and fallback within eight seconds, and encrypted-DNS success of at least
99% with zero physical port-53 or plaintext-fallback observations.

The protected job requires repository variable
`PERFORMANCE_ACCEPTED_BASELINE_COMMIT` to contain a full lowercase commit. It
runs both baseline and candidate seven times for every entry in
`tool/performance_scenarios.json`, then the repository evaluator creates the
bound reliability report. The evaluator independently receives the accepted
baseline through required `--baseline-commit`; every raw baseline/candidate
identity must match that value and the staged candidate. The evidence validator
also binds every raw report's identities to its comparison and release candidate,
not only to matching file digests.

H2 high-BDP has two mandatory scenario entries, not two interchangeable profiles:
`h2-high-bdp` uses `h2-bdp-100ms` (100 Mbps, 100 ms, single flow) and
`h2-high-bdp-four-flow` uses `h2-bdp-500ms` (500 Mbps, 50 ms, four flows), with
bidirectional workloads. Each requires its own seven-sample baseline and candidate
file. There are sixteen input files across eight scenarios but still seven stable
gate IDs. `performance.h2_high_bdp` passes only if both scenario comparisons pass;
a missing, failed, unstable, or not-run scenario cannot be represented by the other.

Version-3 evidence bundles contain `baseline_reports` and `candidate_reports`
arrays covering every required scenario exactly once, plus corresponding array
digests in the comparison. The validator recomputes the complete grouped gate.
Raw measurement reports remain schema v2; obsolete singular-report evidence
bundles cannot prove complete coverage under the new contract.

Missing baseline configuration, a malformed report,
six or eight samples, instability, a budget failure, or `not_run` makes that
supplemental job fail. As with the other protected runners, absence or failure
is not publication success but does not become a publication prerequisite.

The final public release includes the signed packages, per-package SPDX SBOMs,
`release-manifest.json`, and `SHA256SUMS`. Package checksums are calculated after
signing and against the same immutable candidate offered to any enabled
protected runners.
