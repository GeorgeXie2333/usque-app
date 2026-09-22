# Network quality metrics

Usque's transport layer maintains one process-local, non-persistent network
quality source per managed tunnel. It is sampled at most once per second and is
cancelled with the tunnel runtime. This internal model does not itself emit IPC,
write a log, persist history, or upload data.

Each connection attempt has private transport state, UDP/allocation counters,
and H3 queue metrics. Only selection by the transport supervisor promotes an
attempt into the sampled source; losing Happy Eyeballs attempts and non-bearing
recovery probes cannot reset or overwrite the active connection. Runtime queues,
direct DNS state, and runtime allocation counters remain shared across reconnects.
One snapshot captures one selected attempt, and a promotion resets interval-loss
and classification baselines without replacing the runtime's sampler.

## Availability

Every metric carries one of four states:

- `Available`: a real observation is present.
- `Unsupported`: the active transport or locked dependency cannot provide it.
- `NotReady`: the transport supports it but has no valid interval/sample yet.
- `Stale`: the last valid observation is retained but the transport has marked
  it stale. H3 uses the three-second sample age; H2 uses an actual PING timeout.

A numeric zero is therefore never used as a substitute for unsupported,
not-ready, or stale data. H2 loss, congestion window, bytes in flight, and PMTU
are `Unsupported`. H2 PING RTT is `NotReady` before the first PONG, `Available`
after a valid PONG, and `Stale` after its adaptive deadline. If the locked h2
build cannot provide `PingPong`, H2 RTT is explicitly `Unsupported` while the
tunnel remains usable. The locked quiche 0.29.3 `PathStats` exposes smoothed RTT,
minimum RTT, variance, loss, congestion window, delivery rate, and PMTU, but not
latest RTT or current bytes in flight. Those two fields are explicitly
`Unsupported`; smoothed RTT is never relabeled as a latest RTT sample.

For H3, PMTU remains `NotReady` while quiche is probing and becomes available
only when `Connection::pmtu()` reports a completed result. The effective
CONNECT-IP payload is the smaller of the Profile MTU and quiche's real
DATAGRAM writable length after the encoded quarter-stream/context prefix. A
result below the IPv6 1280-byte minimum is `Degraded`; it is never advertised
as a usable IPv6 MTU.

Interval H3 loss uses monotonic deltas:

```text
delta_lost_packets * 10,000 / delta_sent_packets
```

The first interval, an interval with no sent packets, and a counter reset are
`NotReady`. A new connection instance clears the delta baseline and short-term
quality history.

## Bounded queue map

Queue payloads are never copied for measurement. Tokio queues use tracked items
plus an item capacity and byte semaphore. Actor-owned queues use the same atomic
entry accounting. Enqueue/dequeue/drop counts, item/byte high-water marks,
oldest age, close, and cancellation state are process-local numeric data.

| Queue kind | Actual boundary | Bound and accounting |
| --- | --- | --- |
| `TunToTransport` | Platform TUN attach to the packet mux | 1,024 packets; explicit 67,107,840-byte ceiling, which does not tighten the existing valid-IP size bound |
| `ProxyToTransport` | smoltcp proxy pipe drained by the single packet mux actor | Existing 1,024-packet pipe; logical actor handoff time and bytes are tracked because the locked dependency does not expose its private flume depth |
| `TransportOutgoingPackets` | Managed runtime into the reconnecting transport supervisor | 1,024 packets and 67,107,840 bytes |
| `H3DatagramSend` | quiche DATAGRAM send queue | 1,024 datagrams and a family-specific bound: 1,507,328 bytes for IPv4 or 1,486,848 for IPv6; a bounded metadata shadow is reconciled to quiche's public queue length |
| `H3WireSend` | Pacing-aware QUIC wire deque | 64 datagrams and a family-specific bound: 94,208 bytes for IPv4 or 92,928 for IPv6; entries complete only after a full UDP send |
| `TransportToTun` | Managed/final TUN batch sink | 16 batches and 4 MiB; a re-attach atomically replaces the old tracked sink |
| `TransportToProxy` | Packet mux into the smoltcp proxy pipe | Existing 1,024-packet pipe; logical actor handoff time and bytes are tracked for the same locked-dependency reason |
| `DirectDnsRequests` | Active GeoSite direct DNS queries | 512 requests and 32 MiB; semaphore rejection is a queue drop and returns SERVFAIL |

Packet queues keep the oldest timestamp in their FIFO head metadata and do not
take a mutex in the packet path. Direct DNS requests can complete out of order,
so that low-rate control path keeps a bounded timestamp multiset under a local
mutex; snapshots still read only the published atomic oldest timestamp.

The proxy receive pipe transfers an already-owned packet into its reserved
queue slot without copying the payload. Borrowed callers retain a copying
convenience path. Both interfaces preserve queue capacity, cancellation and
the device's per-TxToken reservation; this does not remove QUIC or kernel copies.

Android reads TUN packets into eight-slot slabs and transfers disjoint
`BytesMut` views through `TunPacketIo::start_send_mut_packet`. CONNECT-IP keeps
these views mutable across the TUN queue, mux/NAT edits and supervisor queue.
It freezes each view only after validation, TTL/Hop Limit decrement and checksum
repair, so live neighboring slab packets do not force a whole-packet copy.
The existing `Bytes` entry points remain available and copy only when a shared
allocation needs mutation. L4 still receives its existing immutable packet
representation. The VPN Gate external boundary freezes for native delivery
without adding another TTL decrement.

Memory-only regressions compile the actual Android slab producer and check
pointer identity across 130 packets, batching, both IP families, NAT collisions,
MTU changes, cancellation, backpressure and reattachment. These tests establish
ownership and packet correctness; throughput, CPU/RSS and device lifecycle
measurements require their separate isolated environments. MTU, buffer presets,
configuration, JNI, IPC and protobuf formats are unchanged.

The H2 ADDRESS_REQUEST rejection path is no longer unbounded. Both its pending
control deque and writer channel are capped at 64 capsules, with a 256 KiB byte
budget. Saturation fails closed with `SendQueueFull`.

UDP receive truncation is a per-datagram drop, not a connection failure. Both
receive backends discard and count payloads above the 2048-byte bound while
preserving valid datagrams in the same drain. Discarded datagrams count toward
the 64-datagram actor budget; an all-discarded drain yields before retrying so
cancellation and other tasks remain responsive. Prefetched channel entries
retain both item and byte permits until actual consumption.

## Packet mux mapping bounds

CONNECT-IP's TUN/proxy attribution table admits at most 65,536 main flows on
every platform, plus 8,192 outgoing and 8,192 incoming fragment mappings.
Paired reverse indexes share their forward entry's lifetime. New mappings are
rejected at capacity before packet headers or associated indexes change;
existing mappings remain usable. Allocation grows on demand, not at startup.

The owning mux schedules maintenance once per second, checking at most 4,096
entries from each table per pass. Each mapping has one scan-queue entry;
packet activity refreshes its timestamp without appending scan work. The idle
timeout remains five minutes, with expiry discovered on a subsequent bounded
pass. Empty tables release their backing storage; nonempty tables keep bounded
capacity. Capacity logs contain only reason codes and cumulative counts and
are emitted at most once per thirty seconds. No traffic identifiers are added.

## Direct gateway TCP memory

The direct TUN gateway charges ordinary DNS listeners, half-open sockets and
accepted TCP sockets to its existing platform TCP budget: 48 MiB on Android32,
128 MiB on Android64 and 256 MiB on desktop. Direct sockets retain symmetric
1 MiB receive/send buffers. Business flows use the shared one-shot adapter,
which does not reserve a spare accept socket. A failed admission releases its
NAT reservation and follows the existing routing fallback; established flows
are not evicted. These allocator bounds are not process RSS limits. Proxy TCP
buffer tiers and L4 application budgets are unchanged.

## Proxy DNS resolution

Remote and configured proxy DNS share a four-second absolute deadline across
A/AAAA lookups, socket admission, bind/send/receive and resolver retries. System
lookup awaits are also bounded; late system results are discarded. Validated
NODATA and NXDOMAIN end that query type's retry chain, while temporary failures
may use the remaining deadline at another configured server. Question and
record validation precedes negative-answer classification. No DNS mode falls
back to a different mode.

The internal candidate interface owns unfinished query futures and yields each
address family as it completes. Dropping it cancels owned work. The aggregate
resolver remains available to UDP callers, preserving IPv4-first ordering for
remote/configured results and OS ordering for System mode.

HTTP and SOCKS5 share one target connector with a ten-second deadline covering
both resolution and dialing. It starts with the first available address, spaces
additional attempts by 250 ms, and admits at most two simultaneous attempts and
16 unique candidates. The second slot is reserved for the other family until
that lookup finishes; once only one family remains, both slots can use it.
Fast failures refill immediately. Resource-budget rejection stops new attempts
while allowing an existing attempt to finish. Authentication rejection and
cancellation stop the group; a winner cancels the remaining lookups and dials.
Direct-route policy and server-side name resolution keep their existing paths.

Stack creation retains ownership until its response is consumed. Cancellation
before allocation, during a TCP handshake, or with a queued response reclaims
the socket and TCP reservation. A cancellation wake also works with an idle
stack; a full command queue is drained before the normal cleanup pass. DNS UDP
owners retry close admission when that queue is full. These are in-memory
resource guarantees, not measured throughput or device lifecycle results.

## HTTP/2 flow control and PING

CONNECT-IP uses an explicit h2 client Builder with a 4 MiB stream receive
window, an 8 MiB connection receive window, and server push disabled. These
settings affect only the peer-to-client CONNECT-IP data path. Registration and
future encrypted-DNS control clients keep independent small default Builders;
the send-buffer limit is unchanged.

H2 receive batches span already-ready DATA frames, up to the common 64-packet
or 256 KiB limit. After the first packet, a batch polls at most 64 additional
DATA frames and returns immediately when no more data is ready. This also
bounds lookahead through empty or control-only frames. A peer that flushes one
DATAGRAM per DATA frame therefore does not force every buffered packet to use
its own downstream batch slot. The 16-slot handoffs retain their existing
memory limits; batches are not delayed to fill them. Partial capsules remain
owned by the receiver across cancellation, receive capacity is returned once
per consumed frame, and a lookahead failure is reported on the next receive
after delivering the already-completed batch.

One protocol PING may be outstanding at a time. The interval is five seconds.
The deadline is five seconds before the first sample, then three times smoothed
RTT clamped to two through ten seconds. Smoothed RTT and variance use an integer
EWMA with alpha 1/8; minimum RTT is monotonic for the connection. A timeout
marks retained RTT stale and increments a bounded counter but never closes the
tunnel. Three consecutive failures may classify quality as `Poor`; the existing
connection driver alone decides whether the transport has failed.

Each `reserve_capacity`/`poll_capacity` wait is measured with an actor-local
monotonic timestamp. A successful wait longer than one millisecond increments
the unified `capacity_wait` stall count and total/max duration. Errors and task
cancellation have separate counters and are never counted as successful stalls.

## H3 DPLPMTUD

H3 configures quiche 0.29.3 DPLPMTUD with three attempts per probe size. The
outer UDP payload ceiling is 1472 bytes for IPv4 and 1452 for IPv6. quiche's
locked implementation keeps ordinary data at its conservative 1200-byte QUIC
floor until a probe succeeds; the ceiling is used only as the optimistic probe
bound. Each active address pair has independent publication and revalidation
state.

An `EMSGSIZE` drops the already-generated wire queue, records
`pmtu_send_too_large_count`, and suppresses sends for one second. If quiche has a
completed PMTU result, it starts one `revalidate_pmtu()` round; three invalidated
completed results inside ten seconds terminate the H3 path. While discovery is
already incomplete, a send error is only a failed size probe: quiche continues
its existing loss-based search without restarting it. A separate 30-error budget
covers the locked search's ten bounded sizes with three attempts each, then
terminates with `PMTU_REVALIDATION_EXHAUSTED`. The typed reason survives startup
as well as an established driver failure and permits the existing safe fallback.
No inner or outer datagram is truncated.

During discovery and revalidation both published PMTU numbers are `NotReady`.
quiche's conservative data-send cap is not exported as a measurement. The last
validated value is retained only to count real changes when a new completed
result arrives; newly promoted paths never inherit it.

Other pre-existing bounded structures are deliberately not separate quality
queues:

| Structure | Why it is not another quality queue |
| --- | --- |
| H2 writer channel | Capacity is one encoded batch, and `TransportOutgoingPackets` measures the owning supervisor boundary; each `PacketBatch` is already capped at 64 packets and 256 KiB. |
| H3 actor outgoing/incoming channels | The outgoing capacity is one bounded `PacketBatch`; the incoming side is represented by `TransportToTun`/`TransportToProxy`, while `H3DatagramSend` measures the next protocol queue. |
| Direct-gateway inbound channel | It carries explicitly bypassed direct traffic rather than the managed transport path and remains bounded at 1,024 packets. |
| Per-association SOCKS UDP response channel | It is frontend-local, bounded, and downstream of the `TransportToProxy` handoff already represented in the model. |
| Split-DNS UDP response channel | It is a bounded delivery queue after the measured `DirectDnsRequests` operation; counting it again would double-count one DNS request. |
| Supervisor ICMP return deque | Control flow permits at most one bounded `PacketBatch`; outgoing reads pause until it is delivered. |

## Snapshot and quality label

The sampler keeps at most 30 one-second classification signals. It returns
`LimitedData` until five samples exist. H3 requires available RTT and interval
loss; H2 requires its real RTT and does not become poor merely because QUIC-only
metrics are unsupported.

- `Good`: RTT below 75 ms, H3 loss below 0.5%, every registered queue below
  50%, and no queue drop in the retained window.
- `Fair`: RTT below 150 ms, H3 loss below 2%, every registered queue below 80%,
  and no sustained drop.
- `Poor`: a threshold is exceeded, drops are sustained, PMTU is degraded, a new
  migration failure is observed, or three consecutive H2 PINGs fail.
- `Disconnected`: there is no current connection instance.

## Privacy

The UI retains at most sixty one-second slots and 300 raw points per local
connection instance. Missing/stale/disconnected samples are gaps, not zero;
counter baselines reset on a new instance. H2 loss/congestion/PMTU/migration
are Unsupported, pending H2 PING is NotReady, and three failed PINGs retain the
last measured RTT as Stale. Metrics are not persistence or upload inputs.
The internal metrics rollback stops quality publication and its capability,
not transport work or safety counters. See [rollback](network-quality-rollback.md).

Snapshot types contain only enums, integers, durations, booleans, and a
process-local random connection instance UUID. They do not contain socket addresses,
endpoint names, QNAMEs, DNS server names or bootstrap IPs, SSID/BSSID, QUIC
connection IDs, tokens, packet payloads, or free-form errors. Direct DNS and
migration failures use closed reason-code enums.
