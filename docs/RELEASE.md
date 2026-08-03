# Protected public-beta release

This is the operational contract for `v0.1.0-beta.1`. It does not declare the
beta ready. The workflow remains fail-closed until the signing environments,
hardware laboratories, and every required test are configured and pass.

## Repository protection

- Protect the exact `v0.1.0-beta.1` tag pattern. Only the release maintainer may
  create it. The release gate requires the tag to point to the current `main`
  commit and verifies that the exact SHA has a successful `CI` push run before
  any signing job can start.
- Require approval on the `release-signing`, `release-lab`, and
  `release-publish` GitHub Environments.
- Do not put signing material in repository variables, files, artifacts, logs,
  caches, or a self-hosted runner image.
- Do not commit `release/READY`. The release workflow generates it after both
  laboratories verify the exact signed artifact hashes.
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
| `WINDOWS_ARM64_RUNNER` | Optional single runner label; defaults to `windows-11-arm` |
| `WINDOWS_LAB_RUNNER` | JSON array of protected self-hosted Windows-lab labels |
| `ANDROID_LAB_RUNNER` | JSON array of protected self-hosted Android-lab labels |

Keep offline encrypted primary and secondary backups of both signing identities.
The Windows workflow imports the certificate only into the current runner user,
trusts it only for the job, and removes it in an `always()` cleanup step. It
never re-signs the official Wintun DLL. The Android workflow deletes its
temporary keystore in the same way.

Android builds verify the Gradle 9.1.0 distribution against its published
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

1. The exact tag builds three signed MSIs and four signed APKs in the
   approval-protected signing environment.
2. Each platform job verifies its certificate identity and creates GitHub build
   provenance.
3. A staging job downloads those exact artifacts, rejects missing or additional
   MSI/APK files, creates SHA-256 sidecars, SPDX and CycloneDX SBOMs, license
   inventories, and SBOM attestations.
4. The staging artifact is passed unchanged to the Windows and Android
   isolated laboratories.
5. Each lab re-hashes the candidate, runs its fixed external harness, and
   uploads evidence plus every referenced attachment.
6. The publish job re-verifies both evidence trees. Only then does it generate
   `release/READY` and create the GitHub prerelease.

The fixed primary files are:

- `usque-v0.1.0-beta.1-windows-x64-v1.msi`
- `usque-v0.1.0-beta.1-windows-x64-v2.msi`
- `usque-v0.1.0-beta.1-windows-arm64.msi`
- `usque-v0.1.0-beta.1-android-arm64-v8a.apk`
- `usque-v0.1.0-beta.1-android-armeabi-v7a.apk`
- `usque-v0.1.0-beta.1-android-x86_64.apk`
- `usque-v0.1.0-beta.1-android-universal.apk`

## Windows package rules

WiX is locked through `.config/dotnet-tools.json`. Windows Installer has no
SemVer prerelease field, so `tool/build_windows_msi.ps1` maps a release as:

```text
MSI build = SemVer patch * 100 + beta ordinal
stable ordinal = 99
```

Therefore `v0.1.0-beta.1` is MSI ProductVersion `0.1.1`; the actual SemVer is
kept in ProductName and all filenames. Equal-version major upgrades are enabled
so a user can replace x64-v1, x64-v2, and ARM64 variants without parallel
products owning `Program Files\Usque`. WiX validation therefore suppresses only
ICE61, whose older-version-only rule conflicts with that deliberate policy; all
other standard ICE checks remain enabled.

The build rejects unsigned project EXE/DLL files, a signer mismatch, PDBs,
reparse points, a modified Wintun DLL, a wrong service command, a wrong
uninstall action sequence, a 32-bit component, or an ICE validation failure.
The MSI is never installed on an ordinary development or Codex host.

## Laboratory boundary

The workflow intentionally does not contain commands that run VPN tests on a
general GitHub-hosted runner. Protected self-hosted runners must provide:

- `C:\UsqueReleaseLab\run-windows-release.ps1`
- `/opt/usque-release-lab/run-android-release`

Those adapters own device orchestration and the isolated gateway. Their command
line and evidence contract are defined in [LAB_EVIDENCE.md](LAB_EVIDENCE.md).
Absence of either adapter blocks the release.
