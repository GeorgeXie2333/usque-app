# H3 client receive and recovery behavior

This reference describes CONNECT-IP HTTP/3 receive and recovery behavior.
One selected CONNECT-IP session carries application traffic; endpoint public-key
checks and packet/socket budgets still apply. Current algorithm choices are in
[Congestion control](congestion-control.md), and the shared socket receive-buffer
default is in [QUIC UDP receive buffer](UDP_RECEIVE_BUFFER.md). The configured
TUN MTU remains independent of outer-path PMTU discovery.

## Receive path

The actor drains HTTP DATAGRAMs after each received QUIC UDP packet instead of
waiting for an entire UDP batch. A UDP packet can contain several DATAGRAMs;
the old order could overflow quiche's 64-entry receive queue even with an
empty application channel. Partial application batches are retained until
full or the actor's next delivery step, so many tiny wire packets do not waste
the bounded channel slots. The total receive accounting remains 1024 inner
packets and the shared UDP receive pool remains 192 buffers across paths.

DATAGRAMs arriving alongside the first successful CONNECT response stay in
quiche until the actor processes that response. They are not discarded by a
premature not-ready drain. A retained application batch waits on channel
capacity in the main select loop; capacity return no longer depends on new
network traffic or the one-second quality sample. Peer events, timers,
migration, receiver closure, and shutdown remain independently pollable.

## PMTU and fragmentation

Initial and candidate sockets go through the same per-socket setup before
protection/binding completes and before any QUIC send:

- Windows: IPv4/IPv6 `MTU_DISCOVER=PMTUDISC_PROBE`, which forces nonfragmenting
  datagrams while allowing probes beyond the cached path estimate.
- Linux/Android: `IP_MTU_DISCOVER=IP_PMTUDISC_PROBE` for IPv4;
  `IPV6_MTU_DISCOVER=IPV6_PMTUDISC_PROBE` plus `IPV6_DONTFRAG=1` for IPv6.
  Dual-stack IPv6 sockets also receive the IPv4 policy for mapped addresses.
- Apple/FreeBSD: the applicable IPv4/IPv6 `DONTFRAG` option.

Failure to configure a supported socket is an error, not permission to send
fragmenting probes. Socket drop releases the failed setup; no route, interface
DNS, WFP filter, global proxy, or system-wide setting is changed.

The existing one-hertz actor observation also checks cumulative active-path
loss counters. A completed PMTU above 1200 bytes is revalidated after a window
of at least two seconds and three RTTs when all of these hold:

- At least three newly detected packet losses and at least one lost DATAGRAM.
- Loss is at least 25% of packets sent in the window, or at least two PTOs
  occurred in that window.
- Samples are continuous (no gap exceeding five seconds), counters have not
  reset, and the path is outside the 30-second loss-revalidation cooldown.

These are suspicion thresholds, not proof that the path MTU decreased. They
only restart quiche's existing discovery; probe acknowledgements determine
the usable size. There is no forced unverified MTU reduction and no bypass of
congestion control. Probe-only loss, sparse random loss, idle samples, an
unfinished search, disabled automatic PMTU, and a completed 1200-byte floor do
not trigger this mechanism. Promotion resets loss history. EMSGSIZE retains
its separate suppression and exhaustion budget. Confirmed insufficient IPv6
minimum MTU still follows the existing fail-closed termination policy.

## GOAWAY and automatic recovery

A GOAWAY whose ID still permits the current CONNECT request starts one
30-second grace period. The existing request can continue carrying packets;
no new CONNECT request is opened. Repeated GOAWAY cannot extend the deadline.
Rejection of the current request, stream FIN/reset, connection closure, or
deadline expiry ends the old session through normal teardown.

Under the saved Auto policy only, two short-lived established H3 failures
temporarily prefer H2 for 120 seconds. A PMTU revalidation exhaustion can do
so immediately. A 60-second stable H3 session resets the streak; stale failure
history and physical-generation changes also reset it. The existing H2-to-H3
recovery probe scheduling remains in use. The original saved profile is not
modified, and explicit H3/H2 selection is never overridden.

The preference requires both the existing `fallback_allowed` contract and an
explicit H3 network/protocol failure allowlist. Authentication, identity, pin,
socket protection, address assignment, generic packet send timeout/failure,
and platform failures cannot activate it. In particular, this patch does not
reinterpret confirmed IPv6 minimum-MTU termination as a generic network
failure or silently disable IPv6.

H3 packet-send, packet-receive, and control-channel closure all resolve the
same driver result before applying recovery policy. Actor channels can close
before asynchronous socket cleanup finishes, so channel EOF is not substituted
for a typed PMTU, authentication, identity, or protection failure. Shutdown
resolution remains cancellable and uses the existing ten-second packet-operation
budget. If cleanup stalls past that budget, it reports the non-fallback-eligible
`PacketReceiveStalled` failure and aborts the owned driver; it never invents a
network failure to enable H2.

## Established CONNECT-IP recovery

The established-session supervisor classifies driver exits and failed replacement
attempts using the same failure contract. Authentication, identity, configuration,
address-assignment and socket-protection failures stop automatic attempts. Address
family races cancel remaining candidates after an observed terminal error;
H3/H2 aggregate errors cannot erase a terminal child cause. An endpoint pin mismatch
retains the existing single protected enrollment refresh, with unchanged address
assignment and pin verification. Initial connection failures still return to the
caller rather than starting an unlimited background retry loop.

Ordinary network failures retain the 1/2/4/8/15/30-second retry delays with 20%
jitter. A connection lasting at least 60 seconds resets this backoff. An optional
latest-value physical-network subscription distinguishes Unknown, Offline and
Online address families. Offline waits do not start handshakes or advance the
backoff/count. Unknown or a closed observation channel retains timed retry. A usable
new network resets the old backoff after 250 milliseconds of settling; event-driven
attempts start no more than once per second. A newer observation cancels an obsolete
handshake, and cancellation takes precedence over both ready success and recovery.
Waiting and connecting continue draining bounded outgoing packet queues; they do
not retain an unbounded backlog or recreate local proxy listeners.

Android publishes an immutable generation/availability/family snapshot through
JNI. Windows VPN reuses the existing 500-millisecond Agent observations. The new
`AGENT_PHYSICAL_NETWORK_OFFLINE` code means the Agent confirmed there was no usable
physical interface. Older Agents' generic errors, invalid observations and failed
queries mean Unknown. Windows pure proxy mode has no physical subscription and
keeps timed retries. These are scheduling hints, never authorization to bypass
exact-generation socket protection. L4 demand-driven replacement, downstream chain
recovery and platform-service recovery retain their separate policies.

## HTTP/2 liveness

H2 retains its five-second PING cadence and permits only one outstanding PING.
The soft deadline is five seconds before any RTT sample, otherwise
`max(3 * smoothed RTT, smoothed RTT + 4 * RTT variation)`, clamped to 2–10 seconds.
It records one timeout and continues polling the same PONG. The final deadline,
measured from this PING's start, is three times the soft timeout, clamped to
15–30 seconds. A PONG before that deadline preserves the connection. A permanent
blackhole therefore ends the old driver within this deadline plus at most one
PING interval under normal scheduling.

A scheduling gap exceeding 15 seconds permits one five-second resume grace for
the current PING; it cannot repeatedly extend the deadline, and its RTT sample is
discarded. Driver completion, explicit cancellation and network replacement remain
independently interruptible. Final timeout reports `PACKET_RECEIVE_STALLED` on the
H2 path. The cause is published before connection teardown, so send/receive EOF
cannot hide it. Ordinary stream resets still fail immediately. The driver owns
the heartbeat future and closes its socket/egress lease when it exits.

These policies add no saved settings or GUI phases. Waiting remains Reconnecting;
terminal failures use existing failure codes. No extra identifiers or packet
content are recorded. Actual device suspension, physical recovery latency, leak
behavior and energy use require their respective isolated environments; virtual
clock and in-memory H2 regressions are not evidence of those platform outcomes.

## Safety and validation

No packet content, CID, endpoint, key, token, or new sensitive metadata is
logged. Existing bounded PMTU events/counters are reused. No protobuf fields
or reliability identifiers are changed. The vendored quiche implementation
and frozen `oracle/go` sources are unchanged. A sanitized oracle header
fixture verifies the unchanged extended CONNECT shape.

Regressions cover packed small DATAGRAM bursts (including 64 wire packets
carrying two DATAGRAMs each), pre-ready data retention, complete-actor channel
capacity wakeup and shutdown, accepted GOAWAY data continuity, shrinking
GOAWAY IDs/deadlines, socket option readback, silent size-selective loss,
PMTU cooldown/reset/noise guards, and Auto policy security/generation bounds.
Pump-level shutdown tests force each channel to close before driver completion,
also cover already-completed drivers, and verify typed failure preservation,
immediate PMTU fallback, non-fallback security failures, cancellation, and the
cleanup deadline without sockets or platform changes.
The blackhole test exchanges encrypted QUIC wire buffers with a software
size filter; it is not physical-network evidence.

Required workstation checks are the locked Windows Rust helper Clippy/tests,
Rust format check, pinned Android arm64 Clippy, repository policy, Go module
verification/tests, and frozen-oracle verification. Actual route/WFP/TUN
lifecycle, device VPN, external fragmentation/leak observation, and controlled
throughput measurements remain `not_run` without their protected environments.
Linux runtime tests and Apple/FreeBSD option readback must be run on suitable
hosts; cross-compilation alone does not establish those results.
