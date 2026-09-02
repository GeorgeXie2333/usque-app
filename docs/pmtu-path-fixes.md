# PMTU review: confirmed issues and fixes

The factual review used candidate
`423d99c7d82b5a3e19780e6d4563578635f7c538` and the exact published
`quiche` 0.29.3 dependency. The table distinguishes observed bugs from design
differences or scenarios that were not reachable in the current product.

## Issue disposition

| Review topic | Verified result | Change |
| --- | --- | --- |
| PMTUD lost on runtime-created paths | Confirmed. New client/server paths had no PMTUD state; promotion followed by `revalidate_pmtu()` could not initialize it. A memory-only 1350-byte old path to 1360-byte new path admitted a 1455-byte ordinary packet after promotion. | Give each runtime path fresh PMTUD state, retaining the effective settings and bounded probe ceiling. Validation precedes PMTU probing; promotion no longer restores an unverified ordinary-send ceiling. |
| Probe allowance used as a DATAGRAM admission bound | Confirmed. Before the first probe ACK, a 1282-byte queued DATAGRAM blocked a 64-byte DATAGRAM, although ordinary QUIC packets were capped at 1200 bytes. Revalidation could produce the same mismatch for already queued data. | Advertise the current ordinary-send PMTU. Reject new oversized input with `BufferTooShort` so CONNECT-IP returns the original inner packet for PTB handling. Existing queued entries that no longer fit are discarded through quiche's existing path, allowing smaller entries to progress. |
| Wrong path selected for PMTU probe state | The dependency used active-path state while sending on a possibly different path. Prior evidence did not establish reachability through Usque's migration barrier. Adding PMTUD to candidates makes path isolation necessary. | Select `send_pid` and require validation. A regression verifies a candidate PATH_RESPONSE cannot consume the active path's pending probe. No claim of a previously demonstrated product leak is made. |
| All initial ordinary packets use the larger ceiling | Not confirmed; the dependency already caps ordinary packetization at its current PMTU (initially 1200). The larger output allowance is needed for probes. | Keep that cap and a probe-sized output buffer. Repair DATAGRAM admission, not the probe buffer. |
| Doctor differs from the production connection race | Documented behavior: a serial, single-socket, isolated handshake probe, not the production race. Timeout is a warning, not proof that production cannot connect. | No product behavior change. |
| CUBIC quantum falls into the alleged narrow buffer gap | Not reachable with the current immutable CUBIC configuration; its quantum is ten times the configured UDP ceiling. | No scheduler change. Add IPv4/IPv6-ceiling invariants before discovery, after discovery and after revalidation. |

Automatic PMTU discovery and QUIC migration remain enabled. Disabling PMTUD
still uses the existing fixed **1350-byte** UDP payload policy. The 1200-byte
QUIC discovery floor is not a new product rollback setting.

## Implementation boundary

The fix is a pinned local patch of the existing dependency, not a broad
upgrade. Behavioral changes are confined to its connection/path modules;
one upstream test-comment trailing space is trimmed for the whitespace gate.
Usque adds test harness support and regression tests. Provenance, license, exact
archive hash and removal criteria are in
[`USQUE-PATCH.md`](../third_party/quiche-0.29.3/USQUE-PATCH.md).

No new unsafe blocks, network APIs, privileged actions, packet logging,
telemetry fields, background tasks or unbounded queues are introduced.
Oversized inner packets are not truncated. Newly rejected input uses the
existing CONNECT-IP/PTB result path; previously accepted unreliable DATAGRAMs
use the existing discard/drop behavior if revalidation lowers the bound.
Dropping such entries releases their existing owned buffers. Existing path
validation, anti-amplification, migration-barrier and cleanup rules remain.

## Regression evidence

The tests exchange actual QUIC wire buffers entirely in memory, with ephemeral
mutually pinned TLS identities and the production buffer factory. Software
loss filters simulate payload limits; they are not adapter/device evidence.

On pristine 0.29.3, the initial eight-test set produced six expected assertion
failures and two passes (disabled PMTUD and CUBIC invariants). With the patch,
all eleven tests pass, including three additional boundary checks:

- Initial unacknowledged probe: reject the oversized DATAGRAM; deliver the small one.
- CONNECT-IP batch: preserve oversized inner input for PTB; deliver the small packet and reconcile queue accounting.
- Revalidation: remove an old oversized queue head; continue small-packet delivery.
- Client-created and server-observed paths: independent state, 1200-byte validation traffic.
- Candidate response: leave the active path's pending PMTU probe intact.
- Promotion: rediscover a 1360-byte path after a 1350-byte path without admitting unverified larger data.
- PMTUD disabled: keep the fixed 1350-byte policy across migration.
- New path: respect a peer-advertised 1300-byte UDP limit.
- Handshake override: inherit enable and disable decisions, not stale Config defaults.
- Probe budget override: use two attempts on the migrated server path, not the Config value of seven.
- CUBIC: preserve ten-ceiling quantum for both 1472- and 1452-byte family ceilings.

Ordinary validation on 2026-09-03:

| Gate | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| Windows helper, `-Variant x64-v2 -CargoAction clippy` | Locked workspace/all-targets check passed |
| Windows helper, `-Variant x64-v2 -CargoAction test` | Locked workspace/all-targets tests passed; transport 297 passed |
| Windows helper, `-Variant x64-v2` | Locked Release compile passed; no program launched |
| Debian WSL, Rust 1.97.1, locked workspace/all-targets Clippy/tests | Passed; transport 300 passed, including the Linux loopback/address-layout branches |
| Pinned Android helper, `-CargoAction clippy -AbiFilter arm64-v8a` | Passed |
| Pinned Android helper, `-CargoAction build -AbiFilter all` | arm64-v8a, armeabi-v7a and x86_64 JNI compiled; no APK built |
| `py -3 tool/check_repository_policy.py` and Git whitespace check | Passed |

Root workspace tests do not execute quiche's standalone upstream test suite.
Unchanged Flutter, Kotlin and packaging/signing procedures are outside this
patch's check scope. Linux retains five warnings from generated BoringSSL
bindings; Windows reports informational import-library linker output for the
vendored quiche dynamic library. No generated source or lint policy was
changed to suppress these messages.

Snapshot VM, dedicated Android device, network observer and performance-lab
checks are **not_run**. No installation, live VPN, physical PMTU measurement,
dist packaging, signing, upload or publication is part of this repair.
Protected-runner evidence remains supplementary and non-blocking; missing
evidence must not be represented as a pass.
