# H3 path ownership and validation contract

The H3 actor owns one `PathSocketSet` and advertises build support through
`quic_migration=true`. A same-endpoint, same-outer-family network-generation
change first attempts validation and migration of the existing QUIC connection.
Unsupported or failed attempts use the existing complete reconnect path.
There is never a second data-bearing CONNECT-IP session or multipath fan-out.
The internal `PRODUCTION_NETWORK_FEATURES.quic_migration` build switch controls both capability
advertisement and netstack dispatch. Setting it false retains active socket
ownership while restoring complete reconnect, without a second socket path.

## Ownership and resource bounds

- The set has one slot each for active, candidate, and retiring paths: at most
  three sockets and three receive tasks, with only one active role.
- A path keeps its exact-egress lease until its socket is closed. Receiver
  tasks retain the socket and lease together; explicit shutdown cancels,
  aborts/joins, drains the channel, closes the socket, and releases the lease.
- Candidate supersession releases the old task, channel, socket, and lease
  before preparing the replacement.
- Each receiver channel holds at most four batches. All paths must share one
  192-buffer receive budget, including buffers in flight and queued batches.
  A separately allocated pool is rejected when inserting a path. Exhaustion
  waits cancellably without allocating more storage or clearing socket
  readiness. The portable truncation sentinel makes the actual storage budget
  `192 * 2049` bytes.

`SendInfo.from` must match a socket's local address exactly and `SendInfo.to`
must match that path's peer. Missing or mismatched routing is an error, never an
implicit send on active. Receive events carry their path identifier and local
destination into the same quiche connection. Decrypted application DATAGRAMs
remain connection-scoped; their origin is not guessed from the receive socket.

## Exact-generation setup

Initial and candidate sockets use the target-aware
`protect_for_target_generation` contract. The factory checks generation before
creation, after platform protection, and immediately before returning. A stale
result closes the unexposed socket and releases its lease; initial setup retries
at most twice before returning `UnderlyingNetworkChanged` to the scheduler.

Android retains only the current and adjacent previous generation entries,
including an explicit absent-network entry. Out-of-order records cannot bring
back an expired generation. JNI requests the exact generation, and Kotlin
checks its authoritative generation around `VpnService.protect` and
`Network.bindSocket`; it never substitutes the latest network. The descriptor
duplicate used for binding is always closed. Service destruction rejects new
binding and clears retained network entries. JNI reports stale generation
separately from protection/binding rejection, even if the Rust notification is
still in transit.

Windows now monitors physical generation in every VPN mode, independently of
Geo routing or physical DNS availability. The Agent reads a bounded current
route/interface snapshot, excluding the TUN and its owned stale endpoint
bypass routes, without modifying routes or DNS. It uses the selected interface
with the documented [GetBestRoute2 source selection](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-getbestroute2).
The Engine binds only its socket's outgoing interface with
[IP_UNICAST_IF](https://learn.microsoft.com/en-us/windows/win32/winsock/ipproto-ip-socket-options)
or [IPV6_UNICAST_IF](https://learn.microsoft.com/en-us/windows/win32/winsock/ipproto-ipv6-socket-options)
and retains the existing exact-target transactional permit. No global bypass
is added. A fresh Agent snapshot after binding must still match the expected
generation and selected interface; otherwise both socket and lease are
discarded. Route/source fingerprints remain Agent-local.

Dynamic permit keys include generation. Delayed old-lease release and delayed
snapshot invalidation cannot remove a permit for a newer generation. The
physical monitor owns only a weak reference and is canceled with its
protector. HTTP/2 full reconnect also obtains a target-aware generation lease
and retains it through TLS and driver teardown; the stream closes before the
authorization is released. Desktop proxy mode without authoritative physical
generation does not claim current migration readiness.

When a VPN-created runtime is detached into proxy mode, its protection policy
changes only after the Agent acknowledges a Clean transaction with no active
packet session. The old physical monitor is canceled and stale VPN-generation
setup is rejected. Local proxy listeners remain open; one internal reconnect
may release the old transport's transaction-bound sockets. Subsequent proxy
connections use ordinary host networking and do not query the released Agent
operation. A failed or incomplete rollback never authorizes this transition.

## Migration transmit barrier

`MigrationTxBarrier` pauses new application DATAGRAM injection only during a
validation send cycle. Active output must reach quiche `Done` within 50 ms and
64 generated packets before candidate probing is allowed. An incomplete drain
releases the barrier without purging or dropping application data. Candidate
validation then uses its exact path, and normal active injection resumes when
the cycle ends. The same bounded drain runs before promotion so no pending
old-path wire output can be sent after that socket becomes retiring. Normal
wire generation explicitly selects active; only the barrier drive selects
candidate, and the socket router rejects all retiring sends.

The locked quiche 0.29.3 contract test creates a real pinned-TLS client/server
pair entirely in memory, exchanges spare connection IDs, queues an encoded
CONNECT-IP HTTP DATAGRAM, drains active, probes candidate, and delivers each
wire packet to the peer. The application DATAGRAM arrives during active drain;
no application DATAGRAM arrives during the candidate cycle before promotion.
This test remains a prerequisite for migration activation. A full H3 loopback
test also establishes CONNECT-IP, echoes application packets before and after
migration, verifies one CONNECT request and an unchanged connection instance,
and confirms all socket leases are released on actor close.

## Migration orchestration

The command channel has capacity one. Each request carries a three-second
deadline covering command delivery, exact-generation preparation and path
validation. Only a newer generation can supersede an attempt; older/equal
requests return `StaleRequest`. Preparation is one actor-polled future, not an
unbounded task queue. Dropped callers, timeout and supersession cancel that
future and close the candidate before replying or starting another attempt.

Netstack keeps pumping the old active path while the migration reply is
pending. Its 100 ms network-generation interval survives busy packet polling.
H2 and a withdrawn outer family immediately use complete reconnect. Missing
platform family metadata is resolved by exact protected socket preparation
and QUIC validation; it never permits an unprotected send.

Only an explicit validated event plus `is_path_validated` and a fresh
generation/connection/lease check permit promotion. In one non-awaiting actor
section, `migrate_source`, the full socket/lease/generation role swap, and PMTU
invalidation occur together. PMTU is revalidated once. An unexpected mismatch
between quiche's active path and the socket binding fails the connection rather
than performing a partial reverse migration.

The old socket remains receive-only for at most two seconds, or until a path
closed event/new generation removes it. A packet sent before promotion may
arrive afterward; tests distinguish delayed arrival from a new retiring-path
send. The latter is always forbidden.

## Connection IDs and failure behavior

Both the configured active-CID limit and the local target are four (one active
plus at most three spare SCIDs). SCIDs are 20 CSPRNG bytes and reset tokens are
16 CSPRNG bytes. Retired SCIDs are drained and replenished within the negotiated
limit. `IdLimit`/`OutOfIdentifiers` produce the stable local-CID-unavailable
reason; missing peer spares produce peer-CID-unavailable. Neither is a
connection failure by itself. If a migration needs an unavailable CID, its
bounded request fails and netstack reconnects. quiche's own path table is
bounded by the negotiated limit; it is not expanded to evade CID exhaustion.

No CID, reset token, descriptor, or endpoint address is added to logs, IPC, or
diagnostic exports. Routing failures use fixed text; quality uses allowlisted
reason enums only.

## Validation boundary

Android refreshes its cached generation from the authoritative Java counter
while holding the runtime-install lock and before starting a worker. A stale
bind also refreshes that cache without authorizing the rejected socket; the
next bounded attempt must still bind the exact current generation. Notifications
and refreshes publish monotonically, so a delayed callback cannot restore an
older generation. This closes the pre-install lost-notification window without
weakening protection or creating a device session for validation.

Workstation unit tests cover capacity, active uniqueness, candidate
supersession, socket-before-lease teardown, exact routing, shared-buffer
backpressure, generation races, CID replenishment, and the wire-level barrier
contract. Kotlin host tests cover G0/G1/G2 retention, absent networks,
out-of-order notifications, and history clearing. Pure Windows route-selector
tests cover TUN/owned-bypass exclusion, disconnected links, preexisting route
preservation and hard route/interface bounds; lease tests cover late release,
late invalidation, generation rejection and monitor cancellation. None of
these tests calls native route/WFP mutation APIs on the workstation.

Actual Android bind/protect instrumentation, device/service lifecycle,
external leak observation, and controlled performance measurements require
the protected environments from `AGENTS.md`. Without those environments they
are `not_run`, not passes, and the infrastructure tests do not replace them.
