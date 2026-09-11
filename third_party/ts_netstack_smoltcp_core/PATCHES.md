# Local patch record

Source: `ts_netstack_smoltcp_core 0.4.0` from crates.io
Upstream: <https://github.com/tailscale/tailscale-rs>
License: BSD-3-Clause

Usque carries one behavior fix in `src/lib.rs`:

- Before replaying a command previously returned as `WouldBlock`, discard it
  when its one-shot response receiver has disconnected. Async timeout
  cancellation otherwise leaves a stale UDP receive in the blocked queue; a
  subsequent socket close removes the handle and replay panics inside smoltcp.

The patch must be removed in favor of an upstream release once an equivalent
fix is published and the DNS timeout regression test passes against it.

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

The outer `ts_netstack_smoltcp` crate is not patched or replaced. Usque's
first-party packet device reserves every TX queue slot before giving smoltcp
a token, avoiding that crate's blocking bounded-pipe send implementation.
