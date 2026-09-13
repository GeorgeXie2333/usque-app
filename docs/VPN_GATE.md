# WARP → VPN Gate

VPN Gate is an optional final exit for Windows, Android and Android TV. Open
**Proxy → VPN Gate**, refresh the directory, filter by country, select a TCP
server, and explicitly save or apply the selection. The switch is off for old
and new configurations. Selecting a row edits a draft; it does not reconnect.
Server selection is available only while the page's VPN Gate switch is on.
With the switch off, the directory remains browsable and existing selections
are retained.

The current connected server and the draft are separate. An enabled selection
is device-wide and shared across WARP accounts. A failed apply retains the
requested target and its error. A node disappearing from the directory does
not change its saved configuration or disconnect an existing session.

## Packet path

```mermaid
flowchart LR
    F["System TUN / SOCKS5 / HTTP"] --> R["Existing direct rules"]
    R -->|Proxy| O["OpenVPN memory TUN + final IP stack"]
    O --> W["Private WARP TCP dialer"]
    W --> G["Selected VPN Gate TCP remote"]
    G --> I["Internet"]
    R -->|Explicit direct exception| D["Existing direct network"]
```

OpenVPN is an additional exit setting. WARP CONNECT-IP/L4 and HTTP/3/HTTP/2
remain independent choices. The selected OpenVPN TCP connection always uses
the WARP internal dialer, without Geo matching. There is no second OS VPN
interface, OpenVPN process, physical OpenVPN socket or public bootstrap proxy.
Business UDP uses IP packets inside OpenVPN TCP; HTTP retains its existing
HTTP/CONNECT semantics. TCP encapsulation can increase latency under loss.

The final IP addresses, DNS and MTU come from OpenVPN. WARP assignments stay
inside the underlay. Unsupported proxy address families are refused in the
final stack and TUN path. Explicit Geo, CIDR, LAN, system-proxy bypass and
Android application exceptions retain their direct behavior. User-configured
local/direct DNS is preserved; remote DNS and exit diagnostics use the final
session and never substitute the node's advertised IP for a measured exit.
In remote DNS mode, the OS uses the existing internal DNS listener and its host
route. This keeps a VPN-pushed private resolver inside the final channel even
when LAN bypass is enabled; Geo direct DNS exceptions still follow their rules.

Traffic samples count packets entering and leaving the final VPN Gate channel.
RTT, loss, congestion and HTTP/QUIC diagnostics describe the underlying WARP
connection to Cloudflare; they do not measure the complete path through the
volunteer server. The network quality page labels this scope. Replacing either
the VPN Gate session or the active WARP transport starts a new sample history,
so counters from different sessions cannot form a traffic-rate interval.
WARP connection events remain available while VPN Gate is active.

A connected VPN Gate node providing only IPv4 shows connected when IPv4 is
available; unsupported proxied IPv6 stays blocked. Losing IPv6 from a dual-stack
assignment still shows limited connectivity, as does reduced family support
without VPN Gate.
Exit location and public addresses still depend on a successful final-channel
probe and may be unavailable during network fluctuations.
Across all WARP modes, with or without VPN Gate, each failed exit IP or location
lookup is retried once after one second. Successful lookups are retained; a
second failure leaves that information unavailable. Disconnecting or replacing
the probe cancels pending requests and retries for the old session.

## Directory and saved configuration

The mirror's [cumulative pool contract](https://github.com/GeorgeXie2333/vpngate-list-mirror/blob/main/docs/pool.md)
is validated in `usque-core::vpngate`. Discovery requests Raw
`main/pool/latest.json`, then races the five complete, validated CDN responses
at `cdn`, `fastly`, `gcore`, `testingcf` and `quantil.jsdelivr.net`, using
`@latest/pool/latest.json`. A fast invalid response cannot win.

The chosen index pins the complete data commit. Both `pool/servers.json` and
`pool/countries.json` must match their byte lengths, SHA-256 and schema, and
agree on countries, counts and source acquisition time before an atomic cache
switch. Country filters derive from that node snapshot. Older indexes cannot
replace a newer valid cache. A corrupt downloadable cache can be repaired by
a completely validated refresh without deleting favorites.

A Raw timeout disables Raw for the rest of that acquisition and its WARP
fallback. Lazy configuration requests inherit the selected snapshot's download
policy. A new directory refresh evaluates Raw again. All remaining requests
race the five CDN hosts at the fixed commit; only discovery uses `@latest`.
Other Raw errors enter CDN fallback without permanently disabling Raw. If a
primary acquisition fails, WARP retries once, retaining the already selected
index and completed files. Failures preserve the previous cache.

Connected refreshes and configuration downloads use the final session first
and reuse its WARP underlay for fallback. Offline fallback creates an
account-bound headless WARP session and releases it on completion or
cancellation. It creates no user listener, TUN or system proxy. Jobs cancel on
account/lifecycle changes. Each request has a five-second connection deadline
and a fifteen-second complete-response deadline. Redirects and system proxies
are disabled. Identity encoding is required. Limits are 64 KiB per index,
16 MiB per directory, 5,000 nodes, 256 KiB per configuration JSON and 128 KiB
per decoded OpenVPN configuration.

Configuration is fetched only for explicit save, favorite or favorite-update
operations. The configuration JSON and its decoded original Base64 content
have separate byte lengths and hashes. Both are checked before applying the
existing OpenVPN profile allowlist. A metadata TCP candidate is not a promise
of native compatibility or a successful VPN connection. Node jobs acknowledge
immediately and query in-memory progress without repeatedly parsing the pool;
neither IPC nor Kotlin sends
configuration bodies to Flutter. Saving settings only pins a prepared local
reference. The UI also checks account and connection intent before applying a
completed preparation, so a late result cannot undo Disconnect.

**Cumulative pool** and **Favorites** share the country filter and local flags.
The pool retains source score ordering; favorites use descending saved time.
Source scores, ping and speed are not local measurements. Workers' TCP
observations include their timestamps and do not establish OpenVPN, TLS or
application connectivity. Missing or untested nodes are not treated as proven
offline. The page marks observations older than twelve hours and source data
older than three or twenty-four hours. Local acquisition time and the mirror's
source acquisition time are displayed separately; neither is an upstream
update timestamp. Hourly foreground refreshes do not guarantee CDN freshness.

Favorites are device-local and shared across WARP accounts. Each stable node ID
has one saved configuration snapshot, stored beside application configuration
(Android uses `noBackupFilesDir`), separately from the downloadable pool cache.
Configurations are deduplicated by content hash. Refresh, node disappearance
and ordinary cache cleanup do not delete favorites or replace their bytes.
New configurations are explicitly updated with an expected-old-hash check.
Membership revision checks prevent a removed favorite from being resurrected
by an old update. Limits are 5,000 favorites and 64 MiB of referenced local
configuration objects; favorites are never automatically evicted.

Removing a favorite does not disconnect or change the saved selection. Current
and prepared selections retain independent references. Updating or removing a
favorite also retains any pending local draft until it is saved or discarded.
Unreferenced objects are collected after membership/settings changes; startup
clears abandoned preparation references. Legacy inline saved selections remain
readable and migrate on demand; they are not automatically added to favorites.
The master switch must be on to select a server, while favorite management
remains available with it off. Current, saved and pending selections retain
their own node metadata and configuration hashes.

## Native boundary and verification

`usque-openvpn` embeds OpenVPN 3 Core 3.11.7 at
`18edfae7e7fd8051c93bd4746ec69be91eb02dbb`, C++17 and Mbed TLS 3.6.7. Its C ABI
exchanges bounded packets and typed events with a dedicated core thread.
Transport generation checks reject old callbacks. Native input and output are
each limited to 256 entries / 4 MiB; Rust IP channels have independent byte
budgets. TCP lengths are two-byte network-order frames, read/written with
partial-I/O handling. Cancellation stops producers and joins the core.

Cancellation includes awaits inside event handlers and the final transport
failure notification. Stop is signalled to the native core before joining the
transport task, and it wakes backpressured input writers. Native shutdown waits
at most five seconds. A timeout is reported as a pending worker: the worker
retains its native allocation until it exits, while its caller-owned transport
and final traffic admission are closed. This does not claim the native thread
was forcibly terminated. The 35-second negotiation deadline is separate from
the bounded shutdown wait.

After the desktop service finishes platform cleanup or records its recovery
failure, Engine runtime shutdown waits at most another five seconds for pending
blocking workers. This avoids Tokio's default unlimited shutdown wait. The UI
process monitor uses cancellable 100-millisecond waits and closes its process
handle when another shutdown trigger wins. These limits do not bypass Agent
cleanup or turn an incomplete recovery journal into Clean.

The profile allowlist permits only numeric TCP remotes matching the directory
IP and preserves the advertised port. UDP profiles are not rewritten. Scripts,
plugins, external file references, nested proxies, compression and peer
verification bypasses are rejected. The minimum TLS version is 1.2. CBC/SHA1
refers to the data channel; it does not disable TLS certificate verification.
The native TUN builder captures assignments and never executes pushed OS routes.

Mbed TLS authenticates the OpenVPN CA chain and configured OpenVPN certificate
constraints, rather than assuming an HTTPS DNS SAN for a numeric VPN remote.
Compatibility patches explicitly select CBC PKCS#7 padding and enforce TLS
version limits. The complete patch rationale is in
[the upstream patch record](../third_party/openvpn3-3.11.7/USQUE-PATCH.md).

## Lifecycle, cleanup and privacy

Final traffic is admitted only after negotiation and platform attachment.
Switching closes old final flows, retains a usable WARP session, builds a new
OpenVPN session and applies its final assignment. No failure path intentionally
selects WARP or physical egress as the final fallback. The retry controller
uses the same saved node and existing bounded backoff; certificate,
authentication and configuration errors are terminal.

Windows Agent owns the sole Wintun, route, DNS and WFP journal. Chain protection
is installed before final negotiation, including when the persistent Kill
Switch setting is off. Finalization changes only address/DNS/MTU fields; it
cannot change bootstrap endpoints or direct exceptions. The transition guard
is retained through final setup failure and ordinary node switches, and is
restored through the disconnect journal. Persistent protection remains governed
by the existing Kill Switch setting and Agent recovery rules.

An explicit cancellation during Prepared startup drops the headless startup
future (closing its producer guards) before releasing the startup pipe. A full
disconnect closes final admission and packet producers before releasing either
startup or active leases, then performs asynchronous teardown and rollback.
Ordinary negotiation failures retain the guard for in-place retry. Finalization
also observes cancellation, so a stopped request cannot admit a late connection.
The Agent's existing EOF grace, operation/owner checks and lease epoch remain
authoritative; Prepared alone never authorizes recovery of a live transaction.

Engine JSONL logs carry a fresh random `engine_run_id` and a monotonic
`engine_event_sequence` for each logging run. These distinguish an older Engine
from a newly opened one without exporting process or device identifiers. Fixed
stage labels record waiting for connection ownership, packet/data-plane joins,
lease release, native stop and Agent mutation replies. Native task completion
and a still-pending native worker have separate records. Agent replies include
only the typed phase, journal generation, success flag and elapsed time; polling
does not create lifecycle logs. Correlate those generations and timestamps with
the existing `windows-recovery.json` observations and historical step results.
Successful Wintun restoration still requires both interface and PnP absence;
timeouts, failed probes and identity conflicts do not become absence evidence.

Android uses its existing VpnService and a blocking interface during setup.
A new final interface is established and attached before retiring the old Java
descriptor. A rejected handoff closes final admission and keeps the blocking
interface. Only WARP outer sockets and explicit direct traffic receive physical
socket protection. Always-on/Lockdown and process-death guarantees retain their
existing Android boundaries. In-process admission is not an independent OS
Kill Switch.

Directory hosts learn directory requests; the WARP provider carries the
OpenVPN connection; the selected volunteer provides final egress. Configurations
and public client key material stay in engine storage, not Flutter status,
free-form logs or diagnostics. Native logging and hardware identifiers are
suppressed. No new automatic telemetry or diagnostic upload is introduced.

## Sources, licenses and validation

OpenVPN Core uses MPL-2.0; Mbed TLS uses Apache-2.0; Asio uses BSL-1.0; LZ4
and xxHash use BSD-2-Clause. The selected sources and original license texts
are vendored. [The source lock](../tool/openvpn_sources.json) pins archives,
original file hashes and reviewed patch hashes. Run
`python tool/check_openvpn_sources.py`; it also verifies the license asset
available from the VPN Gate page. Release SPDX generation includes these native
packages, which Cargo/Gradle inventories alone do not discover.

Use the complete [change-scoped checks](../CONTRIBUTING.md). Native protocol
tests additionally run with `cargo test -p usque-openvpn --features interop-test
--locked` after initializing the supported Windows build environment. The
memory peer uses public test identities, TLS 1.2, CBC/SHA1, UDP-shaped IP packets,
rekeying and terminal authentication failures; it opens no OS sockets or TUN.
It is a Core-to-Core protocol test, not evidence of OpenVPN 2 server or public
volunteer availability. The feature is excluded from production builds.

The [local implementation validation record](VPN_GATE_VALIDATION.md) lists
completed commands, the blocked PowerShell check and isolated checks not run.

Real TUN, WFP, routes, leaks, crash recovery and Android device lifecycle must
be checked in the environments defined by [the safety contract](../AGENTS.md).
They are **not run on a development workstation**. The controlled exit matrix
must compare TUN/SOCKS5/HTTP traffic against the same selected VPN Gate session,
then independently exercise direct exceptions, unsupported IPv6, changed
assignments, WARP loss, Gate loss, node switching and explicit disconnect.
Missing isolated evidence is not a pass and is not a publication prerequisite.
