# Usque's pinned quiche 0.30.1 patch

This directory starts from the complete published `quiche` 0.30.1 crate,
with Usque's 0.29.3 transport patches rebased onto it. Original license and public example/test
fixtures are retained. See [COPYING](COPYING) (BSD-2-Clause) and
[COPYING-BBR3](COPYING-BBR3) (Revised BSD for the added BBRv3 implementation).

## Provenance

- Registry archive: `quiche-0.30.1.crate` from crates.io.
- Archive SHA-256:
  `8ba5af7b73277657f07f47615a3440f6920bf1b747fecd7ca5e9957ff77c5b98`.
- Published VCS revision:
  `b364fdfee3799d8c469d5cd8f5693fafb420a466`
  (also recorded in `.cargo_vcs_info.json`).
- All 93 published files were extracted from the checksum-verified archive
  before patching. The retained PMTU changes are in `src/lib.rs` and
  `src/path.rs`; explicit local probe rejection also changes `src/pmtud.rs`.
  The congestion-control additions are confined to `recovery/`,
  its public C algorithm enum, and license/package metadata. The PTO accounting
  patch also changes the shared path/recovery outcome and adds
  `src/path_pto_tests.rs`; the upstream BBRv2, Reno and CUBIC algorithm files
  remain unchanged relative to 0.30.1.
- The root `[patch.crates-io]` selects this directory. Both lockfiles retain
  Boring 4.22.0. The upgrade updates `intrusive-collections` to 0.10.3 and
  `octets` to 0.3.7; the independent test lock also updates `qlog` to 0.18.2.
  The vendored crate is excluded from workspace formatting and membership to
  preserve upstream source formatting.
- The normalized manifest requires `libc >=0.2.191,<0.3`, retaining the existing
  workspace resolution and patched `rustix` 1.1.6. Upstream's temporary
  `<0.2.190` cap works around an iOS `backtrace` dependency absent from Usque;
  retaining that cap would downgrade the workspace's rustix security fix.
  See [the upstream cap](https://github.com/cloudflare/quiche/commit/472f96a979ac30f0d2231beade581c75a4e9c735)
  and [RUSTSEC-2026-0486](https://rustsec.org/advisories/RUSTSEC-2026-0486.html).

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
4. Add the Rust-only `Connection::on_pmtu_probe_send_error()` for an explicit
   UDP message-too-large error. It requires the exact active, validated path
   and an outstanding above-floor probe with the rejected payload length.
   The path's search ceiling is reduced immediately, without the ordinary
   loss retry budget or PTO wait. Only peer ACKs confirm a usable PMTU.
   Locally rejected sizes remain excluded across search restarts/revalidation;
   their late loss/ACK callbacks cannot retire a newer probe or revive a rejected
   size. Recovery still owns packet accounting and no congestion algorithm is
   changed. Newly created paths retain the original configured ceiling.
   Unmatched/stale errors, ordinary packets, disabled discovery and the QUIC
   minimum size are rejected by this API for the caller's existing handling.
5. Preserve distinct cumulative loss-detection callback and actual PTO-expiry
   counters. ACK progress does not erase their history, and a loss-time callback
   does not count as a PTO. Both recovery implementations report which timeout
   occurred; the path accounting tests exercise all four application algorithms.

Related upstream work, rechecked on 2026-11-12:
[runtime-path PMTUD PR #2573](https://github.com/cloudflare/quiche/pull/2573)
(open/unmerged) and
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
The closed `BbrSender` enum keeps upstream BBRv2 sender and pacer behavior
while routing `bbr3` to the independent sender. Its Rust/C algorithm value is
now 6: upstream 0.30.1 assigns 5 to the optional `CongestionWindowUnchecked`.
Any external C consumer of the previous local value must rebuild with the new
header. Usque stores algorithm names and explicitly maps its protobuf enum;
the persisted `bbr3` name and protobuf value 4 do not change. Removed upstream
values 2 and 3 are not reused. `bbr` still names BBRv2. The upstream persistent
RTT-jump counter is forwarded for BBRv2; BBRv3 has no such detector and reports
zero. The unchecked controller remains disabled in the application.

The source includes a bounded virtual FIFO-link test harness with application
limiting, ACK aggregation, injected loss, bandwidth changes and a token-bucket
policer. Transport tests additionally exercise real pinned-TLS QUIC packets
with all four algorithms, including DATAGRAM, PMTU, migration and closure.

CI explicitly runs the standalone crate's locked unit suite on Linux, plus
BBRv3 tests with qlog enabled, and a feature-enabled regression proving BBRv3
and the unchecked controller select independent recovery implementations.
Workspace tests alone do not run that suite:

```shell
cargo test --manifest-path third_party/quiche-0.30.1/Cargo.toml --locked --lib
cargo test --manifest-path third_party/quiche-0.30.1/Cargo.toml --locked --lib --features qlog recovery::gcongestion::bbr3::
cargo test --manifest-path third_party/quiche-0.30.1/Cargo.toml --locked --lib --features congestion_window_unchecked_available bbr3_and_unchecked_keep_independent_recovery
```

On Windows, first initialize the supported environment with the root helper.
The standalone crate also needs the root's BoringSSL dev-CRT profile settings
(`--config profile.dev.package.boring-sys.opt-level=1` and
`--config profile.dev.package.boring-sys.debug=false`). Unlike the workspace's
patched `boring-sys`, the standalone registry build needs the native
`--target=x86_64-pc-windows-msvc` and imported MSVC/SDK include directories
forwarded through `BINDGEN_EXTRA_CLANG_ARGS` on x64 Windows. Restore these
native-only arguments before running Android checks. If the selected NDK
libclang cannot load its `libwinpthread-1.dll`, make that same pinned DLL
available beside each generated `boring-sys` build-script executable; feature
sets such as qlog may create a different build-script directory. Do not change
the NDK revision or trust configuration to work around a loader failure.
Its default TLS tests
read the Windows root certificate store; an access-denied sandbox is not
evidence of a TLS regression, and verification must never be disabled to pass.

The ordinary in-memory tests live in
[`crates/usque-transport/src/h3/pmtu_tests.rs`](../../crates/usque-transport/src/h3/pmtu_tests.rs).
They use Usque's real QUIC buffer factory and ephemeral mutually pinned TLS
identities; no sockets, TUN, platform-network changes or external peers are
involved. See the
[issue/validation record](../../docs/pmtu-path-fixes.md).

Local-send rejection regressions additionally compare a 1464-byte limit with
ordinary packet loss: explicit rejection tries 1472, 1467 and 1465 once each,
while loss retains three attempts per failed size. The transport suite runs
real in-memory QUIC flights with all four congestion algorithms, verifies
continued small-packet delivery, exact path/size matching, peer-ACK validation
and an independent full-ceiling path after migration. PMTUD unit tests cover
late callbacks, search restart and rejection at the minimum size.

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
