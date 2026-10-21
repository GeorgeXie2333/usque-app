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

Generated ICMP feedback has a separate, session-owned queue of at most 32
packets including the in-flight delivery. A full feedback queue drops only new
synthetic ICMP and logs rate-limited aggregate counts. Feedback delivery cannot
stop draining stack output or replace ordinary incoming delivery. Cancellation
and session replacement drop pending feedback; mux ownership and quoted-port
restoration still apply. IPv6 minimum-MTU errors remain terminal even when the
feedback queue is full.

The active CONNECT-IP H3 actor also publishes its exact usable inner packet
budget through a private latest-value channel. It uses quiche's current
DATAGRAM bound and actual flow/context framing overhead, not the diagnostic
PMTU sample or received ICMP. An unfinished probe can reduce this TCP budget;
growth waits for confirmed discovery. The sequential session supervisor fences
old actor updates and assigns monotonic command revisions. H2 clears the QUIC
constraint. A single cancellable consumer updates the MASQUE private stack's
TCP send limit, including existing flows and future connections, independently
of lossy ICMP feedback. Failed command delivery cancels its private service
token; runtime shutdown retains task ownership and cleanup.

The TCP limit changes neither saved/interface MTU nor the peer's negotiated
MSS. Unacknowledged data is resegmented by ordinary TCP retransmission. IPv4
can use smaller packets; IPv6 retains a 1280-byte floor and the existing probe
wait/confirmed-insufficiency failure path. H3's per-packet check remains final.
Native TUN TCP continues using ICMP feedback. DIRECT, L4 and a chain's final
inner stack do not inherit the outer budget; the WARP underlay's own TCP stack
does. UDP/raw packet sizing remains separate.

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
not trigger this mechanism. Promotion resets loss history.

An EMSGSIZE rejection during unfinished discovery is treated as a failed size
probe only when its UDP payload exceeds the active path's current ordinary-send
bound. The actor releases that rejected datagram and retains the unsent tail,
including smaller DNS, TLS and ACK packets. It does not pause ordinary traffic
for one second on each rejected probe. It reports the exact local/peer socket
addresses and rejected UDP length to quiche. Only an outstanding probe of that
size on the active, validated path can narrow the search immediately, without
repeating that size or waiting for its loss timer. Ordinary network loss still
uses the configured retry budget. The error does not publish or force an
unverified PMTU: smaller sizes still require peer acknowledgements.
The local rejection bounds only this path's search; delayed ACK/loss callbacks
for rejected sizes cannot clear the new probe or restore a rejected ceiling.
Normal recovery still retires their packet accounting. A new path starts with
the configured ceiling independently of this limit.
Both UDP backends report a successfully sent prefix before a later error, so
the rejected datagram is the first remaining queue entry.

Errors at or below the ordinary-send bound, errors after discovery completes,
and migration-drain errors retain the conservative queue reset, suppression and
revalidation policy. The finite discovery-error and revalidation budgets remain
in force. Confirmed insufficient IPv6 minimum MTU still follows the existing
fail-closed termination policy.

## GOAWAY and automatic recovery

A GOAWAY whose ID still permits the current CONNECT request starts one
30-second grace period. The existing request can continue carrying packets;
no new CONNECT request is opened. Repeated GOAWAY cannot extend the deadline.
Rejection of the current request, stream FIN/reset, connection closure, or
deadline expiry ends the old session through normal teardown.

The saved Auto policy always races H3 and H2 during recovery. H3 failures do not
create a protocol cooldown. Explicit H3/H2 selections remain unchanged. The
existing background H2-to-H3 recovery probes remain available while H2 is healthy.

H3 packet-send, packet-receive, and control-channel closure all resolve the
same driver result before applying recovery policy. Actor channels can close
before asynchronous socket cleanup finishes, so channel EOF is not substituted
for a typed PMTU, authentication, identity, or protection failure. Shutdown
resolution remains cancellable and uses the existing ten-second packet-operation
budget. If cleanup stalls past that budget, it reports the non-fallback-eligible
`PacketReceiveStalled` failure and aborts the owned driver; it never invents a
network failure to enable H2.

## Continuous CONNECT-IP recovery

A continuous connection intent owns local HTTP/SOCKS listeners, the local TCP
stack and one recovery supervisor. The first attempt starts immediately;
retryable failures continue at a fixed one-second interval with no total deadline.
The legacy control call and bounded diagnostic probes retain their startup limits.
Authentication, identity, configuration, address-assignment and socket-protection
failures keep their existing terminal handling and cleanup order. Automatic
candidate pin failures retain the protected enrollment refresh contract.

Automatic mode keeps the complete Consumer pool. H3 starts immediately and H2
starts 250 ms later; candidates share 14 slots, with H3 capped at eight and H2 at
ten. H3 candidates retain their eight-second timeout, H2 candidates use two
seconds. Each failed candidate frees a slot after socket/authorization cleanup.
The candidate queue advances through the round; subsequent rounds shuffle IPv4
and resample IPv6. A previous winner participates normally in the next race.

Physical-network observations accelerate recovery after a 100 ms settle window.
The one-second timer continues without a usable interface or a callback. New
network generations cancel old candidates and refresh address-family availability.
Windows VPN retains its Agent observations; desktop proxy sessions observe local
source addresses and routes. Android passes immutable observations through JNI.
All transport sockets still use the current platform protection and generation.

HTTP/SOCKS requests that require WARP wait on shared state subscriptions. Recovery
waiting does not consume the ten-second remote-connect budget. Local listeners,
active relays, UDP control connections and the CONNECT-IP stack remain running.
DIRECT and REJECT decisions retain their routing behavior. UDP packets are not
queued across an outage, and the association idle timer pauses during recovery.
The Home action remains Cancel during startup and recovery, using the existing
translations in every supported language.

Android normally leaves recovery to Rust. Its service takes over after a native
worker exit, confirms cleanup, and retries immediately, then once per second.
The TUN and connection intent are retained. Physical network loss keeps the UI
connecting/reconnecting. A typed socket-protection failure retains its existing
new-network requirement. User stop revokes both native and service recovery.

HTTP/SOCKS chain exits retain their frontends and verify the selected server after
WARP recovers. Packet chain drivers retain their channels and local stack through
native reconnects with unchanged final addressing. Address changes and terminal
credential/protection failures retain the platform handoff and cleanup rules.

## HTTP/3 liveness

The idle keepalive remains **30 seconds**. ACK/PTO observation is independent:
at least two PTO events with no ACK progress and an outstanding QUIC probe use
`clamp(4 * RTT, 3 seconds, 10 seconds)` as the failure window. Idle business traffic
alone does not start recovery. A path change or scheduling pause resets observation;
resume has a five-second grace. Successful QUIC migration preserves the session;
a failed migration immediately enters endpoint selection.

## HTTP/2 liveness

H2 retains its five-second PING cadence and one outstanding PING. Its PONG
deadline is `clamp(4 * smoothed RTT, 3 seconds, 10 seconds)`, initially three
seconds. Halfway through that deadline it records a delayed PONG observation;
the same PING remains pending until acknowledgement or the deadline. Expiration
ends the driver with `H2LivenessTimeout` and wakes continuous recovery.

A scheduling gap exceeding 5 seconds permits one five-second resume grace for
the current PING; it cannot repeatedly extend the deadline, and its RTT sample is
discarded. Driver completion, explicit cancellation and network replacement remain
independently interruptible. Final timeout reports `PACKET_RECEIVE_STALLED` on the
H2 path. The cause is published before connection teardown, so send/receive EOF
cannot hide it. Ordinary stream resets still fail immediately. The driver owns
the heartbeat future and closes its socket/egress lease when it exits.

These policies add no saved settings or GUI phases. Waiting remains Reconnecting;
terminal failures use existing failure codes. No extra identifiers or packet
content are recorded. The regressions use a virtual clock and in-memory H2;
device suspension, physical recovery latency, leaks and energy use need the
isolated environments.

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
Linux runtime tests and Apple/FreeBSD option readback need native hosts;
cross-compilation does not run them.
