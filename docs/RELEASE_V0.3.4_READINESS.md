# v0.3.4 release readiness

This is a preparation record, not evidence of publication or a successful
final-candidate check. Compare the user-facing release notes with v0.3.3, the
previous stable release.

The candidate uses twelve Cargo packages at `0.3.4`, Flutter `0.3.4+30`,
Android universal/ARMv7/ARM64/x86_64 version codes 30000/30001/30002/30004,
MSI ProductVersion `0.3.499` and Agent PE version `0.3.499.0`. It targets
fourteen Windows, Android and Linux packages, fourteen SPDX SBOMs, four Linux
signing files, a release manifest and checksums: 34 public assets.

Require candidate CI, Build and CodeQL and the exact main push CI and CodeQL.
Both signing and publishing approvals remain separate. Verify every package,
signature, SBOM, attestation and symbol archive against the tagged candidate.
Protected laboratory validation is optional and remains `not_run` when absent.

The reviewed Windows engine producer is run `41924429831` at commit
`e538b43c4df3b5fa3521d00ede8d204e38744b0a`. Its x64 and ARM64 artifacts are
accepted only through the compatibility adapter after input, ancestry, digest,
GitHub provenance, architecture, source-build receipt and symbol checks. The
two restore receipts are independently attested and retained with the PDBs.
Their x64 native engine tests passed in the producer; full ARM64 engine tests
were `not_run`. Compile-only and release application checks remain separate.

Configuration schema 27 adds local connection scenes and cannot be read by
v0.3.3 or earlier. Agent protocol 3, Windows/Linux recovery journals 6/1 and
recovery export 2 remain unchanged. See [installation compatibility](INSTALLATION.md#configuration-compatibility-when-upgrading).
