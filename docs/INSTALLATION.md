# Installation and removal

Usque has not published a public beta. This document defines the installation and removal behavior that the first beta must meet; it is not an invitation to install an arbitrary repository build.

## Official packages

Official packages will be attached only to this repository's GitHub prerelease for the exact version tag:

- `usque-v0.1.1-beta.2-windows-x64-v2.msi`
- `usque-v0.1.1-beta.2-android-arm64-v8a.apk`

Each release must also provide SHA-256 sidecars, signer fingerprints, SPDX and CycloneDX SBOMs, dependency license inventories, GitHub build provenance, and laboratory evidence. A local validation package, Actions development output, fork artifact, or file shared elsewhere is not an official release.

## Verify before installing

1. Download the package and its `.sha256` file from the same GitHub prerelease.
2. Calculate the package SHA-256 and compare the complete 64-character value.
3. Compare the package signer with the fingerprint published in the release notes.
4. Verify the GitHub artifact attestation when it is available.
5. Stop if the filename, hash, signature, architecture, or version differs.

Never disable endpoint pinning, import signing certificates from an unofficial package, or run a package that asks you to disable antivirus or firewall protection.

## Windows

The first beta targets Windows 10 22H2 build 19045 or later on x86-64-v2 processors.

The MSI uses a stable self-signed Authenticode identity, so Windows may show an unknown-publisher warning. The installer does not add that certificate to the machine Root or Trusted Publisher stores. Confirm the exact certificate SHA-256 from the release notes before accepting the warning.

The interactive installer:

- requires administrator approval to install the narrow `usque-agent` service;
- lets the user select the installation directory;
- installs the GUI, unprivileged engine, Agent, official Wintun DLL, and Start Menu shortcut;
- preserves the selected directory during a major upgrade;
- never starts a VPN connection as part of installation.

### Upgrade

A major upgrade first stops the Agent and restores Usque-owned WFP, route, DNS, system-proxy, and Wintun state. It preserves user profiles, settings, logs, caches, Credential Manager identities, and the clean recovery journal required by the replacement version.

The upgrade must abort with a detailed error if privileged network state cannot be restored. It must not silently continue with stale routes, filters, DNS settings, proxies, or adapters.

### Uninstall

Uninstall Usque from **Settings > Apps > Installed apps** or the classic Programs and Features panel. The MSI performs recovery before deleting the binaries:

1. stop the Agent;
2. remove Usque WFP Kill Switch objects;
3. restore journaled routes, DNS, and system-proxy state;
4. remove the exact Usque-owned Wintun adapter;
5. remove the service, program files, shortcut, and clean machine journal.

The shared Wintun driver package is not removed because another application may use it. No Usque-owned adapter may remain after a successful uninstall.

User data is preserved by default. The uninstall UI offers an unchecked option to delete only the uninstalling user's Usque profiles, preferences, logs, caches, and Credential Manager entries. This action is irreversible and does not affect other Windows users. Silent uninstall also preserves data unless an administrator explicitly supplies `USQUE_REMOVE_USER_DATA=1`; upgrades never purge user data.

If recovery fails, uninstall stops rather than concealing privileged network residue. Recovery and leak testing must take place only in a snapshot-enabled isolated VM with out-of-band access.

## Android and Android TV

The first beta targets Android 8.0 / API 26 or later on arm64-v8a devices, including compatible Android TV devices.

The APK is signed by a fixed release keystore but is not distributed through Google Play. Android may require an advanced sideload flow or ADB. Verify the APK signing certificate SHA-256 before installation or upgrade.

Android requests VPN consent only when VPN output is first enabled. SOCKS5 and HTTP-only modes do not request `VpnService` permission. For process-level leak protection after the application is terminated, enable Android **Always-on VPN** and **Block connections without VPN** in system settings.

Uninstalling the application removes its Android Keystore entries and application-private data according to Android platform behavior. Export a WARP Secret explicitly before uninstalling only if you intend to retain that identity; Secrets never appear in diagnostics or ordinary settings backups.

## Updates

Usque never downloads or installs an update automatically. At most once every 24 hours, the application can check this repository for a newer prerelease, display a notification, and open the release page. Automatic checks can be disabled, and a manual check remains available.

Treat each update as a new package: verify its filename, hash, signer, architecture, and release evidence before installation.

## Development-host boundary

Do not install a generated MSI, start Windows VPN mode, create a TUN/Wintun session, or apply WFP, route, DNS, or system-proxy mutations on an ordinary development host. Safe local operations are source checks, compile-only builds, MSI table/ICE validation, `usque-agent --validate-only`, and SOCKS5/HTTP loopback testing.

See [Release process](RELEASE.md) for the protected build and laboratory contract.
