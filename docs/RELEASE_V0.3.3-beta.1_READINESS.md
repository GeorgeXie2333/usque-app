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

The original Windows Python baseline was 271 tests, five errors and 131
skips: it did not pass. Synthetic host facts fix three Proton errors;
two actual POSIX fixtures are Linux-only and remain required on Linux.
The existing Linux Python job installs hash-locked PoC dependencies.

After these edits, `python -m unittest discover -s tool -p "test_*.py"`
completed 276 tests with 134 skips and no failures on Windows. Ruff lint
and format also passed. A narrow Settings row overflow caused by the longer
Beta label was fixed by allowing that existing label to wrap. The diagnostics
Windows golden was regenerated with the pinned SDK; visual review found only
the version text changed. These results cover the worktree at that check, not a future
tag. Final native/Flutter/Android/packaging and hosted
gates are still pending and must be recorded against the final source.

Run the exact [contribution matrix](../CONTRIBUTING.md) and
[release procedure](RELEASE.md). Missing native Linux/ARM64 hosts are
`not_run` locally; hosted checks do not turn a local omission into a pass.
Protected labs and real install/upgrade/network/leak/performance checks
remain `not_run`, supplemental and non-blocking. No package is installed
or executed and no networking or certificate trust is changed.

Retain final commands, run IDs, candidate SHA, signatures, asset/proof
receipts and symbol hashes. The three Windows/Android symbol archives
belong under `dist/flutter-symbols/0.3.3-beta.1+27/<SHA>/`; no Linux
symbol separation is claimed. Merge back into dev only after verification,
preserve concurrent history, and delete the branch with an exact-head
lease. Do not alter main or move/recreate the tag.
