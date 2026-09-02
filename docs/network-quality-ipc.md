# Network quality IPC contract

Schema 13 adds the process-local network quality path without changing or
reusing any established protobuf tag or enum value. Older profiles normalize
to canonical physical-system direct DNS, preserving schema-12 behavior.

## Append-only fields

- `Profile.direct_dns = 17`
- `Capabilities.network_quality = 20`, `encrypted_direct_dns = 21`,
  `quic_migration = 22`, and `automatic_pmtu = 23`
- `ConnectionSnapshot.network_quality = 17`
- `ControlRequest.get_network_quality = 40`
- `ControlResponse.network_quality = 21`
- `EventEnvelope.network_quality_updated = 23`
- `ConnectionMetrics` quality fields occupy 13 through 63; tag 63 is the
  bounded H3 `pmtu_send_too_large_count`
- `PmtuQuality.send_too_large_count = 8`
- `ConnectionEventType` migration, PMTU, and direct-DNS values occupy 22
  through 30

The build advertises `network_quality=true`, `automatic_pmtu=true`,
`quic_migration=true`, and `encrypted_direct_dns=true`. Encrypted DNS is used
only when explicitly selected in the Profile; its runtime/trust/bounds
contract is in [encrypted-direct-dns.md](encrypted-direct-dns.md).
The H3 migration and multi-socket/CID contract is described in
[h3-path-infrastructure.md](h3-path-infrastructure.md). A specific connection
can still report migration unavailable because of family, generation, or CID
constraints and safely use complete reconnect.
Capabilities describe build/platform support, not current path readiness; H2
reports PMTU as `Unsupported`, while an H3 path still probing reports
`NotReady`.

## Compatibility and availability

Rust/prost and the hand-written Dart codec share a checked-in byte fixture.
Legacy decoders ignore field 21 on a new response. New decoders accept old
responses and profiles with no quality/direct-DNS fields. New enum decoders map
unknown values to `unknown`; unknown fields are skipped. A `known=false` flat
metric is represented as `null` in Dart even if bytes contain a numeric value.
Detailed messages retain explicit `Available`, `Unsupported`, `NotReady`, or
`Stale` availability.

The privileged Agent protocol remains version 3 and adds four independent
append-only fields for exact-generation Windows egress:
`AgentCapabilities.exact_generation_egress = 12`,
`PhysicalInterface.address_family_mask = 4`,
`AcquireDirectEgressRequest.expected_generation = 4`, and
`DirectEgressLease.network_generation = 5`. The new Engine requires the
capability before VPN startup. A nonzero expected generation must match;
`AGENT_STALE_GENERATION` is the fixed retryable error. Zero preserves legacy
request decoding, but the new Engine never uses it. Wire snapshots verify all
four tags without reusing old numbers. These privileged metadata fields are
not copied into network-quality events or diagnostic exports.

The connection instance identifier is a fresh UUIDv4 for the process-local
connection attempt. It is not a QUIC CID and is not persisted.

## Event coalescing

The transport publishes only its latest snapshot through a Tokio watch
channel. The Engine relay and each GUI event stream also retain only one latest
pending snapshot, so a slow client cannot create an event backlog.

Identical quality snapshots are not emitted. Ordinary changes are emitted no
more than once per second. A migration phase change, PMTU change, direct-DNS
degraded/recovered transition, or first queue drop is eligible immediately
when the same one-second output window is available; that send resets the next
periodic window.

## Privacy

Quality conversion emits only numeric values, enums, allowlisted phase/reason
codes, timestamps, and the random connection-instance UUID. It never copies
endpoint addresses, QNAMEs, configured bootstrap IPs, server names, SSID/BSSID,
QUIC source/destination CIDs, tokens, payloads, or raw error text into a quality
snapshot or event. Direct-DNS configuration itself remains visible only in the
explicit Profile configuration message where it is required for editing.
