# macOS VPN Gate and packet TUN follow-up

Validation date: 2026-10-30. Source baseline: `e098f130`.

This change fixes catalogue error reporting and packet-session isolation. It
does not enable production system VPN, publish replacement DMGs, or establish
successful connections to public VPN Gate volunteers.

## VPN Gate in the development build

Use proxy mode with chain proxy disabled to connect WARP first. Then refresh
the VPN Gate list, choose a TCP node, and apply the chain configuration. Public
servers can disappear or refuse connections; source TCP reachability is not
proof of an authenticated OpenVPN session.

macOS catalogue requests require an existing connected WARP underlay. Cached
profiles remain usable offline. Refresh previously replaced the actionable
`MACOS_WARP_CONNECTION_REQUIRED` response with a generic download error. The
editor now displays the translated, sanitized prerequisite and clears its
ownership of the rejected refresh. The regression checks that subsequent
polling does not erase the message or expose backend text.

## Packet TUN corrections

Native pending reads and backpressured packets retain the exact session UUID,
configuration revision, and physical-network generation. Delayed callbacks and
pending batches from a previous identity are discarded. The additive Rust C
entry point also checks that captured identity before enqueueing; the queued
packet retains that identity even if a replacement begins immediately afterward.

Pending packets are bounded by count, individual length, and total bytes. Only
one packet read can be outstanding. Fatal packet IO or lease-renewal failures
stop the pump and timer and rearm independent protection before cancelling the
tunnel. Runtime destruction waits for an in-flight control request to finish.

## Validation

- Flutter: 26 targeted VPN Gate/error tests passed; the final text update also
  passed the 12 directory/error tests. `flutter analyze --no-pub` passed.
- Engine: all 43 macOS tests passed with local test sockets permitted.
- macOS Rust runtime: 20 tests passed, including delayed-packet admission,
  replacement, bounded queues, and credential handoff. Strict Clippy passed.
- VPN Gate core: 32 tests passed, including mirror fallback, hash validation,
  cancellation, saved selections, and favorites.
- Embedded OpenVPN: all 15 native interoperability/unit tests passed, including
  memory TUN, authentication refusal, saturated queues, and renegotiation.
- Native isolated contracts passed, including revision/generation changes,
  stale read callbacks, disconnect, and packet memory limits.
- Apple Silicon: runtime archive and all four providers/control helper built
  and linked for macOS 13+. Both Apple Silicon and Intel native typechecks
  target macOS 13+. Intel execution is unverified.

The restricted sandbox initially refused test sockets; the same isolated tests
passed when local sockets were permitted. No providers were installed or
activated, no routes/DNS were changed, and no real account credentials were used.

## Remaining system VPN prerequisite

The current DMGs are ad-hoc development builds. This architecture requires a
publisher-signed Network Extension distribution with matching entitlements and
provisioning profiles. See [Apple's Network Extension entitlement documentation](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.networking.networkextension).
The user does not have to own the publisher's certificate to install a properly
signed distribution.

Signing alone is insufficient: the production gate remains closed until signed
activation, routing, DNS, independent protection, crash recovery, and external
leak observations pass the existing acceptance criteria. The separate signed
prototype can collect that evidence; this patch does not bypass those gates.
See [the network prototype record](MACOS_NETWORK_PROTOTYPE.md) and
[development packaging](MACOS_DEVELOPMENT_PACKAGING.md).
