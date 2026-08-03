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
