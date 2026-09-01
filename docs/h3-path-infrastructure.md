# H3 path ownership and validation contract

The H3 actor owns one `PathSocketSet`. This infrastructure change still creates
only the active socket and advertises `quic_migration=false`; network-change
orchestration is a separate change. Allowing migration in QUIC transport
parameters does not itself promote a path.

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

Initial and future candidate sockets use the target-aware
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
still in transit. Windows authorization remains owned by its existing
transactional lease implementation; transport makes no platform mutations.

## Migration transmit barrier

`MigrationTxBarrier` pauses new application DATAGRAM injection only during a
validation send cycle. Active output must reach quiche `Done` within 50 ms and
64 generated packets before candidate probing is allowed. An incomplete drain
releases the barrier without purging or dropping application data. Candidate
validation then uses its exact path, and normal active injection resumes when
the cycle ends. Production activation is intentionally deferred to migration
orchestration.

The locked quiche 0.29.3 contract test creates a real pinned-TLS client/server
pair entirely in memory, exchanges spare connection IDs, queues an encoded
CONNECT-IP HTTP DATAGRAM, drains active, probes candidate, and delivers each
wire packet to the peer. The application DATAGRAM arrives during active drain;
no application DATAGRAM arrives during the candidate cycle before promotion.
This test must remain a prerequisite for any future migration activation.

## Connection IDs and failure behavior

Both the configured active-CID limit and the local target are four (one active
plus at most three spare SCIDs). SCIDs are 20 CSPRNG bytes and reset tokens are
16 CSPRNG bytes. Retired SCIDs are drained and replenished within the negotiated
limit. `IdLimit`/`OutOfIdentifiers` produce the stable local-CID-unavailable
reason; missing peer spares produce peer-CID-unavailable. Neither is a
connection failure by itself.

No CID, reset token, descriptor, or endpoint address is added to logs, IPC, or
diagnostic exports. Routing failures use fixed text; quality uses allowlisted
reason enums only.

## Validation boundary

Workstation unit tests cover capacity, active uniqueness, candidate
supersession, socket-before-lease teardown, exact routing, shared-buffer
backpressure, generation races, CID replenishment, and the wire-level barrier
contract. Kotlin host tests cover G0/G1/G2 retention, absent networks,
out-of-order notifications, and history clearing.

Actual Android bind/protect instrumentation, device/service lifecycle,
external leak observation, and controlled performance measurements require
the protected environments from `AGENTS.md`. Without those environments they
are `not_run`, not passes, and the infrastructure tests do not replace them.
