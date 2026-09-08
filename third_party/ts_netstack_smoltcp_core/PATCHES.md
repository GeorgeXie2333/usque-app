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
