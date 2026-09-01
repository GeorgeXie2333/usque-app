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
- `ConnectionMetrics` quality fields occupy 13 through 62
- `ConnectionEventType` migration, PMTU, and direct-DNS values occupy 22
  through 30

The build advertises `network_quality=true`. The encrypted-DNS, migration, and
automatic-PMTU capability flags remain false until their implementation PRs.
Capabilities describe build/platform support, not current path readiness.

## Compatibility and availability

Rust/prost and the hand-written Dart codec share a checked-in byte fixture.
Legacy decoders ignore field 21 on a new response. New decoders accept old
responses and profiles with no quality/direct-DNS fields. New enum decoders map
unknown values to `unknown`; unknown fields are skipped. A `known=false` flat
metric is represented as `null` in Dart even if bytes contain a numeric value.
Detailed messages retain explicit `Available`, `Unsupported`, `NotReady`, or
`Stale` availability.

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
