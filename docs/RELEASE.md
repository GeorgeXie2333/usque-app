# Protected public release

This is the operational contract for `v0.1.2`. The workflow remains fail-closed
until the protected signing inputs, exact artifact set, supply-chain metadata,
and required GitHub Actions checks are configured and pass.

## Repository protection

- Follow [GITHUB_GOVERNANCE.md](GITHUB_GOVERNANCE.md) when the repository is
  made public and when the `main` ruleset is created.
- Protect the exact `v0.1.2` tag. Only the release maintainer may
  create it. The release gate requires the tag to point to the current `main`
  commit and verifies that the exact SHA has a successful `ci.yml` push run,
  including `CI / gate`, before any signing job can start.
- Require approval on the `release-signing` and `release-publish` GitHub
  Environments.
- Do not put signing material in repository variables, files, artifacts, logs,
  or caches.
- A local MSI/APK cannot replace a failed or unavailable GitHub Actions build.

## Protected inputs

The `release-signing` Environment owns these secrets:

| Name | Meaning |
| --- | --- |
| `WINDOWS_SIGNING_PFX_BASE64` | Base64 of the stable self-signed Authenticode PFX |
| `WINDOWS_SIGNING_PFX_PASSWORD` | PFX password |
| `ANDROID_RELEASE_KEYSTORE_BASE64` | Base64 of the fixed Android release keystore |
| `ANDROID_RELEASE_STORE_PASSWORD` | Keystore password |
| `ANDROID_RELEASE_KEY_ALIAS` | Release key alias |
| `ANDROID_RELEASE_KEY_PASSWORD` | Release key password |

The repository or protected environments provide these non-secret variables:

| Name | Required value |
| --- | --- |
| `WINDOWS_SIGNER_SHA256` | SHA-256 of the raw Authenticode signer certificate, 64 hex characters |
| `ANDROID_SIGNER_SHA256` | SHA-256 of the Android signing certificate, 64 hex characters |

Keep offline encrypted primary and secondary backups of both signing identities.
All pre-1.0 packages use fixed, project-controlled self-signed identities. Any
v1.0.0 signing transition is a separate reviewed change and must preserve the
documented update-compatibility guarantees.
The Windows workflow imports the private signing identity only into the current
runner user's personal certificate store. It does not add the self-signed
certificate to Root or TrustedPublisher; verification accepts the pinned
untrusted-root result and independently checks the DER SHA-256 fingerprint. An
`always()` cleanup step removes the private identity. The workflow never
re-signs the official Wintun DLL. The Android workflow deletes its temporary
keystore in the same way.

Android builds verify the Gradle 9.5.1 distribution against its published
SHA-256, use the checked-in strict `app/gradle.lockfile`, and verify resolved
Gradle/Maven artifacts against `gradle/verification-metadata.xml`. Updating an
Android dependency therefore requires an explicit review and regeneration of
both files; CI and release jobs must never use `--write-locks` or
`--write-verification-metadata`.

The MSI does not add the self-signed publisher certificate to the user's
machine-wide Root or TrustedPublisher stores. At runtime the Agent accepts only
the specific `CERT_E_UNTRUSTEDROOT` chain result expected for that self-signed
identity, after Windows has checked the embedded Authenticode file digest and
signature, and then requires the embedded certificate's SHA-256 fingerprint to
match `WINDOWS_SIGNER_SHA256`. Bad digests, expired or revoked certificates,
and every other trust result remain fatal.

## Artifact flow

1. The exact tag builds signed x64-v2 and ARM64 MSIs plus signed arm64-v8a,
   x86_64, armeabi-v7a, and universal APKs in the approval-protected signing
   environment.
2. Each platform job verifies its certificate identity and creates GitHub build
   provenance.
3. A staging job downloads those exact artifacts, rejects missing or additional
   MSI/APK files, creates SHA-256 sidecars, SPDX and CycloneDX SBOMs, license
   inventories, and SBOM attestations.
4. The approval-protected publish job downloads the staged candidate and
   re-verifies every primary artifact against the immutable manifest.
5. Only after that verification does the workflow create the GitHub release.

The fixed primary files are:

- `usque-v0.1.2-windows-x64-v2.msi`
- `usque-v0.1.2-windows-arm64.msi`
- `usque-v0.1.2-android-arm64-v8a.apk`
- `usque-v0.1.2-android-x86_64.apk`
- `usque-v0.1.2-android-armeabi-v7a.apk`
- `usque-v0.1.2-android-universal.apk`

## Windows package rules

WiX is locked through `.config/dotnet-tools.json`. Windows Installer has no
SemVer prerelease field, so `tool/build_windows_msi.ps1` maps a release as:

```text
MSI build = SemVer patch * 100 + beta ordinal
stable ordinal = 99
```

Therefore stable `v0.1.2` is MSI ProductVersion `0.1.299`; the actual SemVer is
kept in ProductName and all filenames. Equal-version major upgrades are enabled
so a validation build can replace the same product without parallel products
owning `Program Files\Usque`. WiX validation therefore suppresses only
ICE61, whose older-version-only rule conflicts with that deliberate policy; all
other standard ICE checks remain enabled.

The build rejects unsigned project EXE/DLL files, a signer mismatch, PDBs,
reparse points, a modified Wintun DLL, a wrong service command, a wrong
uninstall action/condition sequence, a 32-bit component, or an ICE validation
failure. True uninstall must run emergency WFP cleanup, detailed journal
recovery, optional current-user data cleanup, and clean-state finalization after
the service stops and before its binary is removed. Major upgrade runs the
first two actions but must skip user-data cleanup and clean-state finalization
so the replacement service retains both user state and its machine-state
directory. The full installer UI exposes `INSTALLFOLDER`; its selected value is
persisted in the 64-bit machine registry and reused by a major upgrade.

The authored uninstall paths preserve current-user Profiles, preferences, logs,
caches, and Credential Manager records by default. Settings does not host the
MSI wizard, so the package hides the Windows Installer ARP entry
(`ARPSYSTEMCOMPONENT`) and registers `usque-uninstall.exe` as the visible
uninstall command. That helper asks for confirmation and, only if requested,
passes `USQUE_REMOVE_USER_DATA=1` into `msiexec`. The explicit deletion path
targets only the uninstalling user's Usque directories and credential namespace;
silent uninstall (`QuietUninstallString` / `msiexec /x /qn`) preserves data
unless `USQUE_REMOVE_USER_DATA=1` is supplied. The shared Wintun driver package
is not an Usque-owned uninstall target.

## Validation boundary

The protected workflow uses GitHub-hosted runners. It compiles, tests, signs,
inspects, hashes, inventories, and attests the release candidate, but it does not
install an MSI, start Windows VPN/TUN, mutate host networking, install APKs on
physical devices, or run long-duration soak and hostile-network tests. Hardware
laboratory evidence is not a release prerequisite. Real-device interoperability,
lifecycle, leak, performance, and clean-machine installation testing remain
project hardening work rather than claims made by the `v0.1.2` release pipeline.
