# Network quality metrics

Usque's transport layer maintains one process-local, non-persistent network
quality source per managed tunnel. It is sampled at most once per second and is
cancelled with the tunnel runtime. This internal model does not itself emit IPC,
write a log, persist history, or upload data.

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
tunnel remains usable. The locked quiche 0.29.3 `PathStats` exposes RTT, loss,
congestion window, delivery rate, and PMTU, but not current bytes in flight;
that field is explicitly `Unsupported` rather than inferred.

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
| `H3DatagramSend` | quiche DATAGRAM send queue | 1,024 datagrams and 1,382,400 bytes; a bounded metadata shadow is reconciled to quiche's public queue length |
| `H3WireSend` | Pacing-aware QUIC wire deque | 64 datagrams and 86,400 bytes; entries complete only after a full UDP send |
| `TransportToTun` | Managed/final TUN batch sink | 16 batches and 4 MiB; a re-attach atomically replaces the old tracked sink |
| `TransportToProxy` | Packet mux into the smoltcp proxy pipe | Existing 1,024-packet pipe; logical actor handoff time and bytes are tracked for the same locked-dependency reason |
| `DirectDnsRequests` | Active GeoSite direct DNS queries | 512 requests and 32 MiB; semaphore rejection is a queue drop and returns SERVFAIL |

Packet queues keep the oldest timestamp in their FIFO head metadata and do not
take a mutex in the packet path. Direct DNS requests can complete out of order,
so that low-rate control path keeps a bounded timestamp multiset under a local
mutex; snapshots still read only the published atomic oldest timestamp.

The H2 ADDRESS_REQUEST rejection path is no longer unbounded. Both its pending
control deque and writer channel are capped at 64 capsules, with a 256 KiB byte
budget. Saturation fails closed with `SendQueueFull`.

## HTTP/2 flow control and PING

CONNECT-IP uses an explicit h2 client Builder with a 4 MiB stream receive
window, an 8 MiB connection receive window, and server push disabled. These
settings affect only the peer-to-client CONNECT-IP data path. Registration and
future encrypted-DNS control clients keep independent small default Builders;
the send-buffer limit is unchanged.

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

Snapshot types contain only enums, integers, durations, booleans, and a
process-local random connection instance UUID. They do not contain socket addresses,
endpoint names, QNAMEs, DNS server names or bootstrap IPs, SSID/BSSID, QUIC
connection IDs, tokens, packet payloads, or free-form errors. Direct DNS and
migration failures use closed reason-code enums.
