# v0.3.3 release readiness

This is the preparation record for the planned stable v0.3.3 release, not
evidence that a release or any final-candidate check has succeeded. Compare
the user-facing changes with v0.3.2, the previous stable release.

The candidate uses Cargo `0.3.3`, Flutter `0.3.3+29`, Android base 29
(universal/ARMv7/ARM64/x86_64: 29000/29001/29002/29004), Windows MSI
`0.3.399` and Agent PE `0.3.399.0`. It targets fourteen Windows, Android
and Linux packages, fourteen SPDX SBOMs, four Linux signing files, a release
manifest and checksums: 34 public assets.

Require candidate CI, Build and CodeQL, followed by successful push CI and
CodeQL for the exact main SHA. The signing and publishing environments remain
separate. Verify signing identity, provenance, SBOMs, staged files and the
symbol archives for the final tagged candidate before publication. Public
protected-laboratory checks are supplemental and recorded as `not_run`.

The user-facing notes are short and bilingual. Proton VPN is an optional
experimental chain exit; account setup uses the current network. The detailed
connection and privacy boundaries remain in [Proton VPN](PROTON_VPN.md) and
[chain proxy](CHAIN_PROXY.md). Preserve schema 26, Agent protocol 3,
Windows/Linux recovery journals 6/1 and recovery export 2.

## Publication note

Added 2026-11-05: v0.3.3 was published as the stable Latest on 2026-10-26
(UTC) from `1d44597ddb6dee7fca6700c0442e0d3856eea31b`, with 34 public assets.
This note records the outcome only. The text above remains the original
preparation record and does not become final-candidate evidence. Later
development changes are planned for v0.3.4 and need their own readiness record.
