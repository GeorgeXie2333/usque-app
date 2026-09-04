# How v0.2.4 is published

`v0.2.4` is built by the tag workflow on the current `main` commit. The `v0.2.4` tag is maintainer-only. Signing and publish jobs run in GitHub Environments that need approval. If a required file, signature input, or CI result is missing, the workflow fails. A local MSI or APK cannot replace a failed Actions build.

Which signatures count as official, how fingerprints are published, and what happens if a key is lost or leaked are in [CODE_SIGNING.md](CODE_SIGNING.md). Repository rules around this workflow are in [GITHUB_GOVERNANCE.md](GITHUB_GOVERNANCE.md).

## Before signing starts

- The tag must be `v0.2.4` and must point at the current `main` commit.
- That commit must already have a successful `ci.yml` push run, including `CI / gate`.
- `release-signing` and `release-publish` both require approval.
- Android Developer Console must show `io.github.georgexie2333.usque` and the certificate fingerprint in `ANDROID_SIGNER_SHA256` as **Registered**.
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
3. A staging job downloads those artifacts, rejects missing or extra MSI/APK files, writes an internal release manifest, generates SPDX SBOMs, and records SBOM attestations.
4. The publish job rechecks every primary package against the immutable manifest, calculates final package checksums, and creates the GitHub release with the six install packages, manifest, checksums, and per-package SBOMs.
5. When repository variable `RUN_PROTECTED_RELEASE_VALIDATION` is exactly `true`, four protected self-hosted runner classes separately exercise the staged candidate: a Windows snapshot VM, a dedicated Android device, an independent network observer, and a controlled performance lab.
6. Protected validation is supplemental and does not gate publication. The aggregator emits `reliability-report.json` and `device-matrix.md` as a protected Actions artifact only when every required report and evidence file passes its exact-candidate and isolation checks. Missing infrastructure, `failed`, and `not_run` remain visible non-passes and never become release approval.

## Release-note format

`.github/RELEASE_NOTES_TEMPLATE.md` is the publication source for the GitHub
Release body. Before creating a new release tag, replace the **Highlights**
items with that release's user-visible changes. Every statement is written in
English first, followed immediately by its Simplified Chinese translation.
Keep the standard sections for official downloads, installation requirements,
signature and evidence verification, and issue feedback.

The release renderer accepts only the version, official repository URL, and
the two validated signer fingerprints as template values. It rejects missing
or unknown template values and links outside this repository. This keeps the
published body free of community-group links, sponsorships, advertisements,
affiliate links, and referral codes. GitHub-generated release notes stay off
because an automatically appended monolingual changelog would break the
bilingual ordering. The publish job fails instead of falling back to an
unrendered or partially rendered body.

Primary files:

- `usque-v0.2.4-windows-x64-v2.msi`
- `usque-v0.2.4-windows-arm64.msi`
- `usque-v0.2.4-android-arm64-v8a.apk`
- `usque-v0.2.4-android-x86_64.apk`
- `usque-v0.2.4-android-armeabi-v7a.apk`
- `usque-v0.2.4-android-universal.apk`

In addition to the six install packages, the public release includes
`release-manifest.json`, `SHA256SUMS`, and each package's SPDX SBOM. A public
release requires the usual CI, architecture, signature, package, checksum,
SBOM, and provenance checks. Protected-runner validation is optional and
non-blocking; its environments, evidence contract, and privacy boundary are
documented in [RELIABILITY_TESTING.md](RELIABILITY_TESTING.md).

## Windows package rules

WiX is locked through `.config/dotnet-tools.json`. Windows Installer has no SemVer prerelease field, so `tool/build_windows_msi.ps1` maps a release as:

```text
MSI build = SemVer patch * 100 + beta ordinal
stable ordinal = 99
```

Stable `v0.2.4` is therefore MSI ProductVersion `0.2.499`. The real SemVer stays in ProductName and the filenames. The Agent embeds the same mapped value as its four-part PE file version (for example, `0.2.499.0`), and packaging rejects an unversioned or mismatched Agent. Equal-version major upgrades are enabled so a validation build can replace the same product instead of installing a second copy under `Program Files\Usque`. WiX validation suppresses only ICE61, which assumes upgrades must raise the version; every other standard ICE check stays on.

`RemoveExistingProducts` runs after `InstallExecute` and before
`InstallFinalize`. This transactionally installs the fixed, versioned Agent
before a cached older MSI invokes its fail-closed `--recover-state` action. It
is the compatibility bridge for installed `v0.2.4` packages affected by the
asynchronous Wintun-removal check. Keep the Agent's component GUID, KeyPath,
installation path, recovery CLI, and supported journal schema compatible across
this late-removal upgrade. Do not restore early related-product removal while a
supported client may upgrade directly from `v0.2.4`; otherwise that client
would execute the broken Agent before the replacement file exists.

The Agent is installed as demand-start and is not started by the MSI. Its
`MsiLockPermissionsEx` descriptor gives `SYSTEM` and Administrators full service
control and gives the well-known `INTERACTIVE` SID only
`SERVICE_QUERY_STATUS | SERVICE_START`. The package must contain no legacy
`LockPermissions` table. At runtime every non-clean recovery phase maps to Auto
start, and only a durably Clean journal maps back to Demand start. Engine cold
start waits up to 30 seconds for the named pipe; Clean idle exit is 10 seconds
and tunnel reattachment grace is 30 seconds.

The Start Menu shortcut is deliberately non-advertised and lives in its own
HKCU-KeyPath component. Repair, Modify, and Patch are unsupported: the UI shows
an explanation, and an execute-sequence error action rejects command-line
maintenance before `StopServices`. Normal uninstall and major upgrade remain
allowed.

The GUI accepts Restart Manager's `ENDSESSION_CLOSEAPP` query and commit
messages as a maintenance-only disconnect-and-exit request, bypassing the normal
close-to-tray behavior. The MSI sets `MSIRMSHUTDOWN=1` so a pre-protocol or hung
process is force-closed only after Restart Manager's bounded graceful timeout,
and sets `MSIDISABLERMRESTART=1` so an old process is never relaunched after an
uninstall or in the middle of a major upgrade.

The build rejects unsigned project EXE/DLL files, a signer mismatch, an unversioned or version-mismatched Agent, PDBs, reparse points, a modified Wintun DLL, a missing `usque-update.exe`, a wrong service command/start type/DACL, an advertised shortcut, a missing maintenance guard, an early related-product removal sequence, a wrong uninstall action/condition sequence, a 32-bit component, or an ICE failure. The signed update helper reuses the Agent's offline Authenticode verifier and additionally checks the MSI SHA-256, UpgradeCode, mapped stable ProductVersion, summary architecture, and `USQUE_UPDATE_VARIANT` property before starting Windows Installer. True uninstall runs emergency WFP cleanup, journal recovery, optional current-user data cleanup, and clean-state finalization after the service stops and before its binary is removed. A major upgrade runs the first two actions but skips user-data cleanup and clean-state finalization so the replacement service keeps user state and the machine-state directory. The installer UI exposes `INSTALLFOLDER` and stores the chosen path in the 64-bit machine registry for the next major upgrade.

Uninstall keeps the current user's profiles, preferences, logs, caches, and Credential Manager records by default. Settings does not host the MSI wizard, so the package hides the Windows Installer ARP entry (`ARPSYSTEMCOMPONENT`) and registers `usque-uninstall.exe` as the visible uninstall command. That helper asks for confirmation and, only if requested, passes `USQUE_REMOVE_USER_DATA=1` into `msiexec`. Deletion covers only that user's Usque directories and credential namespace. Silent uninstall (`QuietUninstallString` / `msiexec /x /qn`) keeps data unless `USQUE_REMOVE_USER_DATA=1` is set. The shared Wintun driver package is not removed.

User-facing install and uninstall steps are in [INSTALLATION.md](INSTALLATION.md).

## Runner isolation boundary

GitHub-hosted runners compile, test, sign, inspect, hash, inventory, attest, and
aggregate the release. They do not install an MSI, start Windows VPN/TUN,
change runner networking, or install APKs on devices.

The separate, opt-in protected self-hosted jobs perform destructive lifecycle,
independent-network, and performance testing only in the environments described
in [RELIABILITY_TESTING.md](RELIABILITY_TESTING.md). They are supplemental and
do not gate publication. Do not provision those runner labels on a developer
workstation, and do not treat an environment variable or label as proof of
isolation.
