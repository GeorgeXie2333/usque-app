# How v0.1.2 is published

`v0.1.2` is built by the tag workflow on the current `main` commit. The `v0.1.2` tag is maintainer-only. Signing and publish jobs run in GitHub Environments that need approval. If a required file, signature input, or CI result is missing, the workflow fails. A local MSI or APK cannot replace a failed Actions build.

Repository rules that sit around this workflow are in [GITHUB_GOVERNANCE.md](GITHUB_GOVERNANCE.md).

## Before signing starts

- The tag must be `v0.1.2` and must point at the current `main` commit.
- That commit must already have a successful `ci.yml` push run, including `CI / gate`.
- `release-signing` and `release-publish` both require approval.
- Signing material stays in environment secrets. Do not put it in repository variables, files, artifacts, logs, or caches.

## Signing inputs

The `release-signing` environment holds these secrets:

| Name | Meaning |
| --- | --- |
| `WINDOWS_SIGNING_PFX_BASE64` | Base64 of the stable self-signed Authenticode PFX |
| `WINDOWS_SIGNING_PFX_PASSWORD` | PFX password |
| `ANDROID_RELEASE_KEYSTORE_BASE64` | Base64 of the fixed Android release keystore |
| `ANDROID_RELEASE_STORE_PASSWORD` | Keystore password |
| `ANDROID_RELEASE_KEY_ALIAS` | Release key alias |
| `ANDROID_RELEASE_KEY_PASSWORD` | Release key password |

These non-secret variables come from the repository or the same environments:

| Name | Required value |
| --- | --- |
| `WINDOWS_SIGNER_SHA256` | SHA-256 of the raw Authenticode signer certificate, 64 hex characters |
| `ANDROID_SIGNER_SHA256` | SHA-256 of the Android signing certificate, 64 hex characters |

Keep encrypted offline backups of both signing identities. Pre-1.0 packages use these fixed self-signed identities. A v1.0.0 signing change is a separate release.

The Windows job imports the private identity only into the runner user's personal certificate store. It does not add the certificate to Root or TrustedPublisher. Verification accepts the expected untrusted-root result and checks the DER SHA-256 fingerprint. An `always()` step removes the private identity. The workflow never re-signs the official Wintun DLL. The Android job deletes its temporary keystore the same way.

Android builds verify the Gradle 9.5.1 distribution against its published SHA-256, use the checked-in `app/gradle.lockfile`, and check resolved artifacts against `gradle/verification-metadata.xml`. Updating an Android dependency means reviewing and regenerating both files by hand. CI and release jobs must not use `--write-locks` or `--write-verification-metadata`.

The MSI does not install the publisher certificate into the machine Root or TrustedPublisher stores. At runtime the Agent accepts only the `CERT_E_UNTRUSTEDROOT` result expected for this self-signed identity, after Windows has checked the Authenticode digest and signature, and then requires the embedded certificate fingerprint to match `WINDOWS_SIGNER_SHA256`. Any other trust result is fatal.

## Artifact flow

1. The tag job builds signed x64-v2 and ARM64 MSIs plus signed arm64-v8a, x86_64, armeabi-v7a, and universal APKs in the signing environment.
2. Each platform job checks the certificate identity and creates GitHub build provenance.
3. A staging job downloads those artifacts, rejects missing or extra MSI/APK files, and writes SHA-256 sidecars, SPDX and CycloneDX SBOMs, license inventories, and SBOM attestations.
4. The publish job downloads the staged candidate and checks every primary file against the immutable manifest.
5. Only then does the workflow create the GitHub release.

Primary files:

- `usque-v0.1.2-windows-x64-v2.msi`
- `usque-v0.1.2-windows-arm64.msi`
- `usque-v0.1.2-android-arm64-v8a.apk`
- `usque-v0.1.2-android-x86_64.apk`
- `usque-v0.1.2-android-armeabi-v7a.apk`
- `usque-v0.1.2-android-universal.apk`

A public release also needs the usual CI result plus architecture, signature, package, checksum, SBOM, and provenance checks. Endpoint-pin, credential, reconnect, crash, sleep/wake, upgrade, and uninstall leak tests on real hardware are still hardening work, not jobs in this workflow.

## Windows package rules

WiX is locked through `.config/dotnet-tools.json`. Windows Installer has no SemVer prerelease field, so `tool/build_windows_msi.ps1` maps a release as:

```text
MSI build = SemVer patch * 100 + beta ordinal
stable ordinal = 99
```

Stable `v0.1.2` is therefore MSI ProductVersion `0.1.299`. The real SemVer stays in ProductName and the filenames. Equal-version major upgrades are enabled so a validation build can replace the same product instead of installing a second copy under `Program Files\Usque`. WiX validation suppresses only ICE61, which assumes upgrades must raise the version; every other standard ICE check stays on.

The build rejects unsigned project EXE/DLL files, a signer mismatch, PDBs, reparse points, a modified Wintun DLL, a wrong service command, a wrong uninstall action/condition sequence, a 32-bit component, or an ICE failure. True uninstall runs emergency WFP cleanup, journal recovery, optional current-user data cleanup, and clean-state finalization after the service stops and before its binary is removed. A major upgrade runs the first two actions but skips user-data cleanup and clean-state finalization so the replacement service keeps user state and the machine-state directory. The installer UI exposes `INSTALLFOLDER` and stores the chosen path in the 64-bit machine registry for the next major upgrade.

Uninstall keeps the current user's profiles, preferences, logs, caches, and Credential Manager records by default. Settings does not host the MSI wizard, so the package hides the Windows Installer ARP entry (`ARPSYSTEMCOMPONENT`) and registers `usque-uninstall.exe` as the visible uninstall command. That helper asks for confirmation and, only if requested, passes `USQUE_REMOVE_USER_DATA=1` into `msiexec`. Deletion covers only that user's Usque directories and credential namespace. Silent uninstall (`QuietUninstallString` / `msiexec /x /qn`) keeps data unless `USQUE_REMOVE_USER_DATA=1` is set. The shared Wintun driver package is not removed.

User-facing install and uninstall steps are in [INSTALLATION.md](INSTALLATION.md).

## What the runners do not do

GitHub-hosted runners compile, test, sign, inspect, hash, inventory, and attest the release. They do not install an MSI, start Windows VPN/TUN, change runner networking, install APKs on phones, or run long soak or hostile-network tests. Those remain separate hardening work.
