# Reliability test environments

Usque separates deterministic pull-request evidence from destructive or
network-observer release evidence. Missing infrastructure is never converted
to a pass.

## Pull-request gate

The normal CI workflow runs locked Rust tests and Clippy, protobuf wire
fixtures, the diagnostic runner and fault-injection tests, Android JVM tests,
Flutter analysis, controller tests, responsive widget tests, and diagnostic
bundle privacy tests. These jobs do not require a public endpoint and do not
change host routes, DNS, firewall, proxy, or TUN state.

## Protected release runners

The public release workflow waits for four explicitly labelled, self-hosted
runners after the exact signed candidate has been staged:

| Runner label | Required isolation | Scope |
| --- | --- | --- |
| `usque-snapshot-vm` | Windows snapshot VM with an independent management channel | Clean install, upgrade, connected uninstall, Engine/Agent termination, sleep/network change, route/DNS/WFP/proxy restoration, Wintun residual checks |
| `usque-android-device` | Dedicated physical Android device controlled by ADB | Wi-Fi/cellular changes, airplane mode, Doze, lock/unlock, UI/VPN process reclamation, Always-on, Lockdown, reboot, upgrade, TV background/foreground |
| `usque-network-observer` | Controlled gateway outside the Engine process | H3/H2 endpoint behavior, separate IPv4/IPv6 assertions, DNS/Kill Switch/route/direct-rule packet observation |
| `usque-performance-lab` | Stable host, endpoint, topology, load and thermal policy | Seven raw samples for throughput, latency percentiles, CPU, memory, queue pressure, reconnection time and H3/H2 stage timing |

Each runner supplies a protected `usque-reliability-runner` executable. The
repository workflow passes it the exact candidate directory, commit and output
directory. The Windows command is additionally guarded by
`USQUE_ISOLATED_SNAPSHOT_VM=1`. Do not provision these runner labels on a
developer workstation.

## Report contract

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
and `not_run`. It emits the publishable `reliability-report.json` and
`device-matrix.md` only when all required gates pass. PCAPs stay in restricted
CI artifacts and are never copied into the public diagnostic bundle or GitHub
release.

The final public release includes the signed packages, per-package SPDX SBOMs,
`release-manifest.json`, `SHA256SUMS`, `reliability-report.json`, and
`device-matrix.md`. Package checksums are calculated after signing and against
the same immutable files used by the protected runners.
