# Network Doctor

Use Network Doctor to inspect connection settings and recent network readings,
then export a local report if you need help investigating a problem.

## Run a check

1. Open **Settings → Network quality** (also available from Home when supported).
2. Select **Run Network Doctor**. This starts Standard checks and opens Diagnostics.
3. Read each result and its suggested action. For an additional check, wait for
   the current run to finish, select **Deep**, then **Start diagnostics**.
4. Review the external requests described in the confirmation dialog before
   selecting **Run deep checks**. You can cancel a running check.
5. Use **Export diagnostic bundle** to review the included information and save a
   report to a location you choose. Nothing is uploaded automatically.

In Simplified Chinese, these controls are **网络质量 → 运行网络诊断**, **标准**,
**深度**, and **导出诊断包**. If Network quality is unavailable, the connected
Engine may not advertise the required capability; existing connection controls
remain usable.

## Choose Standard or Deep

| Mode | What it does | Network requests |
| --- | --- | --- |
| Standard | Reads saved settings and current local snapshots, including latency, loss availability, queues and DNS state. It targets a two-second run. | None. It does not change settings or platform network state. |
| Deep | Checks configured encrypted-DNS reachability and, when safely disconnected, the configured QUIC endpoint. The whole session is limited to 15 seconds. | May send the fixed test query `example.invalid` to your configured resolver and make an authenticated QUIC handshake. It requires confirmation. |

The QUIC probe is skipped while a connection is active, starting or stopping,
or when a safe disconnected state cannot be established. It does not create
a second traffic-carrying tunnel. Deep checks do not replace a real application
connection test.

## Read the result

| Result | Meaning and next step |
| --- | --- |
| Passed | The check's observed condition passed. Other checks and unobserved network paths may still be unknown. |
| Warning | Review the measured value and suggested action. A timeout or high latency alone does not prove the normal connection cannot work. |
| Failed | Follow the check's explanation, correct the relevant configuration if needed, and rerun it. |
| Skipped / unavailable | Required data, protocol support, credentials or safe probe conditions were missing. This is not a zero reading or a pass. |
| Cancelled / cancelling | The run was cancelled, or is still releasing its temporary resources. Wait for completion before starting another run. |

For example, HTTP/2 packet loss and PMTU are unavailable, rather than zero.
A local pass cannot establish that no DNS or Kill Switch traffic leaked on the
physical network. Keep credentials and raw diagnostic bundles out of public
Issues; follow [Security policy](../SECURITY.md) for suspected vulnerabilities.

## Implementation reference

The sections below describe checks, resource limits and platform integration for
maintainers. They use the existing diagnostic session, progress, cancellation
and local-export framework.

### Check catalog

| Stable check ID | Mode | Observation |
| --- | --- | --- |
| `quality.rtt` | Standard | Available smoothed RTT; warning at 150 ms |
| `quality.packet_loss` | Standard | H3 interval loss; warning at 2% (200 basis points); H2 N/A |
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

### Bounds and lifecycle

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

### Workstation evidence and protected scope

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
Runner isolation and publication policy are defined in
[Contributing](../CONTRIBUTING.md#development-machines).
