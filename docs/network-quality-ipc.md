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
- `ConnectionMetrics` quality fields occupy 13 through 66; tag 63 is the
  bounded H3 `pmtu_send_too_large_count`. Tags 64, 65, and 66 are respectively
  `latest_rtt_ms`, `latest_rtt_known`, and `latest_rtt_availability`. They do
  not reinterpret the original smoothed RTT fields 4 and 5.
- `PmtuQuality.send_too_large_count = 8`
- `NetworkQualitySnapshot.samples = 9` carries at most 16 source observations.
  `NetworkQualitySample` tags 1-7 are sequence, UTC sampling timestamp,
  connection-local monotonic milliseconds, optional cumulative downloaded and
  uploaded bytes, optional available RTT milliseconds, and optional available
  interval loss basis points. Optional zero is a measurement; absence is not.
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

A closed runtime quality source publishes a disconnected snapshot. Replacing
or clearing a source joins its canceled relay before publishing the next
source, so an old relay cannot overwrite a new connection's readings.
With the internal quality build capability disabled, state payloads omit the
quality field, the event stream emits no quality events, and GetNetworkQuality
returns the existing structured invalid-request response instead of a quality
payload. Protobuf field numbers remain reserved and unchanged.

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
Each latest snapshot includes the source's bounded 16-observation ring, so
independent 1 Hz readers can coalesce deliveries without discarding the samples
between them. The ring contains no synthetic catch-up samples. Sequence and
monotonic origin reset with the connection-instance UUID. Counters are read by
the same transport sampler that captures RTT/loss, not by the GUI or JNI reader.

Identical quality snapshots (including their sample ring) are not emitted.
New observations with unchanged numeric values are still distinct. Ordinary
changes are emitted no more than once per second. A migration phase change, PMTU change, direct-DNS
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

## GUI read model and Android bridge

The Quality navigation entry requires the optional build capability. Missing
capabilities leave old connection controls intact. The process-local controller
retains at most 300 one-second points and displays an aligned 60-second window.
Windows and Android share this history with both Home layouts. New engines send
source observations with monotonic timestamps and sequence numbers. The GUI
deduplicates the ring and computes byte rates from those source counters/times,
including when delivered by a quality-only event. Cached state counters and
repaint timers never create source observations. Consecutive sequence numbers,
valid monotonic intervals and nondecreasing counters are required for a rate.
The display grid is separate from the rate calculation and follows the source
monotonic origin. Absent beats are not interpolated. The right edge shows the
latest complete frame while the next delivery is in flight, then advances when
delivery stops. Pause freezes the exact displayed window, including under delay.
Old engines without the sample ring retain full-state-only counter observation;
its receipt clock has its own phase, independent of RTT/source delivery latency.
Counter rates use actual source time (receipt time for legacy engines);
window averages divide total byte deltas by total elapsed time across consecutive
valid intervals rather than averaging unequal-duration rates. Counter resets,
clock rollback, pause, reconnecting state, or stream outages break the rate baseline.
New connection IDs reset both history and byte-counter baselines. Paused,
missing and stale samples stay gaps; closing the app does not persist history.
At most one refresh is outstanding, and a late reply cannot restore a previous
connection after disconnect. Timestamps are range checked and queues capped at
eight in both codecs. Latest/smoothed/minimum RTT are distinct measurements.
H2 shows protocol PING RTT and receive-window stalls, but no packet-loss or PMTU
measurement. PMTU limits use exact bytes, not rounded throughput units.
Full-state fallback polling is also single-flight. A reply predating a newer
event/control state is discarded. Fresh fallback data can resume chart observations
without clearing the app's separate event-pipe-degraded indication. Android polls
on fixed monotonic deadlines with bounded early-tick tolerance and skips catch-up
work; Windows retains its existing fixed-rate, missed-tick-skipping event stream.

Android forwards a maximum 16 KiB allowlisted quality JSON object across the
Messenger/MethodChannel boundary. It retains only fixed numeric fields, eight
known queues, at most 16 numeric source samples, phase/reason enums, and a valid
UUIDv4; unknown or malformed values do not become fake zero measurements. Native capability responses are real
build flags, with safe missing-method behavior for an older JNI library.

The custom direct DNS editor has no provider presets or TLS bypass switch.
Profile validation remains authoritative in the Engine; failed saves restore
the catalog without switching an encrypted profile to plaintext. If the
encrypted capability is disabled, custom values remain read-only, and only an
explicit user selection can switch to physical-system DNS.

See [network-doctor.md](network-doctor.md) for Standard and Deep diagnostics.
