# v0.3.3-beta.2 prerelease readiness

## Source and release scope

Prepared on 2026-10-22 from dev
`88a4bc3b35884d3608f1f1a7ff6d6e1b00ecb106`, seven commits after beta.1.
Remote main `c145311315b09e1b518832eb939f550a8d101155` remains unchanged.
The candidate is prepared on `release/v0.3.3-beta.2`; the final commit,
Actions runs and package evidence must be recorded after they exist.
This document is not proof that the final candidate has passed or published.

Use exact-SHA release-branch push CI and CodeQL plus one Build.
PR/main promotion gates are not applicable, not passed. The existing stable
main-push gate and both protected release approvals remain unchanged.
After publication and independent verification, merge normally back into dev
and remove the release branch with an exact-head lease; never change main.

## Version and compatibility contract

Twelve first-party Cargo packages use `0.3.3-beta.2`; Flutter is
`0.3.3-beta.2+28` with 21 catalogs and six store changelogs.
APK codes are 28000/28001/28002/28004 for universal/ARMv7/ARM64/x86_64.
MSI/Agent PE are `0.3.302` / `0.3.302.0`; BundleVersion retains SemVer.
Deb/rpm use `0.3.3~beta.2` and RPM Release 1, while filenames,
signed manifests and installed records retain the canonical SemVer.
Future builds must increase Android's base beyond 28.
Preserve numeric Beta ordering, anti-rollback and mixed-format rejection,
and Beta-to-newer-stable Windows updates without accepting Beta targets.

Configuration 26, Agent 3, Windows/Linux journals 6/1 and recovery export 2
remain unchanged. Preserve the dev additions for continuous connection,
metadata-only startup, bounded diagnostics and proxy waiting counters;
release preparation introduces no runtime or wire changes.

## Safety and privacy review

Continuous recovery is cancellable and stops on credential, configuration
and protection failures. Local HTTP/SOCKS waiting is bounded; outage UDP
datagrams are not guaranteed to survive. DIRECT/REJECT decisions remain
policy-bound rather than silently falling back to an unprotected exit.
Linux route authorization and diagnostics must retain generation invalidation,
ownership, fail-closed cleanup and diagnostic privacy filtering.
Metadata-only startup must not become proof of identity readiness.

Proton verification uses only synthetic fixtures during workstation checks.
No real puzzle is automated, account authenticated or tunnel started.
Server flags reflect catalogue metadata, not validated egress geography.
Preserve system HTTPS/DNS versus WARP control-path disclosure in the notes.
Linux packages and experimental Proton exits first shipped in beta.1.
Do not claim new live-network, leak, upgrade or performance acceptance.

Keep existing webpki/root dependency fixes and native source/license locks.
No third-party or frozen Go-oracle changes are part of preparation.
Official private keys remain in GitHub Environments, never on this host.
No package installation, TUN, firewall, route, DNS, proxy or trust mutation
is authorized for workstation validation.

## Acceptance and evidence

Preparation checks on Windows completed 282 Python tests (134 skipped),
Ruff lint/format, repository policy, native source/license locks, Buf lint/
format/breaking against the unchanged main, actionlint, both PSScriptAnalyzer
passes, signing cleanup simulations and frozen Go-oracle verification/tests.
Windows Flutter format/analyze and 1286 tests passed (19 skipped); its release
compile and synthetic Proton native policy fixture also passed. The fixture
uses the supported CMake selected by Flutter rather than the older global
CMake, which cannot name the VS 18 generator.

The two changed bitmap baselines were regenerated only on their matching
hosts with the fixed SDK after reviewing differences confined to the beta
ordinal in the version text. Linux widgets passed 1194 tests (one skipped).
Both inert installer authoring matrices passed, covering 42 language MSIs,
40 transforms and ICE/Burn/replacement/quiet-launcher contracts. Local ARM64
native bootstrapper compile/execution is not_run because the MSVC target
tools are absent; its data-only PE fixture is not native compile evidence.
Final Rust, Android, Linux compilation and hosted acceptance are recorded
separately as they complete, not inferred from these preparation results.

The existing beta.1 contract, metadata, policy and narrow Beta tests are
baseline checks only. Re-run the final candidate's full applicable
[contribution matrix](../CONTRIBUTING.md) and required hosted gates.
Record exact commands, failures, retries, skipped and unavailable checks;
protected labs remain `not_run`, optional and never counted as a pass.

Verify fourteen packages, fourteen SPDX files and four Linux signature
metadata files plus the release manifest and checksums: 34 public assets.
The staged set has 35 files, including two non-public ELF reports and no
public checksums yet. Verify all 28 package provenance/SPDX attestations
against the exact tag, workflow and full source SHA.
Require both approvals and the three fixed signing identities.

Keep the release non-draft and prerelease, with `make_latest: false`;
stable Latest remains v0.3.2. Retain the three Flutter symbol groups at
`dist/flutter-symbols/0.3.3-beta.2+28/<SHA>/` and cross-check actual AOT,
build IDs and hashes after copying. Store this candidate's verification
records separately from beta.1 and keep them when cleaning scoped temp files.
After tagging, only transient infrastructure retries are allowed; a source
or workflow fix requires stopping with the original tag preserved.
