# Usque's pinned quiche 0.29.3 patch

This directory starts from the complete published `quiche` 0.29.3 crate,
not a floating branch or an upgrade. Original license and public example/test
fixtures are retained. See [COPYING](COPYING) (BSD-2-Clause).

## Provenance

- Registry archive: `quiche-0.29.3.crate` from crates.io.
- Archive SHA-256:
  `61166d27591eb7cb1310eec2b8fc6ae0e0686e9e4ed742a3ffc6317171175e7d`.
- Published VCS revision:
  `09b125d4cfc16e78d73d8382c93926f3aba063d4`
  (also recorded in `.cargo_vcs_info.json`).
- All 87 published files were verified byte-for-byte against that archive
  before patching. Only `src/lib.rs` and `src/path.rs` contain behavioral
  changes. One trailing space in a test comment in `src/recovery/mod.rs` was
  removed to satisfy the repository's whitespace gate; its code is unchanged.
  The other 84 published files remain byte-identical. This note is the sole
  additional file.
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

Related upstream work, inspected on 2026-09-03:
[runtime-path PMTUD PR #2573](https://github.com/cloudflare/quiche/pull/2573)
(open/unmerged, head `cc07864532f1d6232fd4d063ef08cf920a9070bd`) and
[send-path PMTUD PR #2566](https://github.com/cloudflare/quiche/pull/2566).
The retained enablement/budget and validation gating follow the same approach
as #2573; this local patch also bounds new-path ceilings and repairs DATAGRAM
admission. These links are context, not build-time dependencies or a claim
that upstream has merged the fixes.

## Regression coverage and removal

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
handshake-override and disabled-feature regression contracts on the supported
targets. Do not replace it with an unpinned Git dependency.
