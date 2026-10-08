# Release process

This is a maintainer reference for the checked-in release workflow, not a
record that the current checkout has been published. The authoritative
executable contracts are [release.yml](../.github/workflows/release.yml) and
[release_contract.py](../tool/release_contract.py).

The current release contract targets `v0.3.0`. The checked-in workflow
accepts only `v0.3.0` and requires that tag to point at
the current `main` commit when its gate runs. The tag is maintainer-only.
Signing and publish jobs run in GitHub Environments that need approval. If a
required file, signing input, or CI result is missing, the workflow fails. A
local bundle, MSI, or APK cannot replace a failed Actions build.

The current source retains the newer-Agent-first Windows upgrade sequence
and complete payload replacement introduced in v0.2.5. Those fixes are not part
of the original v0.2.4 release. The multilingual EXE installer and hidden-bundle
uninstall lifecycle are new in v0.2.6, not the original v0.2.5 MSI-only release.
Read each release's tagged documentation and notes for its delivered behavior.
This guide does not authorize moving or reusing a
published tag; a subsequent release needs a separately reviewed version and
workflow update and the existing approval gates. Static and compile-only
checks are not evidence of a successful real-machine upgrade.

## Preparing v0.3.0

The coordinated source version is `0.3.0`, with Flutter `0.3.0+24` and Android
base versionCode `24`. Cargo's workspace and first-party lock entries, the
registered locale catalogs, `release.yml`, and the CI version gate target
`v0.3.0`. These declarations do not establish a passed candidate, authorize
an approval or prove publication. Complete the exact-candidate checks before
tagging; do not retag or reuse `v0.2.9` for the newer source.

Verify that change across:

- `Cargo.toml` and the first-party workspace package entries in `Cargo.lock`;
- `apps/usque_gui/pubspec.yaml`, with version `0.3.0+24`;
- the `app_version` entries in all 21 registered Dart locale catalogs;
- the tag trigger, `RELEASE_TAG` and `ANDROID_VERSION_CODE` in
  [release.yml](../.github/workflows/release.yml), and the release-version gate
  in [ci.yml](../.github/workflows/ci.yml);
- the accepted-tag statements in this guide and
  [GitHub governance](GITHUB_GOVERNANCE.md), and the development wording and
  package examples in the six root READMEs and [Installation](INSTALLATION.md).

Base Android versionCode `24` follows the published `23`. Verify each derived
APK code against the packages and update paths already distributed. Use
`0.3.0+24`, keep `ANDROID_VERSION_CODE` and the CI check at `24`, and run:

```shell
python tool/release_contract.py verify-version --root . --tag v0.3.0 --android-version-code 24
```

That command must succeed for the coordinated checkout. Split APK version
codes are 1024 (ARMv7), 2024 (ARM64), and 4024 (x86_64); the universal APK uses
24. These advance the respective v0.2.9 codes 1023/2023/4023/23. Version-code
comparison must cover the three ABI-specific updates and an installed
universal APK updating to its device's ABI-specific APK. A split APK has a
higher code than the same release's universal APK; do not assume manually
switching from split to universal is an accepted upgrade or bypass Android's
monotonic version check.

The version helper verifies Cargo's workspace version, Flutter, registered locale catalogs
and the release workflow; review first-party `Cargo.lock` entries and the CI
invocation separately because the helper does not inspect them.

Version-number fixtures for MSI conversion and historical validation records
keep their original versions, candidates, test counts and `not_run` results.
They do not prove the final `v0.3.0` candidate. The tag must point at the
resulting current `main` commit after its required CI succeeds. Keep the
existing signing and publication approvals, cleanup checks and immutable
candidate requirements. Rerun applicable checks after the final source change
and retain the exact commit and Actions run identities.

The current source uses configuration schema 24: schema 22 adds WARP DoH/DoT
settings, schema 23 adds account-specific Zero Trust endpoint overrides, and
schema 24 migrates custom bypasses to unified routing rules with Ads.
Recovery journal schema 5, Agent protocol 3 and sanitized recovery export
schema 2 remain unchanged. Once configuration is migrated, the `v0.2.9`
engine rejects schema 24. Unchanged Agent and journal versions do not prove
downgrade compatibility; do not edit schema numbers to bypass that check.
Review the release-note summary and highlights
against `v0.2.9..HEAD`: custom WARP encrypted DNS, experimental Zero Trust
endpoint editing, native Windows setup/removal, desktop controls, Home and
branding changes, Android recovery and chain UDP burst handling are the new
scope. Existing HTTP/SOCKS5 chain exits and automatic Consumer endpoints must
not be advertised as first introduced in `v0.3.0`.

Which signatures count as official, how fingerprints are published, and what happens if a key is lost or leaked are in [CODE_SIGNING.md](CODE_SIGNING.md). Repository rules around this workflow are in [GITHUB_GOVERNANCE.md](GITHUB_GOVERNANCE.md).

## Before signing starts

- For `v0.3.0`, complete the version and workflow changes above first. The
  accepted tag must match `release.yml` and point at the current `main` commit.
- That commit must already have a successful `ci.yml` push run, including `CI / gate`.
- `release-signing` and `release-publish` both require approval.
- Android Developer Console must show `io.github.georgexie2333.usque` and the certificate fingerprint in `ANDROID_SIGNER_SHA256` as **Registered**.
- Signing material stays in environment secrets. Do not put it in repository variables, files, artifacts, logs, or caches.

Review the live `main` ruleset, tag restrictions and both environment approval
settings before tagging. A checked-in document cannot establish their current
GitHub configuration. `PR Check / gate`, `CI / gate` and `Build / gate` are the
documented merge checks. The release gate separately requires a successful
`ci.yml` **push** run for the exact tagged current `main` SHA; an earlier PR
run or local check is insufficient. `publish` depends only on `stage-candidate`
and requires `release-publish` approval; protected reliability jobs are not
dependencies of publication.

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

The Windows job imports the private identity only into the runner user's personal certificate store. It does not add the certificate to Root or TrustedPublisher. It records the imported identity before checking the fingerprint or locating SignTool, so those failures still reach cleanup. Verification accepts the expected untrusted-root result and checks the DER SHA-256 fingerprint. An `always()` step removes both the certificate and its private key with `-DeleteKey`; its `finally` block deletes the temporary PFX even if certificate removal fails. Cleanup errors fail the job. The workflow never re-signs the official Wintun DLL. The Android job deletes its temporary keystore afterward.

Android builds verify the Gradle 9.5.1 distribution against its published SHA-256, use the checked-in `app/gradle.lockfile`, and check resolved artifacts against `gradle/verification-metadata.xml`. Updating an Android dependency means reviewing and regenerating both files by hand. CI and release jobs must not use `--write-locks` or `--write-verification-metadata`.

The Windows bundle and MSI do not install the publisher certificate into the
machine Root or TrustedPublisher stores. At runtime the Agent accepts Windows'
successful Authenticode result (`0`) or the `CERT_E_UNTRUSTEDROOT` result
expected for this self-signed identity, after the digest and signature checks.
It then requires the embedded certificate fingerprint to match
`WINDOWS_SIGNER_SHA256`. Every other trust result is fatal; success never
bypasses the fingerprint check.

## Artifact flow

1. The tag job builds signed x64-v2 and ARM64 installer bundles, their signed update MSIs, and signed arm64-v8a, x86_64, armeabi-v7a, and universal APKs in the signing environment.
2. Each platform job checks the certificate identity and creates GitHub build provenance.
   Both Build and Release use the shared read-only APK packaging validator:
   native libraries must be compressed, the application must explicitly enable
   extraction, 64-bit ELF LOAD segments must support 16 KiB alignment, and ZIP
   alignment must pass. Wrong ABIs, duplicate entries, shipped symbols and native
   debug information are rejected before candidate upload. No APK is installed.
3. A staging job downloads those artifacts, rejects missing or extra EXE/MSI/APK files, writes an internal release manifest, generates SPDX SBOMs, and records SBOM attestations.
4. The publish job rechecks every primary package against the immutable manifest, calculates final package checksums, and creates the GitHub release with six user-facing installers, two update-only MSIs, the manifest, checksums, and per-package SBOMs.
   Separately retain the three matching Flutter symbol artifacts for the
   supported lifetime of this release, following [Flutter release symbols](FLUTTER_SYMBOLS.md).
   Verify their source SHA, version, AOT/build-ID and symbol hashes against the
   actual packages. Symbols stay outside the 18 public Release assets and must
   not be deleted with temporary package-verification downloads.
5. Only in a private repository, and when repository variable `RUN_PROTECTED_RELEASE_VALIDATION` is exactly `true`, four protected self-hosted runner classes separately exercise the staged candidate: a Windows snapshot VM, a dedicated Android device, an independent network observer, and a controlled performance lab. The public repository skips these jobs even if the variable is enabled; its Actions artifacts cannot provide a restricted evidence store.
6. Protected validation is supplemental and does not gate publication. In the private execution context, the aggregator emits `reliability-report.json` and `device-matrix.md` only when every required report and evidence file passes its exact-candidate and isolation checks. These artifacts inherit the private repository's read permissions. Skipped or unavailable validation is `not_run`, not a pass. Missing infrastructure, `failed`, and `not_run` never become release approval.

## Release-note format

`.github/RELEASE_NOTES_TEMPLATE.md` is the publication source for the GitHub
Release body. Before creating a new release tag, replace the **Highlights**
items and version summary with that release's user-visible changes. Keep the
upgrade notes and folded technical and DNS details in sync with that version,
and recheck versioned facts such as configuration schema, Agent protocol, and
recovery journal and export schema numbers against the source. Every statement is written in
English first, followed immediately by its Simplified Chinese translation.
Separate each bilingual list item from the next with a blank line so GitHub
renders paragraph spacing after the Chinese text as well as before it.
Keep the standard sections for official downloads, installation requirements,
signature and evidence verification, and issue feedback.
Keep the version summary, highlights, download table, official-source warning,
and Windows EXE guidance visible. Put supplementary installation guidance,
upgrade notes, technical and DNS details, verification instructions and evidence,
and feedback guidance in separate, default-collapsed `details` blocks with
English-first bilingual summaries. Keep the standard section headings outside
the blocks so readers can find them without expanding the content.

The release renderer accepts only the version, official repository URL, and
the two validated signer fingerprints as template values. It rejects missing
or unknown template values and links outside this repository. This keeps the
published body free of community-group links, sponsorships, advertisements,
affiliate links, and referral codes. GitHub-generated release notes stay off
because an automatically appended monolingual changelog would break the
bilingual ordering. The publish job fails instead of falling back to an
unrendered or partially rendered body.

Download badges and platform icons live in `docs/assets/release/` and use
repository image URLs pinned to the release tag. Keep all six installer links,
accurate system requirements, and descriptive image alt text when updating the
table. Do not add third-party badge services or update-only MSI download buttons.
Keep the four required bilingual section names; decorative emoji may follow them.

The v0.3.0 contract's primary files (official only after approved publication):

- `usque-v0.3.0-windows-x64-v2.exe`
- `usque-v0.3.0-windows-arm64.exe`
- `usque-v0.3.0-windows-x64-v2.msi`
- `usque-v0.3.0-windows-arm64.msi`
- `usque-v0.3.0-android-arm64-v8a.apk`
- `usque-v0.3.0-android-x86_64.apk`
- `usque-v0.3.0-android-armeabi-v7a.apk`
- `usque-v0.3.0-android-universal.apk`

The two EXEs and four APKs are the user-facing installers; the two MSIs are
update payloads consumed by the signed Windows updater. In addition to these
eight primary artifacts, the public release includes `release-manifest.json`,
`SHA256SUMS`, and each artifact's SPDX SBOM. A public
release requires the usual CI, architecture, signature, package, checksum,
SBOM, and provenance checks. Protected-runner validation is optional and
non-blocking; its environments, evidence contract, and privacy boundary are
documented in [RELIABILITY_TESTING.md](RELIABILITY_TESTING.md).

## Windows package rules

These rules describe the current authoring and verification code. The Agent
file-version check and late related-product removal sequence were added after
the original v0.2.4 tag; they must not be presented as properties already
verified in that older package. User-facing applicability is recorded in
[Installation and removal](INSTALLATION.md#version-scope).

WiX is locked through `.config/dotnet-tools.json`. Windows Installer has no SemVer prerelease field, so `tool/build_windows_msi.ps1` maps a release as:

```text
MSI build = SemVer patch * 100 + beta ordinal
stable ordinal = 99
```

Stable `v0.3.0` maps to MSI ProductVersion `0.3.99` and Agent PE file version
`0.3.99.0`; `v0.3.0-beta.3` maps to `0.3.3`. The prior `v0.2.9` mapped to
`0.2.999` / `0.2.999.0`; the increased minor component makes the new stable
version higher despite its lower build component. The real SemVer stays in
ProductName and the filenames, and packaging
rejects an unversioned or mismatched Agent. Equal-version major upgrades are
enabled so a validation build can replace the same product instead of
installing a second copy under `Program Files\Usque`. WiX validation suppresses
only ICE61, which assumes upgrades must raise the version; every other standard
ICE check stays on.

The user-facing Windows artifact is a WiX Burn bundle with the repository's
native C++ setup window. It contains the signed native setup EXE, signed English
MSI and 20 language transforms. It initially uses the current Windows UI
language, permits a language change before installation, and falls back to
English for unsupported languages. The bundle suppresses the inner MSI UI;
direct MSI deployments retain the custom MSI UI. Every localized MSI is
compiled and fully ICE-validated from the same ProductCode before its transform
is generated.
`MajorUpgrade/@IgnoreLanguage` is required so a direct update MSI can replace
an installation created with any transform. The tag workflow signs the base
MSI first, builds the bundle, detaches and signs the Burn engine, reattaches it,
then signs and re-verifies the final EXE. The base MSI remains a release asset
only because the in-app updater validates and invokes MSI directly. Localized
installs set `TRANSFORMSSECURE=1` so Windows Installer retains the selected
transform for elevated major upgrades and the custom uninstall path.

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

### Package rejection checks

The release fails if any of these checks fail:

| Area | Rejected package contents or metadata |
| --- | --- |
| Binaries | Unsigned project EXE/DLL files, a signer mismatch, an unversioned or version-mismatched Agent, a modified Wintun DLL, or a missing `usque-update.exe`. |
| Payload layout | PDBs, reparse points or a 32-bit component. |
| Service and shortcuts | A wrong service command, start type or DACL; an advertised shortcut; or a missing maintenance guard. |
| Upgrade and removal | Early related-product removal, a language-sensitive upgrade row, or a wrong uninstall action/condition sequence. |
| Bundle and localization | A missing or malformed language transform, a visible duplicate Burn uninstall entry, a direct quiet-MSI uninstall registration that would leave Burn registered, or an ICE failure. |

The signed update helper uses the Agent's offline Authenticode verifier. Before
starting Windows Installer it also checks the MSI SHA-256, UpgradeCode, mapped
stable ProductVersion, summary architecture and `USQUE_UPDATE_VARIANT`.

True uninstall runs emergency WFP cleanup, journal recovery, optional
current-user data cleanup and clean-state finalization after stopping the service
and before removing its binary. A major upgrade runs only the first two actions;
it preserves user data and the machine-state directory for the replacement service.

The installer exposes `INSTALLFOLDER` and records the selected path in the
64-bit machine registry for the next major upgrade.

### Visible uninstall entry

Uninstall keeps the current user's profiles, preferences, logs, caches, and
Credential Manager records by default. Settings does not host the MSI wizard,
so the package hides the Windows Installer ARP entry (`ARPSYSTEMCOMPONENT`) and
registers `usque-uninstall.exe` as the visible uninstall command. The bundle
also sets `DisableModify=yes` and `DisableRemove=yes`, which keeps its Burn
registration out of Programs and Features instead of creating a second
uninstall route. The helper asks for confirmation in the Windows UI language,
then copies itself out of the install directory before removal begins. The
temporary Rust/Win32 window remains open while the MSI worker reports progress,
files-in-use questions, cancellation availability and the final result.
Uninstall, current-user data deletion and hidden-bundle cleanup have separate
results; an incomplete cleanup or restart requirement cannot become a false
success. Window cancellation is disabled during irreversible deletion and
registration cleanup.

### Hidden bundle cleanup

The helper resolves the hidden bundle through the architecture-specific stable Burn
provider key, requires the cached EXE to remain in its bundle-ID cache directory
and have the same Authenticode signer, uninstalls the current MSI, then runs the
cached bundle quietly so Burn removes its own registration and cache.
Windows Installer and Burn own separate per-machine elevation boundaries, so
Windows may request administrator approval for each phase; the helper itself
is never elevated from its user-writable temporary path.

### Quiet uninstall

The registered `QuietUninstallString` embeds the repository's quiet launcher
in a hidden system PowerShell host. It stages the signed helper, waits for the
installed staging process to exit, locks the copy against changes, rechecks
its signer using the installed verifier, and then waits for the temporary
worker's final exit code. Thus no installed image stays mapped during removal,
and callers receive failures and reboot requirements instead of asynchronous
success. Installation paths are passed as process filenames, never script
source; no execution-policy override is used. Direct `--quiet` from the install
directory fails closed rather than detaching. The registered command keeps
data.

### Data retention and MSI-only deployments

An administrator may request deletion explicitly; deletion covers
only that user's Usque directories and credential namespace. A direct
`msiexec /x` command is reserved for MSI-only deployments because it cannot
clean an EXE bundle registration. The shared Wintun driver package is not
removed.

### WiX argument handling

The quiet launcher's fixed executable prefix and quotes are authored in WXS.
Only its Base64 script token crosses the WiX `-define` command-line boundary;
passing the complete quoted command loses quotes under PowerShell's Legacy
native argument passing. The MSI verifier still compares the full resulting
Registry value against the trusted launcher, and CI compiles real inert MSIs
under all three PowerShell 7 argument-passing modes.

User-facing install and uninstall steps are in [INSTALLATION.md](INSTALLATION.md).

## In-app update verification

The app offers only non-prerelease GitHub Releases. It requires the exact update
MSI (Windows) or APK (Android) and `release-manifest.json` from the same release.
It streams the package into a private `.part` file, checks the declared size and
SHA-256, and atomically exposes the completed file. Cancellation and failure
remove partial downloads; abandoned update packages expire after seven days.

On Windows, **Restart and update** flushes local settings, disconnects the Engine normally, and starts the signed `usque-update.exe` helper. The helper checks the MSI digest, Authenticode signer, UpgradeCode, ProductVersion, architecture, and installed variant before waiting for the GUI to exit and running Windows Installer in passive, no-restart mode. It deletes the MSI at a terminal result and starts the installed application again unless Windows requires a reboot. Validate the real upgrade and failure-recovery paths only in a snapshot-enabled VM.

On Android, **Install update** verifies that the APK stays in Usque's private cache and has the same package name and signing identity, a higher version code, the advertised version, and native code for the running ABI. Android may first open the permission page for installing unknown apps; Usque then submits the APK with `PackageInstaller` and Android shows its normal confirmation UI. Success, failure, cancellation, package replacement, and the next startup all clean the cached APK. Validate this path only on a dedicated phone or TV.

These checks complement the user-facing [update steps](INSTALLATION.md#updates).
They do not authorize a development workstation to install or exercise a package.

## Runner isolation boundary

GitHub-hosted runners compile, test, sign, inspect, hash, inventory, attest, and
aggregate the release. They do not run an installer bundle, install an MSI, start Windows VPN/TUN,
change runner networking, or install APKs on devices.

The separate, opt-in protected self-hosted jobs perform destructive lifecycle,
independent-network, and performance testing only in the environments described
in [RELIABILITY_TESTING.md](RELIABILITY_TESTING.md). They are supplemental and
do not gate publication. Do not provision those runner labels on a developer
workstation, and do not treat an environment variable or label as proof of
isolation.
