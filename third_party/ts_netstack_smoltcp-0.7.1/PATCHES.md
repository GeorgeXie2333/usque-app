# Usque's pinned ts_netstack_smoltcp 0.7.1 patch

The complete published 0.7.1 crate is retained before applying the local patches.
Archive SHA-256:
`6680a0ee790ac59b012a96386855b4a1770ffd80fce1d666bdb7737419025659`.
Published VCS revision: `a2a56c41b1db72d50d59d71d35c6669ff9639d14`.
Upstream consolidated the former core/socket crates into `netcore` and `netsock`
modules; the workspace now patches this one crate. Local core changes were
three-way rebased from the checksum-verified 0.4.0 core archive.
Two unchanged README lines retain upstream trailing spaces through exact-path
Git whitespace attributes; no crate-wide formatting exemption is added.

Upstream's half-closed TCP accept and configurable request cancellation fixes
are retained. `complete_if_channel_dropped` is false for ordinary operations
and true for nonblocking cleanup and cancellation wakes. Local creation
ownership still reclaims responses abandoned after allocation or delivery.
Both blocked-command pumping and cancellation reaping respect the upstream
completion flag; a creation with no owner is reclaimed even if that flag is set.

The private MASQUE stack can opt into `SetTcpPacketBudget`, a revisioned control
command that sets separate IPv4/IPv6 TCP IP packet bounds. It validates both
families before mutation, ignores old revisions and updates existing connected
TCP sockets and future outbound connects. Default stacks retain no additional
limit. IPv6 limits below 1280 are rejected. No UDP/raw or interface MTU changes
are made. The command uses the narrow
[smoltcp patch](../smoltcp-0.14.0/USQUE-PATCH.md); this manifest also patches the
dependency for standalone tests so they cannot silently use registry smoltcp.

Source: `ts_netstack_smoltcp 0.7.1` from crates.io
Upstream: <https://github.com/tailscale/tailscale-rs>
License: BSD-3-Clause
The published 0.7.1 archive omits the license text. `LICENSE` retains the
Tailscale copyright notice and BSD-3-Clause terms from the previous core vendor.

The runtime and test smoltcp dependencies remain pinned to `=0.14.0`, and
the standalone manifest selects the same local smoltcp patch as the workspace.
Its `std` feature forwards to `smoltcp/std`, and `socket-tcp-cubic` remains
enabled. This preserves Usque's existing CUBIC behavior after upstream 0.7.1
removed that feature from its defaults. No physical device or `tun` feature is
enabled by the application or the standalone checks.

The published tests/examples still refer to the workspace-private `ts_cli_util`.
Their standalone harness uses a standard boxed error and omits that unavailable
tracing initializer. Packet exchange, half-close and cancellation assertions
are unchanged. The library, TCP/UDP integration and IP-family tests run with:

```shell
cargo test --manifest-path third_party/ts_netstack_smoltcp-0.7.1/Cargo.toml --locked --lib --tests --features std,tokio
```

This keeps the former core test coverage and adds the merged socket regressions.
It does not execute the newly included throughput benchmarks or TUN examples.
Remove local patches only when an upstream release satisfies the same ownership,
buffer-budget, packet-budget and saturated-cleanup regressions.

The opt-in L4 TUN adapter also requires bounded listener allocations,
single-accept listeners without spare sockets, and an explicit TCP abort
command. These use the existing per-stack TCP buffer accounting and keep
unselected listener behavior unchanged. Stale queued TCP commands and duplicate
close requests return an error rather than dereferencing a removed handle.
No network protocol dependency or platform mutation is added.

One-shot listener ownership transfers to the accepted TUN stream. A closed
socket slot is not recycled until that unique listener token is released;
late stream cleanup therefore cannot abort a different socket that reused its
index. Cancelled response delivery also reclaims the allocated listener/socket.

L4 cleanup uses the additive `try_request_nonblocking` entry point. It reports
queue saturation as `TryRequestError::Full`, distinct from a closed stack;
the legacy best-effort entry point remains source-compatible. This lets the
single-owner L4 wrapper enqueue a bounded asynchronous cleanup retry instead
of silently losing Close when the command queue is full. A regression checks
both a one-slot queue and the production 256-slot queue.

Closing a one-shot listener immediately drains already-closed socket allocations
after releasing the unique listener owner. Reclamation no longer waits for an
unrelated packet to make the stack report I/O progress; live or still-owned
sockets retain the existing close/ownership checks.

Usque's first-party packet device reserves every TX queue slot before giving smoltcp
a token, avoiding that crate's blocking bounded-pipe send implementation.

Socket creation cancellation also covers TCP connect and UDP bind, including
the interval after a response is queued and before its handle is received.
The core retains the response sender until the response is consumed. The
locked flume implementation leaves an unconsumed message queued when its last
receiver drops; tests cover both queued cancellation and successful handoff.
Disconnected connect requests abort their pending socket and immediately free
its TCP reservation. Already cancelled creation commands allocate nothing.
Async creation cancellation disconnects the reply and submits a nonblocking
`ReapCancelled` wake. A full command queue already wakes the actor, whose next
I/O pass performs the same cleanup. No packet or external interface changes.

Raw socket creation now participates in the same cancellation ownership tracking
as UDP bind and TCP connect. Unclaimed IPv6 fragment sockets are reclaimed before
or after response delivery, including a saturated command queue. First-party
UDP and raw wrappers own their handles exclusively and retry Close through the
existing bounded cleanup path; the upstream best-effort socket Drop is not used
for final DNS or chained protocol transport.

An additive `BindWithReceiveBuffer` UDP command allows a bounded receive-only
override (at most 128 KiB and 512 metadata entries). Ordinary binds and all
transmit buffers retain the configured sizes. Only Usque's private VPN-protocol
underlay selects this override; DNS and SOCKS UDP associations retain their
existing allocation policy. A 64-packet burst of 1248-byte payloads previously
delivered only 52 packets through the 64 KiB receive ring. The first-party memory
regression also covers 300 ACK-sized packets, eight 16000-byte datagrams, packet
order, and an abandoned public receive waiter.
The new bind participates in the same cancelled-creation reclamation as the
original bind. Invalid dimensions are rejected before allocation. This changes
bounded in-memory buffering, with no physical socket, routing or platform change.
