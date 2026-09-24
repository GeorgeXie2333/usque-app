# Usque's pinned quiche 0.29.3 patch

This directory starts from the complete published `quiche` 0.29.3 crate,
not a floating branch or an upgrade. Original license and public example/test
fixtures are retained. See [COPYING](COPYING) (BSD-2-Clause) and
[COPYING-BBR3](COPYING-BBR3) (Revised BSD for the added BBRv3 implementation).

## Provenance

- Registry archive: `quiche-0.29.3.crate` from crates.io.
- Archive SHA-256:
  `61166d27591eb7cb1310eec2b8fc6ae0e0686e9e4ed742a3ffc6317171175e7d`.
- Published VCS revision:
  `09b125d4cfc16e78d73d8382c93926f3aba063d4`
  (also recorded in `.cargo_vcs_info.json`).
- All 87 published files were verified byte-for-byte against that archive
  before patching. The retained PMTU changes are in `src/lib.rs` and
  `src/path.rs`. The congestion-control additions are confined to `recovery/`,
  its public C algorithm enum, and license/package metadata. The follow-up
  DATAGRAM window classification changes BBRv2/BBRv3 as described below;
  Reno and CUBIC algorithm implementations remain unchanged.
- The root `[patch.crates-io]` selects this directory. The workspace lockfile
  changes only quiche's source/checksum entry; dependency versions and other
  lockfile edges are unchanged. The vendored crate is excluded from workspace
  formatting and workspace membership to preserve upstream source formatting.

## Changes

1. Retain the effective PMTUD enablement and probe-attempt budget, including
   an accepted TLS-handshake override. Both client-created and server-observed
   runtime paths receive independent, fresh PMTUD state. Their probe ceiling
   is bounded by the configured send ceiling and the local/peer UDP limits;
   they do not copy another path's measured MTU.
2. Require QUIC path validation before PMTU probe sizing/emission and consult
   the actual `send_pid` when emitting a PMTU probe. A response on a candidate
   path must not consume the old active path's pending probe. Pending
   PATH_RESPONSE/PATH_CHALLENGE frames take priority even after local path
   validation, so a full-sized probe cannot displace the peer's response.
3. Bound `dgram_max_writable_len()` by the active path's current ordinary-send
   PMTU, not its larger probe allowance. Recompute the queued DATAGRAM bound
   after processing losses. The existing too-large-queue-entry discard then
   prevents an old oversized head from blocking smaller DATAGRAMs after
   revalidation. The existing ordinary packetization cap is retained.
4. Notify BBR of application-limited sending only when the DATAGRAM queue is
   empty. A queued DATAGRAM that cannot fit the remaining congestion window
   or output buffer is still application backlog. Marking that condition as
   application-limited can suppress bandwidth samples. The packetizer check
   changes neither BBR's algorithm nor its parameters. Four in-memory cases
   cover BBRv2/BBRv3 with a partly consumed window or a short output buffer,
   and verify that an empty queue still sends the notification. A test-only
   counter observes these notifications independently of the existing
   test-only `app_limited()` cwnd heuristic.
5. Accumulate both recovery backends' per-path lost bytes when loss is declared.
   All four algorithms previously exposed a permanently zero path counter even
   though the connection's loss counter increased. The existing DATAGRAM loss test
   now covers all four algorithms and compares path and connection counters.
   PMTU probe bytes retain their exclusion from congestion loss accounting.
6. Count a remaining congestion-window space smaller than the current path's
   maximum datagram size as window-limited in BBRv2/BBRv3 model growth and
   application-limited classification. DATAGRAMs cannot be split to fill that
   tail; requiring byte-exact occupancy could prevent BBRv2 PROBE_UP from
   raising `inflight_hi` after small-packet traffic. BBRv2 ACK aggregation
   uses the same predicate, and BBRv3 records it for its probing round.
   Exactly one full packet of free space remains non-limiting. This changes
   classification only: packetization still enforces the original byte window,
   send quantum and pacing deadlines, with no extra packet allowance. Tests
   cover both sides of the boundary and a changed path MSS.

Related upstream work, inspected on 2026-09-03:
[runtime-path PMTUD PR #2573](https://github.com/cloudflare/quiche/pull/2573)
(open/unmerged, head `cc07864532f1d6232fd4d063ef08cf920a9070bd`) and
[send-path PMTUD PR #2566](https://github.com/cloudflare/quiche/pull/2566).
The retained enablement/budget and validation gating follow the same approach
as #2573; this local patch also bounds new-path ceilings and repairs DATAGRAM
admission. These links are context, not build-time dependencies or a claim
that upstream has merged the fixes.

## Regression coverage and removal

The independent `recovery/gcongestion/bbr3.rs` state machine follows
[draft-ietf-ccwg-bbr-06](https://www.ietf.org/archive/id/draft-ietf-ccwg-bbr-06.txt)
(6 July 2026), sections 4 and 5. It reuses the existing delivery sampler, not
BBRv2's state machine or tuning parameters. Its code and test adaptations are
described in [the application contract](../../docs/congestion-control.md).
The closed `BbrSender` enum routes `bbr3` to the new sender. BBRv2 retains its
state machine and gains, with the DATAGRAM window classification above.
Public algorithm value 5 is appended;
removed values 2 and 3 are not reused. `bbr` still names BBRv2.

The source includes a bounded virtual FIFO-link test harness with application
limiting, ACK aggregation, injected loss, bandwidth changes and a token-bucket
policer. Transport tests additionally exercise real pinned-TLS QUIC packets
with all four algorithms, including DATAGRAM, PMTU, migration and closure.

CI explicitly runs the standalone crate's locked unit suite on Linux, plus
BBRv3 tests with qlog enabled. Workspace tests alone do not run that suite:

```shell
cargo test --manifest-path third_party/quiche-0.29.3/Cargo.toml --locked --lib
cargo test --manifest-path third_party/quiche-0.29.3/Cargo.toml --locked --lib --features qlog recovery::gcongestion::bbr3::
```

On Windows, first initialize the supported environment with the root helper.
The standalone crate also needs the root's BoringSSL dev-CRT profile settings
(`--config profile.dev.package.boring-sys.opt-level=1` and
`--config profile.dev.package.boring-sys.debug=false`). Its default TLS tests
read the Windows root certificate store; an access-denied sandbox is not
evidence of a TLS regression, and verification must never be disabled to pass.

The ordinary in-memory tests live in
[`crates/usque-transport/src/h3/pmtu_tests.rs`](../../crates/usque-transport/src/h3/pmtu_tests.rs).
They use Usque's real QUIC buffer factory and ephemeral mutually pinned TLS
identities; no sockets, TUN, platform-network changes or external peers are
involved. See the
[issue/validation record](../../docs/pmtu-path-fixes.md).

On Windows, establish the supported native environment using the repository
helper and run the required root Rust gates; do not run a plain release Cargo
command in a fresh shell. The PMTU subset is also runnable in that configured
shell with:

```powershell
cargo test --locked -p usque-transport h3::pmtu_tests:: -- --test-threads=1
```

The root workspace gates compile this dependency but do not run quiche's
standalone upstream test suite. Remove this override only after a pinned
upstream version satisfies the same migration, probe-isolation, DATAGRAM,
handshake-override, DATAGRAM backlog/window classification, loss accounting and
disabled-feature regression contracts on the supported
targets. Do not replace it with an unpinned Git dependency.
