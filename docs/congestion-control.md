# HTTP/3 congestion control

Advanced settings exposes the device-wide `cubic`, `reno`, `bbr` and `bbr3`
selection. CUBIC remains the default. `bbr` uses quiche's existing BBRv2;
`bbr3` is an independent, experimental BBRv3 implementation. This is a sender
setting for the client's HTTP/3 MASQUE connection, not a request to change
Cloudflare's sender, system TCP, proxy listeners, or application TCP stacks.
HTTP/2 remains controlled by the operating system.

The selector sits below SNI. Its display order is `cubic`, `BBRv2`, `BBRv3`,
`reno`; persisted values remain `cubic`, `bbr`, `bbr3`, `reno`. The interface
keeps only a short activation hint and pending state; algorithm details
belong in this document rather than the settings form.

## Saved preference versus session

Saving only the algorithm persists a preference without reconnecting. A manual
connect or retry captures the latest saved value. A fresh engine connection
also reads saved settings; Android restoration of an existing session uses
its confirmed recovery profile. The current session retains its captured value through automatic
reconnection, QUIC migration, H3/H2 switching, hot frontend updates and internal
reconnections caused by other settings. Merely reopening the GUI is not a new
engine session. An active H2 connection reports that H3 control is not applied.

The selector shows the saved preference or unsaved draft, not a claim about the
live connection. A mismatch with the internal session snapshot is pending until
the next user session; the running algorithm is not displayed in settings.
Returning the saved value to the captured value removes the pending indicator.
Unsaved edits remain a separate
draft; storage failure leaves that draft editable and displays the failure.

Windows binds the value under the existing mutation lock and retains it outside
the transient data-plane object. Android keeps desired and effective recovery
profiles separate. A manually started Android session consults the Rust profile
catalog. Process restoration retains the confirmed session preference and
does not activate settings saved for a later manual connection. See the
[network settings contract](NETWORK_SETTINGS.md) for mixed edits and failures.
No configuration operation changes the congestion algorithm of a live socket.

## Configuration and wire compatibility

Schema 14 adds `network.congestion_control` and the corresponding runtime
Profile field. Missing fields in older configurations default to `cubic`;
unknown or malformed explicit values are rejected. Resetting advanced settings
selects CUBIC in the draft before normal saving.

The protobuf enum is append-only: unspecified 0, CUBIC 1, Reno 2, BBR 3, BBR3 4.
`Profile.congestion_control` is field 18;
`ConnectionSnapshot.session_congestion_control` is field 18;
`Capabilities.h3_congestion_control_algorithms` is field 24. A missing status
value is unknown, not an assertion that CUBIC is running. An old engine that
does not advertise support cannot enable the selector. Android JSON uses the
same lowercase names. No existing field numbers or wire types change.

## BBRv3 basis and QUIC adaptations

The fixed reference is
[draft-ietf-ccwg-bbr-06](https://www.ietf.org/archive/id/draft-ietf-ccwg-bbr-06.txt),
published 6 July 2026. This remains an Internet-Draft, not a finalized RFC.
The independent implementation includes Startup, Drain, ProbeBW DOWN/CRUISE/
REFILL/UP, ProbeRTT, short/long-term congestion bounds, precautionary probing,
ACK aggregation, app-limited sampling and conservative spurious-loss undo.
Source and license provenance remain with the
[pinned dependency](../third_party/quiche-0.29.3/USQUE-PATCH.md).

QUIC byte accounting includes congestion-controlled packet bytes. Send quantum
and offload budget use the draft's QUIC rules; actual monotonic send timestamps
are used, not a future pacing deadline. PMTU changes do not invent bandwidth
samples. QUIC PTO is not TCP RTO: it schedules probes without collapsing cwnd.
Undo requires acknowledgement of every recorded loss in the recovery episode;
evidence is limited to 4096 packet numbers and overflow disables undo rather
than falsely concluding a recovery was spurious. No extra BBR tuning is exposed.

## Validation and safety

Tests cover configuration migration, exact names, protocol packing and defaults,
session-preserving saves, Android recovery binding, pending/draft UI states,
and algorithm transitions based on the draft's Appendix A scenarios. Real QUIC
buffers are exchanged in memory for all four algorithms. Existing CUBIC/Reno/
BBRv2 tests remain in the independent vendored suite, executed explicitly in CI.

Use the complete applicable matrix in [CONTRIBUTING](../CONTRIBUTING.md),
including Windows helper-based Rust checks/build, locked Flutter checks/build,
Android Rust and Kotlin checks, protobuf checks and workflow validation.
Deterministic tests do not establish real-network throughput or fairness.
Performance, VPN lifecycle and leak observations require the appropriate
isolated environments; missing runs are `not_run`, never passing evidence.
Protected-runner evidence remains supplemental and does not gate publication.

The change adds no privileged networking operation, system-proxy mutation,
TLS bypass, credential field, diagnostic upload or telemetry. Existing cleanup,
generation ownership, certificate pinning, queue limits and fallback safety
rules remain in force. No installers or release APKs are installed for testing.
