# Network Doctor

Doctor extends the existing diagnostic session, progress, cancellation and
local-export framework. The Quality page runs Standard immediately. Deep is a
separate explicit confirmation describing the external requests. No report is
uploaded automatically. Local observations are not external proof of zero DNS
or Kill Switch leaks; that evidence belongs to the protected network observer.

## Added catalog

| Stable check ID | Mode | Observation |
| --- | --- | --- |
| `quality.rtt` | Standard | Available smoothed RTT; warning at 150 ms |
| `quality.packet_loss` | Standard | H3 interval loss; warning at 200 basis points; H2 N/A |
| `quality.queue_pressure` | Standard | Item/byte utilization, warning at 50% or recorded drops; GUI marks 80% as severe |
| `quality.pmtu` | Standard | Current outer payload limit and degraded phase; H2 N/A |
| `transport.migration_capability` | Standard | H3 same-family/CID capability or complete-reconnect fallback |
| `dns.direct_encrypted_configuration` | Standard | Validated explicit custom settings; no plaintext fallback |
| `dns.direct_encrypted_runtime_state` | Standard | Ready/degraded state, not an external packet observation |
| `transport.h3_path_validation_probe` | Deep | Authenticated, disconnected-only QUIC handshake; no HTTP/3 stream |
| `dns.direct_encrypted_reachability` | Deep | Fixed reserved `example.invalid` lookup via the configured encrypted resolver |

Unknown, unsupported, disconnected or stale measurements cannot pass as zero.
Findings use existing pass/warning/failure/skipped/cancelled statuses, fixed
summary/remediation codes and allowlisted numeric evidence. Exports omit
resolver names, bootstrap/endpoint addresses, QNAMEs, SSIDs, CIDs and raw errors.
The reserved probe name is constant program behavior, never a user's query.

## Bounds and lifecycle

Deep H3 checks use the same configured family ordering as normal connections:
Auto and Prefer IPv6 try IPv6 then IPv4; Prefer IPv4 reverses that order; forced
single-family policies never try the other family. Only configured endpoints
with an available (or unknown) family are considered. Checks remain serial with
at most one live socket, share one 3.8-second deadline, and reserve time for an
allowed alternate. A cancelled or changed-network check never starts fallback.

Standard targets two seconds, reads current snapshots and local configuration,
and creates no external connection. It does not reconcile connection state,
change generation, or apply route, DNS, proxy, firewall or profile mutations.
Windows actual platform-state inspection is not initiated by Standard; missing
independent OS observations remain unknown, not false passes.

Deep has a 15-second session budget and four-second per-check ceilings. Socket
I/O gets 3.8 seconds, leaving cleanup time. DNS and QUIC share one probe resource
group, not unlimited parallel tasks. Every new socket has a generation-tagged
lease contract: active Windows VPN uses Agent/WFP; Android binds the exact
underlying Network (and protects in VPN mode). Windows proxy and disconnected
desktop probes use ordinary host networking, with a logical generation-zero
lease, and cannot prove Agent/WFP egress. Generation changes, cancellation and deadlines
release sockets, TLS/QUIC state, bounded pools and tasks. DNS uses a dedicated
short-lived pool and waits for actual socket-permit release; it never clears or
modifies the live business resolver pool. Its strict trust roots, TLS name
verification, explicit bootstrap, and no-port-53-fallback policy are unchanged.

H3 is skipped while a connection is active, starting, stopping, or cannot be
excluded safely. Windows holds the disconnected lifecycle guard for the Deep
session. Android serializes the probe with connection work, cancels it before
starting a connection, and checks again in Rust that no runtime exists. The
probe constructs only quiche handshake state: no HTTP/3 object, CONNECT-IP
stream, business path, candidate promotion, or TUN exists. Missing saved identity
or safe platform state produces skipped, not a simulated pass. Desktop reads
only the TLS key, endpoint pin and assigned-address records, not account tokens
or license material. Android's saved-identity read is cleanup-free and temporary
secret arrays are zeroized on both sides of JNI.

Android uses one bounded Doctor worker separate from account operations, one
pending Binder probe, and one exact request-ID native cancellation slot. A
Standard snapshot round trip is limited to 750 ms. Cancellation remains
`cancelling` until the diagnostic worker unwinds. Old JNI methods/capabilities
remain optional; unsupported probes are skipped without insecure alternatives.

## Workstation evidence and protected scope

Android timeline reads use append-only Binder message 14 and an optional JNI
method. Rust mirrors the bounded native transport timeline in memory at 1 Hz
and on shutdown; the getter returns at most 256 events and 192 KiB. The UI
allows one outstanding request for at most 750 ms. Missing/old methods fall
back to the existing phase timeline; native events and real RTT/fallback/queue
counters take precedence when present. Late replies and UI destruction cannot
complete a request twice. The full timeline is never added to regular events.
Exports use the same enum/numeric allowlist and omit live absolute timestamps;
the requested diagnostic session is frozen before the asynchronous read.

Ordinary tests cover read-only configuration/state equality, the 15-second
session budget, dependency ordering, resource-group serialization, cancellation,
authenticated loopback QUIC with zero HTTP streams, DoH/DoT loopback cleanup,
generation changes, malformed wire values, and export allowlists. Flutter
tests cover a bounded 60-second chart with text alternatives, missing-capability
navigation, English/Simplified Chinese, light/dark, 200% scaling, keyboard and
TV navigation, confirmation, errors and retry. Golden files use synthetic test
measurements and are not performance or leak evidence.

Windows VPN lifecycle, Android device/Doze, real adapter migration, externally
observed candidate/old-path payloads, physical port-53 counts and repeated lab
performance remain protected-runner tests. If unavailable, record `not_run`;
never treat an unavailable lab or a local Doctor pass as protected evidence.
Protected results remain supplemental and do not gate publication.
