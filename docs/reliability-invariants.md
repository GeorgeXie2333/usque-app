# Reliability invariants

These identifiers are release contracts. Tests, diagnostic reports, and CI
jobs must reference the identifier directly so a failure can be traced to the
property that was violated. New invariants may be appended; an existing
identifier must not be reused for a different property.

| Identifier | Required property | Automated evidence |
| --- | --- | --- |
| `INV-SINGLE-ACTIVE-TUNNEL` | Exactly one MASQUE data-bearing path accepts packets. Candidate recovery paths cannot receive data before atomic promotion. | Transport supervisor path-promotion and fault-script tests. |
| `INV-KILLSWITCH-FAIL-CLOSED` | With Kill Switch enabled, only the protected endpoint and explicit direct-egress leases may use the physical network. | Windows Agent plan tests plus the external leak gate. |
| `INV-NO-PHYSICAL-DNS-FALLBACK` | Tunnel DNS never silently falls back to physical DNS. Physical DNS is used only by an explicit split/direct rule. | Split-DNS generation and leak-observer tests. |
| `INV-OLD-PATH-QUIESCED` | A physical network generation change cancels the old connection before a replacement path receives packets. | Deterministic generation-change fault test. |
| `INV-PLATFORM-STATE-RESTORED` | Engine/Agent/VPN exit and install lifecycle cleanup restore the captured platform lease, or leave an explicit fail-closed state. | Recovery-journal tests and isolated Windows/Android lifecycle gates. |
| `INV-DIAGNOSTICS-READ-ONLY` | Standard diagnostics do not mutate connection or platform state. Deep-diagnostic temporary resources are guarded and restored. | Diagnostic runner cancellation and before/after snapshot tests. |
| `INV-EXPORT-SANITIZED` | Export data is allowlisted and excludes secrets, profile names, endpoints, full addresses, hostnames, user paths, SSIDs, and package lists. | Adversarial diagnostic-bundle fixtures. |
| `INV-BOUNDED-WORK` | Every queue, loop, retry, probe, and diagnostic check has a capacity, timeout, and cancellation path. | Queue-capacity, timeout, cancellation, and task-drain tests. |
| `INV-PROTOCOL-APPEND-ONLY` | Existing protobuf field numbers and wire fixtures remain unchanged. | `usque-ipc` checked-in wire snapshot tests. |

## Release reporting

Pull-request jobs report deterministic in-process evidence. VM, device, and
independent network-observer gates report `not_run` when their required
environment is unavailable; they must never translate missing evidence into a
pass. A release candidate is blocked when any safety invariant fails or lacks
its required release-level evidence.
