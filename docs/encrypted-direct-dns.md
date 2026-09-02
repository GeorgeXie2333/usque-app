# Encrypted direct DNS contract

Direct DNS is an explicit Profile choice: physical-system DNS (the legacy
default), DNS over HTTPS, or DNS over TLS. It changes only Geo-selected direct
queries. WARP/tunnel DNS, application-owned encrypted DNS, and unrelated
traffic are not intercepted or decrypted. There are no vendor presets or
embedded resolver bootstrap addresses.

## Configuration and trust

`DirectDnsSettings` is validated in core before opening a connection. TLS
server names are IDNA-normalized DNS names, at most 253 characters/ASCII bytes
after normalization, and validated with rustls `ServerName`. URL syntax,
wildcards, empty labels, control characters and whitespace are rejected rather
than silently trimmed. Numeric IPs belong only in the bootstrap list.

The list contains 1–8 distinct numerical unicast IPs. Private unicast is
allowed for enterprise resolvers. Unspecified, multicast, IPv4 broadcast and
unscoped IPv6 link-local addresses are rejected. The current `IpAddr` settings
do not carry a link-local scope, so link-local IPv6 cannot be used.

Port zero canonicalizes to 443 (DoH) or 853 (DoT). DoH paths are ASCII paths
starting with one `/`, at most 256 bytes, with no query, fragment, URL/host
syntax, backslash, whitespace or control characters. An empty DoH path becomes
`/dns-query`. DoT requires an empty path. Canonical physical-system JSON retains
only `mode`; stale custom fields are cleared before saving.

Production TLS uses the existing explicit ring provider and webpki roots,
normal name/validity/chain verification and no early data. There is no custom
CA, certificate-ignore switch or pin override. Test roots exist only in unit
fixtures. DoH requires negotiated `h2`; DoT does not require HTTP ALPN.

After TLS chooses a bootstrap address, HTTP/2 preface failures retain that
address for the existing one-retry exclusion policy. The failed stream and
lease are dropped before retry. The two-address budget, total query deadline,
and no-retry-on-timeout policy are unchanged. Both an outer deadline expiry
and a transport-reported preface I/O timeout preserve the Timeout classification.

## Protocol and semantic validation

DoH uses HTTP/2 POST, HTTPS authority derived from the configured name/port,
the configured path and `application/dns-message` for Content-Type and Accept.
Only status 200 is accepted; redirects are never followed. Media types are
parsed completely, including legal token/quoted parameters, not matched with
substring searches. Bodies are capped at 65,535 bytes; rejected/abandoned
streams are reset. Its receive windows are 65,535 bytes per stream and
256 KiB per connection, independent of MASQUE tuning. These exchanges follow
the wire format in [RFC 8484](https://www.rfc-editor.org/rfc/rfc8484.html).

DoT uses the two-byte big-endian message length from
[RFC 7858](https://www.rfc-editor.org/rfc/rfc7858.html), with one outstanding
query per connection. A zero length, EOF, canceled partial exchange, or invalid
response discards that connection. Neither encrypted protocol applies the
physical UDP-to-TCP truncation fallback.

Both protocols reuse the existing Split DNS parser for transaction ID,
question, opcode, record, CNAME, TTL and route-hint validation. No second DNS
semantic parser is introduced. DNS name decoding is bounded before allocation
can grow beyond the supported name length. The application's UDP response-size
limit still applies, independently of encrypted upstream transport.

## Bounds, cancellation and generations

- At most four encrypted DNS sockets/connections, including connecting,
  retiring and Happy Eyeballs losers. Permits live with actual I/O until its
  socket and platform lease have been dropped.
- DoH: four connections, sixteen concurrent requests per connection, sixty-four
  total. DoT: four connections, one request per connection and at most sixty
  more admitted queries waiting for a slot.
- The sixty-four-query encrypted budget is separate from Split DNS's 512-task
  budget. The latter includes queued UDP replies and TCP sessions. Full
  admission returns a safe failure; no unbounded worker queue is spawned.
- Idle connections close after sixty seconds; a connection is replaced after
  at most one thousand queries. In-flight DoH streams finish before normal
  retirement; network/profile cancellation closes them immediately.
- A query has a four-second total deadline, with 2.5 seconds for socket
  preparation/connect/TLS and at most three seconds for request/response,
  always capped by the total deadline. At most one request retry uses a
  different bootstrap IP, and one query visits at most two bootstrap IPs.
- Bootstrap candidates are filtered by authoritative family availability.
  IPv6/IPv4 Happy Eyeballs starts the alternative after 250 ms (or immediately
  after a failed first candidate). Losers release their socket and lease.
- Exact-generation protection is checked before setup, after bind/protect,
  before/after TLS, before the request and before returning its result. Android
  uses the exact `Network`; Windows VPN uses the Agent's generation-tagged
  target lease. Windows proxy and disconnected desktop probes use ordinary
  host networking with a logical generation-zero lease, not Agent/WFP
  protection. Bootstrap never performs a hostname lookup.
- A bounded 100 ms generation observer cancels the old pool; every query also
  checks generation synchronously at its boundaries. New-generation requests
  cannot reuse old connections. Profile shutdown rejects new work and cancels
  old work. Queued application replies recheck generation before injection.

## No plaintext downgrade

Only the physical-system variant can discover physical DNS servers or use
plain UDP/TCP DNS. An encrypted resolver error returns a fixed error and Split
DNS replies SERVFAIL. The encrypted branch cannot change protocols, resolve a
bootstrap name with system DNS, or fall back to port 53.

The same runtime-owned pool serves Geo-selected HTTP/SOCKS proxy hostnames.
An encrypted resolution error is terminal. If resolution succeeded but a
direct data socket failed, the existing tunnel fallback reuses those resolved
IP addresses; it does not re-query the hostname through a system/configured
plaintext resolver. Physical-system routing keeps its existing fallback.

Disabling the internal encrypted-DNS capability rejects an encrypted Profile
before connection. It never rewrites the saved mode or silently substitutes
physical-system DNS. Users may explicitly change the Profile themselves.

## Privacy and validation limits

### Profile/config schema 13

`AppConfig.shared_network.direct_dns` is hydrated into each account's runtime
Profile. Old schema-12 configurations and missing protobuf Profile field 17
canonicalize to System. Shared settings, not per-account endpoint overlays,
select DNS. `DirectDnsSettings` wire fields 1–5 are mode, server name, DoH path,
bootstrap IPs, and port; unknown protobuf fields retain compatibility. Android
uses the equivalent `direct_dns` JSON object. Core validation remains
authoritative after the editor's validation. Explicit configuration export
preserves these user values; diagnostic export excludes them.

Android waits for a usable non-VPN network in every mode, but only System
Split DNS requires its physical DNS metadata. DoH/DoT bootstrap is independent
of that list. See the deployment-specific [threat model](direct-dns-threat-model.md)
and [capability rollback](network-quality-rollback.md).

Metrics contain only protocol/mode, fixed phase/reason codes, RTT, counters and
queue pressure. No QNAME, wire message, configured name, bootstrap/answer IP,
certificate or physical DNS server is added to logs, metrics or diagnostics.
Geo fallback logs no longer include raw target/error text. Metrics stay local.

Workstation tests use in-memory test CA material and loopback fake DoH/DoT
servers. They cover protocol/PKI rejection, concurrency, reuse/recycle/idle
cleanup, generation races, cancellation, exact leases, bootstrap retry,
application SERVFAIL, proxy fallback and parser/media-type property fuzzing.
The fake protector panics if encrypted code calls physical DNS discovery or
hostname resolution. Existing system-mode truncation/oracle fixtures remain.

These tests are not external leak proof. Actual device/adapter binding,
observer packet counts and controlled performance evidence require the
protected environments in `AGENTS.md`. Unavailable runs are `not_run`, never
pass, and do not become publication prerequisites.
