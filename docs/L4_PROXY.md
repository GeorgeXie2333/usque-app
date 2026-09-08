# Experimental L4 proxy data plane

L4 is an explicit, TCP-only HTTP/3 mode for Windows and Android. It is not
part of Auto, and it never falls back to CONNECT-IP/H2 or replays established
TCP connections through a replacement session. The default remains CONNECT-IP
with Auto (H3, then H2).

## Using it

Select **L4 (experimental)** in Advanced network settings, save, and reconnect
when requested. SOCKS5 and HTTP traffic goes directly to CONNECT streams,
without creating a local TCP/IP stack. VPN/TUN uses a bounded local TCP
termination bridge. Switching data planes or toggling TUN in L4 reconnects;
existing application connections are not preserved across those changes.
Frontend listener/authentication edits retain the existing hot-update contract.
Congestion control remains a saved preference for the next user session.

An older engine without all required L4 capability fields cannot enable L4.
The Home readout and Network quality page show the running data plane, not
just a saved preference. QUIC readiness does not prove CONNECT permission:
the CONNECT-verified indicator is set only after a real successful CONNECT.
Existing application exit-information requests are unchanged; L4 itself adds
no automatic destination probe. Doctor's QUIC handshake probe alone does not
mark CONNECT verified.

## Identity and endpoints

| Loaded credential provider | Effective L4 SNI |
| --- | --- |
| Consumer (Free or WARP+) | `consumer-masque-proxy.cloudflareclient.com` |
| Zero Trust | `zt-masque-proxy.cloudflareclient.com` |

The TLS credential loader provides the identity; profile labels and manually
entered SNI/IP values cannot change the provider. Consumer uses the configured
endpoint IPs. Zero Trust retains the existing authenticated registration/managed
endpoint hydration and reauthentication checks. The shared endpoint port remains
configurable. The saved CONNECT-IP SNI is not overwritten and becomes editable
again outside L4. A 403 never rotates SNI or enrolls a new identity. Pin refresh
uses the existing authenticated source once; a further mismatch fails closed.

The Zero Trust mapping comes from
[upstream issue 126](https://github.com/Diniboy1123/usque/issues/126), not an
official long-term protocol guarantee. Source-derived vectors and attribution
are in the [L4 interoperability fixture](../crates/usque-transport/tests/fixtures/l4/README.md).
Live Consumer and Zero Trust reachability must be recorded separately; offline
fixture success is not a Cloudflare account test.

## Traffic behavior

- TCP uses classic HTTP/3 CONNECT: only `:method` and `:authority`, followed
  by DATA after final 2xx. No CONNECT-IP address negotiation, extended CONNECT,
  capsules, DATAGRAM requirement, business-data 0-RTT or second TLS/QUIC stack.
- SOCKS UDP ASSOCIATE returns command-not-supported and creates no relay.
  Unknown SOCKS bind addresses are unspecified addresses with zero port.
- Ordinary UDP entering TUN is rejected before Geo UDP routing. Eligible
  unicast packets receive rate-limited ICMP unreachable responses. Invalid,
  broadcast/multicast and unsupported packets do not cause direct egress.
- Valid UDP/53 is parsed as DNS, then sent as length-prefixed TCP DNS inside
  L4. A query sent to an application-selected DNS IP retains that resolver.
  The internal DNS endpoint is always installed for L4 TUN, independent of Geo.
  Oversized UDP replies use TC, not arbitrary IP fragmentation.
- Remote proxy DNS uses L4 TCP DNS. Explicit LocalConfigured/System retain
  their existing semantics. EdgeResolved is available only to L4 SOCKS/HTTP:
  the domain goes into CONNECT authority without a local lookup. It does not
  invent domain information for TUN IP packets.
- Configured Geo TCP and direct DNS rules retain their protected direct
  paths. Encrypted direct DNS never falls back to plaintext. A TCP direct
  attempt falling back to L4 keeps its resolved IP and is subsequently treated
  as an L4 flow, including migration ownership.
- Existing application/LAN/CIDR bypass rules remain explicit platform rules.
  Traffic excluded from TUN is outside its UDP rejection boundary.

IPv4 options, fragments, IPv6 extension headers, general UDP and remote ICMP
Echo are not transparently supported. Some applications do not fall back from
UDP/QUIC to TCP. L4 is therefore not a transparent replacement for CONNECT-IP.
The TUN System-DNS restriction remains unchanged.

## Resource and recovery contract

Each QUIC connection has one actor. It processes bounded round-robin work and
never waits for a slow client's receive queue. Byte ownership is charged while
retained by application queues or quiche zero-copy send buffers, including
retained slices until ACK/drop. Frontend admission is bounded before parsing
and authentication. Local TCP listeners, half-opens and accepted sockets share
the allocator; a one-shot listener does not allocate a spare accept socket.

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

Session failures use jittered 1/2/4/8/15/30-second backoff. Active flows retain
the 30-second keepalive baseline. Idle sessions may expire and reconnect on
the next request. Only an undelivered CONNECT can be retried; accepted TCP bytes
are not replayed. Same-endpoint/same-family verified path migration keeps L4
streams and TUN mappings. A real session rebuild ends its old flows.

DNS has a four-second total deadline, 16 active operations, 64 additional
admitted waiters and at most two reusable TCP streams per resolver. Idle streams
expire after 30 seconds. Transactions/questions and response framing are checked;
network and QUIC-session generations are checked before publishing results.
Credentials and endpoint context are immutable within their cancelled runtime
scope. Account replacement stops the old scope rather than reusing its work.

## Observability and safety

Schema 15 appends the data-plane setting; schema 14 migrates to CONNECT-IP.
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

## Validation and performance evidence

Follow the complete applicable matrix in [CONTRIBUTING](../CONTRIBUTING.md).
Additional safe tests are `l4::actor_tests`, `l4::client_tests`, `l4::tun_tests`,
the L4 GUI tests and the vendored allocator/listener tests:

```powershell
cargo test --manifest-path third_party/ts_netstack_smoltcp_core/Cargo.toml --lib --locked
```

The [performance scenario manifest](l4-performance-scenarios.json) keeps proxy
and TUN measurements separate. It is a sampling contract, not a generated pass
report or a replacement for existing performance thresholds. Run each case at
least seven times with fixed candidate, baseline, platform and network metadata.
Report medians, dispersion and request-level p95/p99, not an unmeasured speedup.

Live Cloudflare interoperability, real Windows/Android lifecycle, externally
observed leak safety and controlled performance measurements are **not run on
a development workstation**. They require the distinct protected environments
in [AGENTS](../AGENTS.md). Missing/failed evidence remains `not_run`/`failed`,
never `passed`; these supplemental reports do not become publication prerequisites.
