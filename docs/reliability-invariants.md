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
pass. Protected-runner evidence is supplemental and does not gate publication;
its absence or `failed`/`not_run` state must remain explicit. Any evidence used
for a candidate must still match its exact identity and isolated environment,
and forged or mismatched evidence is rejected.

## Network-quality and migration invariants (append-only)

| Identifier | Required property | Evidence |
| --- | --- | --- |
| `INV-MIGRATION-CANDIDATE-NO-APP-SEND` | Before validation and promotion, candidate may send only QUIC validation/maintenance/control, never client application DATAGRAM, CONNECT-IP, proxy, or DNS payload. Receive remains connection-scoped. | Locked-quiche barrier and full H3 loopback tests; protected external observer when available. |
| `INV-MIGRATION-VALIDATED-BEFORE-PROMOTION` | Only an explicitly validated candidate on the current generation can become active. | Actor validation, timeout, generation-race and connection-close tests. |
| `INV-MIGRATION-ATOMIC-PROMOTION` | Path ID, socket, lease, generation and PMTU state change in one non-awaiting H3 actor section. | Complete-binding promotion and PMTU-once tests. |
| `INV-PMTU-FAILS-SAFE` | Unknown/decreased PMTU remains conservative; no silent IP truncation, EMSGSIZE spin, or ICMP PTB bypass. | PMTU state-machine, send-error circuit-breaker and packet-size tests. |
| `INV-DIRECT-DNS-NO-PLAINTEXT-FALLBACK` | DoH/DoT use only their selected encrypted protocol and explicit bootstrap addresses; any failure returns failure/SERVFAIL without system DNS fallback. | Encrypted direct-DNS transport and protected observer evidence are required for that implementation. |
| `INV-METRICS-LOCAL-AND-SANITIZED` | Quality metrics remain local and contain no QNAME, DNS server, bootstrap IP, SSID/BSSID, endpoint, CID/token or payload. | Typed snapshots, codec and adversarial export tests. |

`INV-OLD-PATH-QUIESCED` keeps its original meaning for replacement QUIC
connections: the old connection is quiesced before replacement. Migration
inside one existing connection follows the three migration invariants above.
After promotion the old socket may receive delayed QUIC packets during its
bounded grace period, but cannot send application or control packets.
