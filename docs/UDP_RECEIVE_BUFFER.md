# QUIC UDP receive-buffer policy

Normal Windows and Android builds target **2 MiB** for each outer QUIC socket's
receive buffer. This applies to CONNECT-IP H3 (including the H3 leg of Auto) and
L4, whether the output is VPN/TUN, SOCKS5 or HTTP. H2 uses system TCP and does not
enter this path. Other source platforms retain their OS defaults.

The policy is applied in common socket preparation, including replacement and
migration-candidate sockets. It does not change socket protection, exact endpoint
authorization, network-generation ownership or the migration send barrier.
L4 remains opt-in; Auto still excludes it. Identity/SNI, pins, DNS, UDP blocking
and Kill Switch behavior are unchanged.

## Bounded, best-effort request

- Read the existing `SO_RCVBUF`. If it is already at least the target under the
  platform's accounting, keep it; do not shrink an already-sufficient buffer.
- Otherwise request 2097152 bytes with ordinary `SO_RCVBUF`, then read back the
  actual receive **and** send sizes. Rejection is observable and does not abort
  the connection. An accepted call may still be clamped by the OS.
- Android/Linux raw readback accounts for twice the requested value; a 2 MiB
  request can therefore report 4 MiB. Windows readback is not doubled by this
  policy. The stored values are raw OS readings, not normalized estimates.
  See the [Linux socket API](https://man7.org/linux/man-pages/man7/socket.7.html)
  and [Winsock options](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-setsockopt).
- No `SO_RCVBUFFORCE`, global sysctl, privilege escalation, runtime environment
  override, persisted setting or UI control is introduced. Configuration version
  remains 15. Normal builds need no experiment flag.

This changes an OS socket buffer, not the QUIC flow-control window, application
buffer budget or TUN MTU. The send buffer, default MTU 1280, QUIC windows, TCP
tiers, queues, workers and pacing stay unchanged. The target is per socket, not
a whole-process RSS limit or additional application buffer allocation. Existing
session/path-count limits still apply.

## Diagnostics and compatibility

Android exports `network-quality.json` → `udp_socket_receive` for active H3.
Windows exports `udp-receive.json` when an active H3 socket observation is
available. L4 keeps its existing `l4.performance` buffer sizes and `receive`
observation. Collection is bounded and sampled with existing maintenance; it
adds no packet-content logging, socket identifiers or automatic upload.

| Field | Meaning |
| --- | --- |
| `buffer_target_bytes` | Policy target; absent in OS-default test observations or older producers |
| `requested_buffer_bytes` | Argument actually passed to the setter; absent if no call was needed |
| `buffer_request_status` | `accepted`, `rejected`, `already_sufficient` or `not_requested` |
| `receive_buffer_bytes` / `send_buffer_bytes` | Actual raw reads, independently nullable; L4 uses its existing `udp_*_buffer_bytes` names |
| `socket_drops_reported` | Socket-local kernel report, unknown until observed; not end-to-end loss |

`already_sufficient` has a target but no new request. Failed reads remain unknown,
not zero. Receive/send backend selection is unchanged. Windows uses the existing
portable receiver and does not claim Linux overflow-counter support.

Wire additions are append-only: `NetworkQualitySnapshot.udp_socket_receive` is
field 10; `L4ReceiveSnapshot.buffer_target_bytes` is field 14. Older consumers
ignore them and newer consumers retain unknowns when older producers omit them.
Kotlin, Dart and diagnostic exports filter status strings through allowlists.
Connection/attempt replacement and shutdown clear stale socket observations.

The previous Android build-only features and their Gradle/PowerShell selectors
have been removed. OS-default versus requested-buffer comparisons remain in
loopback tests and the existing internal L4 test options, not in shipping APK
configuration. Native build information explicitly reports
`network_experiment=none`; the Kotlin allowlist still reads earlier comparison
markers. Historical outcomes and candidate boundaries are consolidated in the
[receive experiment record](RECEIVE_BUFFER_EXPERIMENTS.md).

The Android L4 JSON boundary is 96 KiB of UTF-8, not 98304 UTF-16 code units.
It rejects oversized text before JSON parsing, including multibyte and surrogate
pair inputs. Native build-info JSON has the same byte check at its existing
1 KiB limit. Receive history remains bounded at 120 entries.

## Evidence and limits

The user reported Android L4 B download at 892.9 Mbps and Android H3 B at
725.9 Mbps, versus about 500 Mbps for default H3. These are user-provided
observations, not seven-run lab results or a universal performance guarantee.

The connected H3 B diagnostic received on 2026-09-10 confirms a release arm64
native build, an accepted 2097152-byte request, 4194304-byte raw receive capacity,
229376-byte send capacity, `recvmmsg` / `sendmmsg`, and no reconnection. It still
reports socket drops and transport-to-TUN queue drops. Increasing the socket
buffer does not establish that the entire downstream path is loss-free. Queue
tuning is separate work and is not bundled into this default change.

Raw diagnostic archives, device identifiers, addresses and screenshot artifacts
are not checked in. Windows throughput with the new policy, repeated controlled
Android throughput, platform lifecycle and externally observed leak validation
remain `not_run` on this workstation. The change adds no publication gate.

## Validation records

The [historical experiment record](RECEIVE_BUFFER_EXPERIMENTS.md) retains the
Android A/B observations and
[2026-09-10 production-promotion checks](RECEIVE_BUFFER_EXPERIMENTS.md#production-promotion-validation-2026-09-10).
Those results apply to the recorded working-tree candidate, not to every later
build. For a new change, run the applicable
[Contributing checks](../CONTRIBUTING.md#required-checks-by-change-scope).
