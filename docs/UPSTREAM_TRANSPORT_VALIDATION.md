# Upstream transport fixes: local validation

Date: 2026-10-18 (Asia/Taipei).
Baseline: `02685ee3` plus the changes described here, tested before committing.
This is a working-tree validation record, not evidence for a published release.
Final check results apply only to this tested working tree.

## Batch 1: independent bounded ICMP delivery

The new real-pump regression first failed on the original implementation:
`queued ICMP must not prevent draining the full outgoing pipe`. It combines
a one-slot PacketDevice, blocked incoming delivery and an oversized send result;
no OS socket or TUN is created. The repair retains up to 32 synthetic ICMP
packets, including the pending delivery, independently of normal inbound data.
It preserves outgoing progress and drops only excess generated feedback.

Further regressions cover overflow, FIFO delivery, cancellation, receiver
closure, IPv6 minimum-MTU failure with a full feedback queue, and mux ownership
and quoted source-port restoration when TUN and proxy flows share a local port.

## Batch 2: local TCP packet budget

A smoltcp regression first recorded the missing segmentation behavior: after
requesting a smaller budget, retransmission still carried 1460 payload bytes
and failed `retransmit still exceeds the MASQUE packet budget`. The small
control API had been introduced for the test, but none of the three original
MSS calculations used it yet. This distinguishes the red test from an already
working production integration.

The pinned TCP patch now applies a local IP-packet bound to send readiness,
ordinary/fast/RTO retransmission and new SYN MSS. The peer MSS and interface MTU
remain independent. Only an opted-in MASQUE private stack receives revisioned
commands. Current-session H3 capacity comes directly from quiche and actual
CONNECT-IP overhead, through a latest-value channel; old actor subscriptions
are dropped on replacement. Commands reject stale revisions.

The memory link test deliberately drops oversized IPv4/IPv6 TCP packets and
all ICMP. Existing connections recover after budget reduction, retain byte
order, accept new connections under the reduced budget, ignore old commands,
restore the interface bound for H2, and close with FIN. Separate regressions
cover real QUIC PMTU revalidation/promotion, bounded command coalescing and
cancellation, and exclusion of external-chain and L4 monitors.

## Safety and scope

No privileged networking, installer, credentials, TLS trust, saved profile,
protobuf fields or public queue identifiers change. Feedback logs contain only
aggregate counts. Queue and command ownership is bounded and released on
shutdown. An inability to apply a local TCP limit cancels the private service
token; its tasks remain owned and are aborted/joined during runtime shutdown.
The actor retains its final packet-length guard; IPv6 probing waits and
confirmed minimum-MTU failure retain their prior behavior. Neither arbitrary
ICMP nor sampled diagnostics can control the TCP budget.

The frozen Go oracle and patched quiche sources are unchanged. The new
[smoltcp source record](../third_party/smoltcp-0.14.0/USQUE-PATCH.md) identifies
the exact archive and narrow patch. The dependency wrapper's standalone
manifest also selects that patch. UDP/raw sizing, DIRECT, L4 and the chain's
final inner stack do not inherit the outer H3 limit.

## Check commands and results

The Windows helper initializes the supported MSVC/Ninja/libclang environment.
`RUST_TEST_THREADS=4` bounds test concurrency. Test runs that need UDP/TCP
loopback use a process with local networking access; an initial sandbox run
timed out in unrelated loopback tests and was interrupted, not counted as a pass.
The Python commands use the verified bundled Python 3.12.14 executable.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | passed, warnings denied |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | passed: 1834 tests across 24 test binaries; 13 existing optional/isolated tests ignored |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | passed: release compilation only |
| `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed; pinned NDK/CMake, no JNI artifacts copied |
| `cargo test --manifest-path third_party/smoltcp-0.14.0/Cargo.toml --locked --lib` | passed: 676 tests |
| `cargo test --manifest-path third_party/ts_netstack_smoltcp_core/Cargo.toml --locked --all-targets` | passed: 14 unit tests and 1 UDP memory smoke test |
| `python tool/check_repository_policy.py` | passed |
| `git diff --check` | passed |
| `go mod verify` from `oracle/go` | passed: all modules verified |
| `go test ./...` from `oracle/go` | passed compilation; archived packages contain no Go test files |
| `python tool/verify_oracle_archive.py` | passed: 41 files match the frozen upstream commit |
| `go install github.com/rhysd/actionlint/cmd/actionlint@914e7df21a07ef503a81201c76d2b11c789d3fca` followed by `actionlint -no-color` | passed; actionlint 1.7.12 |

The changed vendored Rust files also passed the following scoped format check;
other upstream sources were not reformatted:

```powershell
rustfmt --edition 2024 --config skip_children=true --check `
  third_party/smoltcp-0.14.0/src/socket/tcp.rs `
  third_party/ts_netstack_smoltcp_core/src/lib.rs `
  third_party/ts_netstack_smoltcp_core/src/command/error.rs `
  third_party/ts_netstack_smoltcp_core/src/command/stack_control.rs `
  third_party/ts_netstack_smoltcp_core/src/stack_control_impl.rs `
  third_party/ts_netstack_smoltcp_core/src/socket_impl/tcp/stream.rs `
  third_party/ts_netstack_smoltcp_core/tests/udp.rs
```

Archive comparison verified that only `src/socket/tcp.rs` and the standalone
`Cargo.lock` differ from the hash-verified smoltcp archive; its patch record is
an additional file. Root and wrapper lockfiles retain the same dependency
versions and change only smoltcp's source selection.

## Pre-commit review and dependency audit

A second source review checked bounded feedback ownership, receiver closure,
session replacement, TCP options and retransmission, new outbound sockets,
budget revision ordering, and cleanup of the budget consumer. No additional
blocking defect was found in either fix. The complete checks above remain
applicable: the review did not change production or test code.

The review also completed the CI dependency audit omitted from the first
validation record. The official Windows x64 OSV-Scanner 2.3.8 executable was
verified against SHA-256
`cb04e79dd9698a7bc821bbfdddec916a416d1409fda79c927c509d37d00c9716`
from the [upstream release](https://github.com/google/osv-scanner/releases/tag/v2.3.8).
Only the scanner platform differs from CI. Commands, from the repository root:

```powershell
python tool/export_gradle_runtime_sbom.py `
  apps/usque_gui/android/app/gradle.lockfile `
  target/upstream-validation/android-release-runtime.cdx.json
& ./target/upstream-validation/osv-scanner.exe scan --format json --all-packages `
  --output-file target/upstream-validation/osv-shipping.json `
  --config osv-scanner.toml -L Cargo.lock -L apps/usque_gui/pubspec.lock `
  -L target/upstream-validation/android-release-runtime.cdx.json
& ./target/upstream-validation/osv-scanner.exe scan --format json --all-packages `
  --output-file target/upstream-validation/osv-oracle.json `
  --config osv-scanner.toml -L oracle/go/go.mod
python tool/check_osv_results.py `
  --shipping target/upstream-validation/osv-shipping.json `
  --oracle target/upstream-validation/osv-oracle.json --config osv-scanner.toml
```

Both scanners completed with exit code 1 (findings), and the repository's
severity/exception policy completed with exit code 0. The production scan
covered 375 Rust, 65 Dart and 96 Android runtime package entries, including
the local smoltcp 0.14.0 patch. No known vulnerability was reported for smoltcp.
The unchanged baseline dependency rustls 0.23.45 has four findings:
`RUSTSEC-2026-0346` (medium, 4.0), `RUSTSEC-2026-0347` (low, 2.3), and
`RUSTSEC-2026-0348` / `RUSTSEC-2026-0349` (low, 3.7 each). These do not cross
the existing blocking threshold; no exception or suppression was added.
The frozen, non-shipping Go oracle has informational findings and was not
modified. This audit is not a claim that every dependency is vulnerability-free.

## Not run

Windows snapshot-VM TUN/WFP/route lifecycle, Android device lifecycle, external
leak/fragmentation observation, physical path migration and controlled
performance-lab measurements are `not_run`: the required isolated environments
were not used. Memory and loopback regressions do not establish these outcomes.
No MSI or release APK was produced or installed.
