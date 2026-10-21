# Continuous connection and proxy recovery

The Connect action creates a cancellable connection intent. Local preparation
starts once; transport recovery continues until a connection succeeds, the user
stops, or a credential/configuration/protection failure requires user action.

| Default | Value |
| --- | --- |
| Retry after a failed round | 1 second, indefinitely |
| Physical event settling | 100 ms |
| Auto H2 start after H3 | 250 ms |
| Candidate limits | 14 total; H3 8; H2 10 |
| Candidate timeout | H3 8 seconds; H2 2 seconds |
| Alternate address family delay | 250 ms |
| H3 idle keepalive | 30 seconds |
| H2 PING interval | 5 seconds |
| ACK/PONG deadline | 4 × RTT, clamped to 3–10 seconds |
| Scheduling-pause grace | 5 seconds |
| HTTP/SOCKS remote connect budget | 10 seconds of ready time |

Automatic selection retains the full configured Consumer pool, port, SNI and
IP-family policy. H2 excludes the HTTPS-only `.1` hosts, reshuffles IPv4 and
resamples IPv6 after a completed round. There is no persistent winning-endpoint
cache or blacklist. A newer network generation retires old candidates before
selecting another winner. Candidate slots remain occupied until cleanup completes.

Recovery waits on a monotonic timer and network observations. Missing physical
interfaces keep the same intent active. Windows proxy observation reads source
addresses and default routes; VPN mode uses existing Agent observations and
exact endpoint authorizations. Android retains its TUN and updates underlying
network bindings. Linux checks its Agent's physical snapshot every second;
transient missing snapshots and routes preserve the session. Windows automatic
candidate authorization waits for the Agent to release a retiring lease before
retrying the same exact endpoint and generation.

HTTP and SOCKS listeners remain bound during recovery. Requests wait through a
shared health subscription before sending application bytes. DNS/TCP work
interrupted by a new outage returns to waiting, carrying its remaining ready-time
budget. Request bodies use backpressure. Existing relays keep their normal
EOF/RST semantics and CONNECT-IP retains its local TCP state. DIRECT and REJECT
routes are evaluated before WARP readiness. UDP associations retain their local
port and control connection; idle expiration pauses and outage datagrams are
dropped. A failed chained SOCKS UDP upstream is replaced on the next datagram.
UDP DNS/send tasks are bounded and cancelled when their tunnel generation ends.
HTTP and encrypted-DNS pools retire entries from previous generations.

Packet-based chain exits activate local HTTP/SOCKS protocol handling while
waiting for the final network. DIRECT requests can run immediately. The final
stack adopts those listeners and wakes existing waiting requests. OpenVPN and
WireGuard recovery keep packet channels alive; a new Proton WireGuard generation
creates a new local-agent authorization session before business traffic resumes.
Pipelined SOCKS bytes remain bounded and survive a client write-half-close.

The shared frontend admission limits bound connections before protocol parsing.
Connection diagnostics expose `waiting_requests` and `peak_waiting_requests`.
Candidate IDs, protocol readiness, retry scheduling and network changes retain
the existing bounded connection timeline. Repeated recovery reasons share a row
with a repetition count. No addresses or credentials are added
to these counters.

## Control protocol

`Capabilities.continuous_connection` uses field 51. A capable client sends
`ConnectRequest.continuous` (field 2) or `RetryRequest.continuous` (field 1).
The response acknowledges Preparing; status subscriptions deliver later progress.
The desktop client uses a five-second acknowledgement deadline. On timeout it
queries status without resubmitting the command. Missing flags retain bounded
legacy startup. Android carries the flag as runtime MethodChannel/Binder metadata;
the saved Profile schema is unchanged. Reconfiguration inherits the running
continuous flag unless the caller explicitly supplies another value. Android
completes service recovery and account handoff after the current native snapshot
is ready and, in VPN mode, its TUN is attached.

Diagnostics append `ConnectionMetrics.waiting_requests` (67),
`peak_waiting_requests` (68) and `ConnectionEvent.repetitions` (13).

Duplicate requests wake the existing recovery task. Stop cancels the intent and
all pending candidate/request work. Shutdown and data removal join an unpublished
startup worker before completing platform cleanup. Continuous network startup releases the
desktop mutation lock while awaiting transport work. The UI retains its Cancel
action throughout Connecting and Reconnecting, with existing localized labels.

## Validation

Deterministic coverage includes two hours of fixed retry intervals, offline
periodic recovery, 100 ms network settling, H3 blackholes with H2 admission at
250 ms, full H2 pool traversal, shared candidate limits, long request waits,
POST body delivery, client cancellation, and old/new protobuf defaults.

Workstation gates and isolated measurements are recorded in
[the validation record](CONTINUOUS_CONNECTION_VALIDATION.md).
