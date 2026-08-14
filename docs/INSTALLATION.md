# Installation and removal

Install only packages from this repository's GitHub release for `v0.1.2`.

## Official packages

- `usque-v0.1.2-windows-x64-v2.msi`
- `usque-v0.1.2-windows-arm64.msi`
- `usque-v0.1.2-android-arm64-v8a.apk`
- `usque-v0.1.2-android-x86_64.apk`
- `usque-v0.1.2-android-armeabi-v7a.apk`
- `usque-v0.1.2-android-universal.apk`

Each release also has SHA-256 sidecars, signer fingerprints, SPDX and CycloneDX SBOMs, dependency license inventories, and GitHub build provenance. A local validation package, Actions development output, fork artifact, or a file from somewhere else is not an official release.

## Verify before installing

1. Download the package and its `.sha256` file from the same GitHub release.
2. Calculate the package SHA-256 and compare the full 64-character value.
3. Compare the package signer with the fingerprint in the release notes.
4. Check the GitHub artifact attestation when it is available.
5. Stop if the filename, hash, signature, architecture, or version differs.

Never disable endpoint pinning, import signing certificates from an unofficial package, or run a package that asks you to turn off antivirus or the firewall.

## Windows

The release needs Windows 10 22H2 build 19045 or later. Use the x64-v2 MSI on native x64 Windows and the ARM64 MSI on native ARM64 Windows.

The pre-1.0 MSI uses a fixed self-signed Authenticode identity, so Windows may show an unknown-publisher warning. The installer does not add that certificate to the machine Root or Trusted Publisher stores. Confirm the certificate SHA-256 from the release notes before accepting the warning.

The interactive installer:

- asks for administrator approval to install the `usque-agent` service;
- lets you choose the install directory;
- installs the GUI, unprivileged engine, Agent, official Wintun DLL, and Start Menu shortcut;
- keeps that directory on a major upgrade;
- does not start a VPN during install.

### Upgrade

A major upgrade first stops the Agent and restores Usque-owned WFP, route, DNS, system-proxy, and Wintun state. It keeps user profiles, settings, logs, caches, Credential Manager identities, and the recovery journal the new version needs.

If privileged network state cannot be restored, the upgrade stops with an error. It must not continue with leftover routes, filters, DNS, proxies, or adapters.

### Uninstall

Uninstall from **Settings > Apps > Installed apps** or the classic Programs and Features panel. Settings launches `usque-uninstall.exe`, which asks for confirmation and offers an unchecked option to delete only the current user's Usque profiles, preferences, logs, caches, and Credential Manager entries. Cancel leaves the product installed.

Confirming Uninstall starts Windows Installer, which then:

1. stops the Agent;
2. removes Usque WFP Kill Switch objects;
3. restores journaled routes, DNS, and system-proxy state;
4. removes the Usque-owned Wintun adapter;
5. removes the service, program files, shortcut, and clean machine journal.

The shared Wintun driver package stays, because another application may use it. A successful uninstall must not leave an Usque Wintun adapter.

The data-deletion option cannot be undone and does not affect other Windows users. Leave it unchecked to keep local data for a later reinstall. Silent uninstall (`msiexec /x {ProductCode} /qn`, or the registered `QuietUninstallString`) also keeps data unless an administrator sets `USQUE_REMOVE_USER_DATA=1`. Upgrades never show the confirmation dialog and never purge user data. Re-running the MSI while Usque is installed still offers the same default-off deletion checkbox on the maintenance remove path.

If recovery fails, uninstall stops rather than leaving privileged network residue behind. Recovery and leak testing belong on a snapshot VM, not on a daily-driver machine. Development-machine limits are in [CONTRIBUTING.md](../CONTRIBUTING.md).

## Android and Android TV

The release needs Android 8.0 / API 26 or later, including compatible Android TV devices. Use the arm64-v8a package on ARMv8, x86_64 on x64, or armeabi-v7a on ARMv7. The universal APK contains all three ABIs and is larger; use it only when the per-ABI package cannot be determined.

The pre-1.0 APK is signed by a fixed, project-controlled certificate and is not on Google Play. Android may require a manual install or ADB. Check the APK signing-certificate SHA-256 before install or upgrade.

Android asks for VPN consent only when VPN output is first enabled. SOCKS5 and HTTP-only modes do not request `VpnService`. For process-level leak protection after the app is killed, enable **Always-on VPN** and **Block connections without VPN** in system settings.

Uninstalling the app removes its Android Keystore entries and private data the way Android usually does. Export a WARP Secret before uninstalling if you want to keep that identity. Secrets never appear in diagnostics or ordinary settings backups.

## Updates

Usque never downloads or installs an update by itself. At most once every 24 hours it can check this repository for a newer release, show a notification, and open the release page. Automatic checks can be turned off; a manual check stays available.

Treat each update as a new package: check the filename, hash, signer, architecture, manifest, and attestations before installing.

The GitHub release workflow does not install packages on physical devices or run long VPN tests. Signing and supply-chain steps are in [RELEASE.md](RELEASE.md).
