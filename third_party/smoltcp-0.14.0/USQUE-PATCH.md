# Local TCP packet budget patch

Source: smoltcp 0.14.0, published crates.io archive.
Archive SHA-256: `b6f8b28ad56c6e35524a37dd492af5d1a47e31e1a4d175cd12f89c075f01980f`.
Upstream: <https://github.com/smoltcp-rs/smoltcp>.
License: [0BSD](LICENSE-0BSD.txt).

Two unchanged archive lines have trailing spaces (the 6lowpan example comment
and the fixed-width netsim snapshot); the license ends with a blank line.
Exact-path Git whitespace attributes preserve those source bytes;
first-party code and the TCP patch keep their
normal whitespace/format checks.

The source archive is retained without an upgrade. The only Rust source change
is in `src/socket/tcp.rs`: an optional local transmit IP packet limit, disabled
by default. It limits the local MSS calculation for send readiness, ordinary
transmission, retransmission and new SYN MSS advertisements. Negotiated remote
MSS, interface MTU, UDP/raw traffic and TCP connection state are unchanged.
TCP options still consume the effective packet budget. Invalid bounds are
rejected before mutation; the setting survives socket reset/connect like other
socket options. The MASQUE wrapper additionally enforces IPv6's 1280-byte floor.

Tests cover unacknowledged-data retransmission after a reduction, timestamps,
fast retransmission, validation, restoring the interface bound and SYN MSS.
The standalone Cargo.lock pins test dependencies; the workspace and wrapper
lockfiles select this same source. Replace this patch when an upstream API can
provide equivalent behavior and the integration regressions pass.
