# v0.3.2 release readiness review

Review date: 2026-10-17. This preparation record is not final-candidate
certification or evidence that a package was published.

## Source and scope

The reviewed dev baseline is `11d19a442ee341ad8ff98e6f88d61b1f64a8ca51`;
main was `42a026eaf1a8dc58c724463ad2b8e842a7e6112b`. Its unique Dependabot
policy patch is already preserved in dev. Ordinary merge retains both histories.
Main promotion is a maintainer-authorized fast-forward through the existing
owner exception, not a PR or Squash; PR Check is `not_run`.

The release targets 12 Cargo packages at 0.3.2, Flutter 0.3.2+26 and 21 catalogs.
Android codes are 1026/2026/4026/26. MSI is 0.3.299, Agent PE 0.3.299.0 and
BundleVersion 0.3.2. Two EXEs, two update-only MSIs, four APKs, eight SPDX files,
the manifest and SHA256SUMS total 18 public assets.

The source includes Chrome 155 TLS, IPv6 endpoint presets and bounded leases,
live Ads updates, registration/DNS recovery and Android UI lifecycle changes.
Configuration 26 and journal 6 are not downgrade-compatible with v0.3.1.
Protocol 3 and sanitized export 2 remain. Release preparation adds no runtime
interface, weakens no protection and does not modify third-party patches.

## Baseline checks, not final results

At the reviewed baseline, the existing v0.3.1 version contract, repository
policy, locked Cargo metadata and diff whitespace check completed successfully.
Python 3.12.14 ran 113 tests, with five Linux-only skips. The first sandbox
attempt failed on temporary-directory permissions; the normal-host retry
completed. These counts do not describe the bumped candidate.

## Required candidate and publication evidence

- Rerun the v0.3.2 contract and every applicable contribution-matrix check.
  Keep exact command outcomes and unavailable host/isolated checks separate.
- Verify final dev CI, Build and CodeQL, dependency/security findings and OSV.
  After promotion, require exact-SHA main push CI and CodeQL. The release
  contract rejects dev/PR/manual/unfinished/wrong-SHA CI runs.
- Recheck immutable tag, approval environments, secret names and fixed public
  fingerprints. The maintainer confirmed the unchanged Android application-ID
  and certificate pair is still Registered; no live console read is claimed.
- Approve signing only after the release gate, and publication only after
  package, signature, architecture, supply-chain, symbol and staging checks.
- Verify all 18 public assets, 16 attestations and three retained symbol
  archives against the precise tagged candidate. Retain verification records
  and symbols independently of Actions artifact expiry.

Do not install or execute installers/APKs, create VPN/TUN, mutate networking
or install certificate trust on the workstation. Protected lab runs remain
`not_run`, not passed, and are supplemental rather than publication gates.
Deterministic tests and fake-engine goldens do not prove real upgrades,
anonymity, leak prevention or measured performance. Preserve the tag if a
post-tag code/workflow change becomes necessary.
