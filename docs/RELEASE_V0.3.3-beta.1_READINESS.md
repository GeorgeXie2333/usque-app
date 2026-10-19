# v0.3.3-beta.1 prerelease readiness

## Scope and source

Prepared on 2026-10-20 from dev
`27811c77820e18f995e400a3362d2096dee8e3e7`; remote main is
`c145311315b09e1b518832eb939f550a8d101155` and must remain unchanged.
Local backup references retain both sources. This is preparation evidence,
not a claim that an uncommitted tree is the final signed candidate.

The short-lived `release/v0.3.3-beta.1` branch requires exact-SHA release
**push** CI/CodeQL and Build. PR/main promotion gates are not applicable,
not passed. Stable main admission remains intact. No automatic Beta update
channel, separate app identity, new protocol fields or schema changes.

## Contract and safety review

Twelve Cargo packages use `0.3.3-beta.1`; Flutter uses
`0.3.3-beta.1+27` with 21 catalogs. APK codes are
27000/27001/27002/27004. MSI/Agent PE are 0.3.301/0.3.301.0.
Deb/rpm metadata uses `0.3.3~beta.1`; signed/installed identities retain
SemVer, numeric ordinals and fail-closed anti-rollback checks.
Windows permits an installed Beta to advance to stable 0.3.3, but never
offers a Beta target through the stable updater.

Fourteen packages and fourteen SPDX files form part of 34 public assets.
The 35-file staged candidate includes two non-public ELF reports; publish
adds checksums. Verify 28 package provenance/SPDX attestations and three
fixed signing identities, then both approvals in order. Stable Latest
remains v0.3.2; the Beta must be non-draft, prerelease and not Latest.

Linux installer ownership, symlink, format, signature and rollback checks
are retained. The new comparator does not authorize downgrades or mixed
formats. Native Proton fixtures use synthetic challenges only; they do
not authenticate, open a WebView or establish live tunnel acceptance.
Account/verification system-network and WARP control-path privacy limits
remain explicit in the notes. No official keys are accessed locally.

## Validation record

The second candidate's hosted Windows x64 Build failed the loopback HTTP
cancellation fixture at its peer-close wait: a 100 ms sleep could cancel
before a connection was established, leaving the fixture in accept(). The
pre-tag correction pins only the test's localhost address and waits for a
complete request before cancellation. Readiness remains bounded, and the
original two-second peer-close assertion is retained. Production HTTP,
timeout, retry and credential handling are unchanged. This requires a new
candidate SHA and fresh required Actions, not a waived or silently retried
failure.

The superseded Ubuntu Rust gate also stalled in the metadata-backoff fixture:
its paused clock advanced before a blocking persistence load finished, then
an unbounded yield loop prevented further timer progress. Both admission and
backoff fixtures now use bounded real-time observation and abort-on-drop task
ownership. The backoff test records request times and explicitly requires at
least 30 seconds before retry; production lease scheduling is unchanged.

The original Windows Python baseline was 271 tests, five errors and 131
skips: it did not pass. Synthetic host facts fix three Proton errors;
two actual POSIX fixtures are Linux-only and remain required on Linux.
The existing Linux Python job installs hash-locked PoC dependencies.

After these edits, `python -m unittest discover -s tool -p "test_*.py"`
completed 276 tests with 134 skips and no failures on Windows. Ruff lint
and format also passed. A narrow Settings row overflow caused by the longer
Beta label was fixed by allowing that existing label to wrap. The diagnostics
Windows golden was regenerated with the pinned SDK; visual review found only
the version text changed. The first candidate exposed one Linux Settings
golden mismatch; a local pinned Linux-SDK regeneration retained right-aligned
update actions, and a regression asserts that alignment. The reviewed Linux
image changes only the version text. CI now installs Node/MIME fixture tools
in the existing Python job as well as hash-locked PoC dependencies. These results cover the worktree at that check, not a future
tag. Final native/Flutter/Android/packaging and hosted
gates are still pending and must be recorded against the final source.

Run the exact [contribution matrix](../CONTRIBUTING.md) and
[release procedure](RELEASE.md). Local native ARM64 tools are unavailable and remain `not_run`. The existing
WSL Linux SDK matches the pin and supports local widget/golden validation;
it does not replace Ubuntu-22.04 package-baseline checks or native ARM64.
Hosted checks do not turn a local omission into a pass.
Protected labs and real install/upgrade/network/leak/performance checks
remain `not_run`, supplemental and non-blocking. No package is installed
or executed and no networking or certificate trust is changed.

Retain final commands, run IDs, candidate SHA, signatures, asset/proof
receipts and symbol hashes. The three Windows/Android symbol archives
belong under `dist/flutter-symbols/0.3.3-beta.1+27/<SHA>/`; no Linux
symbol separation is claimed. Merge back into dev only after verification,
preserve concurrent history, and delete the branch with an exact-head
lease. Do not alter main or move/recreate the tag.
