# Experimental L4 proxy mode

L4 is an explicit, TCP-only HTTP/3 mode for Windows and Android. It is not
part of Auto, and it never falls back to CONNECT-IP/H2 or replays established
TCP connections through a replacement session. The default remains CONNECT-IP
with Auto (H3, then H2).

## When to use L4

Use L4 when you want TCP proxying over HTTP/3 and understand the application's
traffic requirements. With L4 alone:

- SOCKS5 and HTTP support TCP connections. SOCKS5 accepts DNS-only UDP
  associations and converts valid UDP/53 queries to TCP DNS; ordinary SOCKS5 UDP
  forwarding is unavailable.
- VPN/TUN accepts TCP. Valid DNS queries on UDP port 53 are converted to TCP DNS,
  while ordinary UDP and remote ping are not supported.
- Applications that require UDP may fail rather than switching to TCP.
- Switching modes or changing TUN use reconnects and ends existing application
  connections.

An enabled OpenVPN-over-TCP chain exit, either a custom **OpenVPN** TCP
configuration or a [VPN Gate](VPN_GATE.md) node, can carry application UDP inside
its additional OpenVPN TCP connection. That does not make the L4 transport itself
UDP-capable. Chain exits that require UDP cannot be enabled with L4; see
[chain proxy compatibility](CHAIN_PROXY.md#compatibility--兼容范围). Existing
explicit direct and platform bypass rules retain their scope.

## Using it

Select **L4 (experimental)** in Advanced network settings, save, and reconnect
when requested. SOCKS5 and HTTP traffic goes directly to CONNECT streams,
without creating a local TCP/IP stack. VPN/TUN uses a bounded local TCP
termination bridge. Switching data planes or toggling TUN in L4 reconnects;
existing application connections are not preserved across those changes.
Listener and authentication edits apply as in other modes. Congestion control
remains a saved preference for the next manual connection.

The transport selector shows a short L4 hint while L4 is selected; the traffic
limits are on the proxy page and above. If the engine does not support L4, the
option is unavailable and explains why; a previously saved L4 selection shows
the unsupported-engine warning until another mode is chosen.

The Home readout and Network quality page show the running data plane, not
just the saved preference. A ready QUIC connection does not mean CONNECT is
allowed, so the CONNECT-verified indicator appears only after a real CONNECT
succeeds; Doctor's QUIC handshake probe does not set it. L4 does not add any
automatic destination probe.

## Implementation reference

The sections below specify identity, protocol, resource and platform behavior.
For shared terminology, see the [technical reference index](README.md#technical-reference--技术规范).

### Identity and endpoints

| Loaded credential provider | Effective L4 SNI |
| --- | --- |
| Consumer (Free or WARP+®) | `consumer-masque-proxy.cloudflareclient.com` |
| Zero Trust | `zt-masque-proxy.cloudflareclient.com` |

The TLS credential loader provides the identity; profile labels and manually
entered SNI/IP values cannot change the provider. Consumer uses the selected
[Automatic or Custom endpoint policy](NETWORK_SETTINGS.md#automatic-endpoints--自动选择端点).
Custom uses the saved numeric pair; Automatic races eligible account endpoints.
Zero Trust retains the existing authenticated registration/managed
endpoint hydration and reauthentication checks. The shared endpoint port remains
configurable. The saved CONNECT-IP SNI is not overwritten and becomes editable
again outside L4. A 403 never rotates SNI or enrolls a new identity. Pin refresh
uses the existing authenticated source once; a further mismatch fails closed.

The Zero Trust mapping comes from
[upstream issue 126](https://github.com/Diniboy1123/usque/issues/126), not an
official long-term protocol guarantee. Source-derived vectors and attribution
are in the [L4 interoperability fixture](../crates/usque-transport/tests/fixtures/l4/README.md);
the fixture is offline and does not contact Cloudflare®.
The [2026-10-14 fingerprint experiment](L4_CHROME_FINGERPRINT_VALIDATION.md)
verified Consumer L4 CONNECT and HTTPS with Default and Chrome 131 profiles at
one ingress. Zero Trust was not tested in that experiment.

### Traffic behavior

- TCP uses classic HTTP/3 CONNECT: only `:method` and `:authority`, followed
  by DATA after final 2xx. No CONNECT-IP address negotiation, extended CONNECT,
  capsules, DATAGRAM requirement, business-data 0-RTT or second TLS/QUIC stack.
- SOCKS UDP ASSOCIATE creates a local DNS-only relay. Valid UDP/53 queries use
  TCP DNS through the selected final stream exit, without requiring upstream
  UDP support. Ordinary proxied UDP remains unavailable; malformed DNS is not
  converted into a TCP connection. The relay ends with its control connection
  or idle timeout. Unknown SOCKS bind addresses are unspecified addresses with
  zero port.
- Ordinary UDP entering TUN is rejected before Geo UDP routing. Eligible
  unicast packets receive rate-limited ICMP unreachable responses. Invalid,
  broadcast/multicast and unsupported packets do not cause direct egress.
- Valid UDP/53 is parsed as DNS, then sent as length-prefixed TCP DNS inside
  L4. A query sent to an application-selected DNS IP retains that resolver.
  The internal DNS endpoint is always installed for L4 TUN, independent of Geo.
  Oversized UDP replies use TC, not arbitrary IP fragmentation.
- Remote proxy DNS uses L4 TCP DNS. Explicit LocalConfigured/System retain
  their existing semantics. EdgeResolved is available only to L4 SOCKS/HTTP:
  the domain goes into CONNECT authority without a local lookup. With Allow
  local network or custom address rules enabled, existing exit DNS resolves
  the name first so routing checks can submit a checked numerical target.
  It does not invent domain information for TUN IP packets.
- With a final HTTP/SOCKS5 proxy, Automatic chain DNS defaults to verified DoH
  through that proxy. Custom or non-default inherited DNS retains TCP DNS.
  Application-selected UDP/53 queries still use TCP to that resolver; a refused
  port-53 CONNECT fails explicitly. DoH never falls back to plaintext or physical
  DNS. See the [chain DNS choices](CHAIN_PROXY.md#http-and-socks5-exits--http-与-socks5-出口).
- Configured Geo TCP and direct DNS rules retain their protected direct
  paths. Encrypted direct DNS never falls back to plaintext. A TCP direct
  attempt falling back to L4 keeps its resolved IP and is subsequently treated
  as an L4 flow, including migration ownership.
- Allow local network also selects protected direct TCP paths in HTTP and
  SOCKS5. Existing platform exclusions remain outside TUN's UDP rejection
  boundary; the setting does not enable ordinary UDP forwarding in L4 alone.

IPv4 options, fragments, IPv6 extension headers, general UDP and remote ICMP
Echo are not transparently supported. Some applications do not fall back from
UDP/QUIC to TCP. L4 is therefore not a transparent replacement for CONNECT-IP.
The TUN System-DNS restriction remains unchanged.

### Resource and recovery contract

Each QUIC connection has one actor. It processes bounded round-robin work and
never waits for a slow client's receive queue. Byte ownership is charged while
retained by application queues or quiche zero-copy send buffers, including
retained slices until ACK/drop. Frontend admission is bounded before parsing
and authentication. Local TCP listeners, half-opens and accepted sockets share
the allocator; a one-shot listener does not allocate a spare accept socket.
The internal `stack_tcp` adapter owns the listener and accepted stream together;
FIN, abort and deferred cleanup retain that ownership even when the command
queue is full. L4 uses this shared adapter with its existing buffer tiers and
performance observer.

The local packet device reserves a bounded TX slot for every packet before
handing smoltcp a transmit token. It never performs a blocking queue send or
discards accepted TCP data on a full queue. Android retains at most one pending
packet per direction while continuing reverse I/O, health sampling and control
handling. See the [backpressure and stop follow-up](L4_BACKPRESSURE_FIX.md).

| Limit | Desktop | Android 64-bit | Android 32-bit |
| --- | ---: | ---: | ---: |
| Business streams | 256 | 128 | 64 |
| Pending business dials | 128 | 64 | 32 |
| Relay bytes per direction | 128 KiB | 128 KiB | 64 KiB |
| Initial connection receive window | 8 MiB | 4 MiB | 2 MiB |
| Aggregate connection receive-window ceiling | 64 MiB | 32 MiB | 8 MiB |
| Initial stream receive window | 1 MiB | 512 KiB | 256 KiB |
| Stream receive-window ceiling | 32 MiB | 16 MiB | 4 MiB |
| Shared application buffer budget | 128 MiB | 64 MiB | 24 MiB |

The receive-window ceiling is conservatively split in half between possible
sessions **before** advertisement: already advertised QUIC credit cannot be
revoked when a draining/replacement session appears. This means a sole session
does not claim the aggregate maximum. Actual stream admission is also bounded
by the server's stream credit and remaining memory. These budgets are not RSS
limits. TUN terminators prefer 256 KiB per direction and downshift to 64 KiB;
packet and TCP-stack reservations are charged to the application budget.
Fixed admission reservations leave one eighth of the budget (up to 16 MiB)
available for DATA progress. Filling all relay/listener reservations must not
leave every accepted stream unable to read or write its next chunk.

One main session and at most one draining session share a two-slot hard cap,
including address-family candidates. Slots remain held until actor/path cleanup.
GOAWAY drains for at most 30 seconds; repeats cannot extend that deadline.
Handshake work is shared independently of individual waiting callers. A target
has a 10-second overall dial budget; QUIC establishment uses the existing
8-second bound and 250-ms address-family racing. Healthy MAX_STREAMS exhaustion
waits within the original deadline, without rebuilding the session.

For locally resolved HTTP/SOCKS5 targets, this ten-second deadline includes
DNS. Each family becomes usable as soon as its answer arrives. Target dialing
allows two attempts and 16 candidates, with 250 ms between launches and immediate
replacement after a fast failure. The unresolved alternative family retains
the second slot. Only losing attempts are cancelled; the winner remains owned
by its session's cancellation token. Edge-resolved CONNECT keeps server-side
resolution.

Session failures restore the outer connection immediately, then retry at a fixed
one-second interval while the connection intent remains enabled. Recovery runs
even without pending business requests. New HTTP/SOCKS requests wait for session
readiness; their ten-second connect budget counts only ready time. Established
L4 streams finish according to their underlying stream result. H3 keepalive
remains 30 seconds, with separate ACK/PTO liveness observation.

DNS has a four-second total deadline, 16 active operations, 64 additional
admitted waiters and at most two reusable TCP streams per resolver. Idle streams
expire after 30 seconds. Transactions/questions and response framing are checked;
network and QUIC-session generations are checked before publishing results.
Credentials and endpoint context are immutable within their cancelled runtime
scope. Account replacement stops the old scope rather than reusing its work.

HTTP/SOCKS5 chain exits also reserve 16 DNS streams and 80 pending DNS dials,
separately from ordinary TCP and UDP-association control connections. DNS keeps
its class through proxy-endpoint address racing and the underlying L4 dialer,
so full business connection slots do not consume the DNS slots. All classes
still share the existing memory budget; exhausted memory remains a bounded
failure rather than permission to exceed that budget or bypass the final exit.

The local SOCKS5 UDP relay converts at most four DNS queries concurrently per
association, including replies waiting for delivery. A slow query does not block
other queries or ordinary UDP on that association. Each query reserves bounded
request/response memory; excess queries receive SERVFAIL immediately. Replies
retain their query IDs and resolver addresses even when they complete out of
order. Closing the control connection cancels and joins the outstanding work.

### Observability and safety

Schema 15 appends the data-plane setting; migrating any configuration older
than schema 15 sets it to CONNECT-IP.
The protobuf/JNI additions report mode, capabilities, CONNECT verification,
stream and DNS counters, buffer pressure, TUN/half-open counts and migration
ownership. Unknown status is not success. CONNECT-IP payload and DATAGRAM
metrics are unsupported in L4. Application bytes and outer QUIC bytes remain
separate. Added counters contain no target names/IPs, DNS content, CIDs or keys.

Windows continues to use the Agent transaction, exact endpoint/protocol/generation
leases, packet ring, WFP and restoration journal. Local interface addresses come
from registration, not fabricated CONNECT-IP negotiation. Android retains
VpnService protection, network binding, generation checks and lifecycle policy.
No broad process/UDP bypass, platform-safety downgrade or installer behavior is
introduced. Failed traffic does not silently become direct traffic.

Android reports native stop requests and confirmations separately. A native
stop waits up to five seconds for its worker to finish, retaining ownership and
refusing a replacement runtime when completion is unconfirmed. The native TUN
duplicate is released before awaiting backend cleanup; Java's protective FD is
retained for fail-closed recovery unless the user explicitly disconnects.
`pending_cleanup` includes queued/unconfirmed native stops, and such a snapshot
does not claim that the native runtime is stopped.

### Validation and performance evidence

Follow the complete applicable matrix in [CONTRIBUTING](../CONTRIBUTING.md).
Additional safe tests are `l4::actor_tests`, `l4::client_tests`, `l4::tun_tests`,
the L4 GUI tests and the vendored allocator/listener tests:

```powershell
cargo test --manifest-path third_party/ts_netstack_smoltcp_core/Cargo.toml --lib --locked
```

The [performance scenario manifest](l4-performance-scenarios.json) defines
separate proxy and TUN measurements. Run each case at least seven times with
fixed candidate, baseline, platform and network metadata, and report medians,
dispersion and request-level p95/p99.

Live Cloudflare interoperability, real Windows/Android lifecycle, externally
observed leaks and controlled performance are not tested on a development
workstation; they need the protected environments in
[Contributing](../CONTRIBUTING.md#development-machines).

---

Cloudflare and WARP+ are trademarks and/or registered trademarks of Cloudflare, Inc. in the United States and other jurisdictions.
