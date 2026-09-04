# Installation and removal

Install only packages from this repository's [GitHub Releases page](https://github.com/GeorgeXie2333/usque-app/releases) for `v0.2.4`.

## Official packages

- `usque-v0.2.4-windows-x64-v2.msi`
- `usque-v0.2.4-windows-arm64.msi`
- `usque-v0.2.4-android-arm64-v8a.apk`
- `usque-v0.2.4-android-x86_64.apk`
- `usque-v0.2.4-android-armeabi-v7a.apk`
- `usque-v0.2.4-android-universal.apk`

The GitHub Release attaches those six packages plus `release-manifest.json`,
`SHA256SUMS`, and each package's SPDX SBOM. GitHub shows a SHA-256 for each
asset; signer fingerprints remain in the release notes, and build provenance
remains available through GitHub attestations. Optional protected-runner
summaries, restricted packet captures, and raw lab evidence are CI artifacts
rather than required public release assets. A local validation package, Actions
development output, fork artifact, or a file from somewhere else is not an
official release.
Official identities and fingerprint rules are in
[CODE_SIGNING.md](CODE_SIGNING.md).

## Verify before installing

1. Download the package from the [GitHub Releases page](https://github.com/GeorgeXie2333/usque-app/releases).
2. Calculate the package SHA-256 and compare it with both `SHA256SUMS` and the digest GitHub shows for that asset.
3. Compare the package signer with the fingerprint in the release notes.
4. Check the GitHub artifact attestation when it is available.
5. Stop if the filename, hash, signature, architecture, or version differs.

Never disable endpoint pinning, import signing certificates from an unofficial package, or run a package that asks you to turn off antivirus or the firewall.

## Direct-country DNS privacy

Direct-country rules are optional and downloaded separately. Per-country GeoIP data is combined with one global V2Fly GeoSite catalog. Downloads may be started while Usque is disconnected, but they still obey Android Lockdown and any Windows Kill Switch state; a real system block appears as a retryable network failure and never replaces a valid cached ruleset.

With direct-country routing enabled, Usque identifies GeoSite matches before sending DNS. System (the default) sends matching domains to the physical DNS provider. An explicit DoH/DoT choice sends them to the configured TLS-authenticated resolver using numeric bootstrap; failures do not fall back to plaintext. Non-matching domains continue through the configured WARP DNS. Application-owned encrypted DNS does not expose QNAME to Usque and is classified by GeoIP. This applies to Android VPN, Windows TUN and known SOCKS5/HTTP hostnames. Desktop proxy mode is not a VPN Kill Switch; its ordinary host-network boundary is documented in the [direct DNS threat model](direct-dns-threat-model.md).

## Windows

The release needs Windows 10 22H2 build 19045 or later. Use the x64-v2 MSI on native x64 Windows and the ARM64 MSI on native ARM64 Windows.

The pre-1.0 MSI uses a fixed self-signed Authenticode identity, so Windows may show an unknown-publisher warning. The installer does not add that certificate to the machine Root or Trusted Publisher stores. Confirm the certificate SHA-256 from the release notes before accepting the warning.

The interactive installer:

- asks for administrator approval to install the `usque-agent` service;
- lets you choose the install directory;
- installs the GUI, unprivileged engine, Agent, official Wintun DLL, and Start Menu shortcut;
- keeps that directory on a major upgrade;
- installs the Agent as a demand-start service and does not leave it running;
- does not start a VPN during install.

After installation, an interactive Windows user can start the Agent through
Usque without another UAC prompt. The service ACL grants that user only start
and status-query access; stopping, deleting, or reconfiguring the service still
requires an administrator. The Agent starts when the Engine first needs a
privileged operation and exits after the recovery journal has been clean, with
no clients or recovery jobs, for 10 seconds.

The service temporarily changes itself to automatic start before Usque records
or applies privileged network state. This lets the next boot recover an
interrupted VPN or system-proxy transaction. At startup it verifies the exact
adapter identity and the network resources needed for reattachment. A surviving
tunnel, or a lost Engine lease, gets a 30-second reattachment window. Missing
resources are recovered instead of being treated as a live tunnel. If no Engine
returns, the Agent restores Usque network state, changes back to demand start,
and exits.

On confirmed shutdown/restart the Agent stops admitting operations, stops packet
forwarding, and restores network state within a 30-second service preshutdown
budget. Ordinary service stops retain the existing maintenance/reattachment
behavior. Interrupted or failed cleanup keeps its journal for the next start.

`RecoveryRequired` keeps the Agent available and automatic. Starting a connection
first makes at most one authenticated, operation- and generation-checked recovery
attempt, before DNS or VPN startup. It never recovers an active session or another
user's transaction. Failed or timed-out recovery does not start a new tunnel;
the journal is retained and the app displays a recovery-specific error. Older
Agents without guarded recovery support require a matching application/Agent
update, not a fallback to unguarded maintenance recovery. Do not delete the
recovery journal to bypass an error.

### Upgrade

A running Usque process is asked to disconnect and exit through Windows Restart
Manager before any installed files are replaced. Usque treats that maintenance
request differently from an ordinary window close, so the close-to-tray setting
does not keep the process alive. If an older or unresponsive build cannot honor
the request, Windows Installer uses its bounded force-shutdown fallback; it does
not restart that process during the upgrade.

A major upgrade first stops the Agent and restores Usque-owned WFP, route, DNS, system-proxy, and Wintun state. It keeps user profiles, settings, logs, caches, Credential Manager identities, and the recovery journal the new version needs.

If privileged network state cannot be restored, the upgrade stops with an error. It must not continue with leftover routes, filters, DNS, proxies, or adapters.

### Uninstall

Uninstall from **Settings > Apps > Installed apps** or the classic Programs and Features panel. Settings launches `usque-uninstall.exe`, which asks for confirmation and offers an unchecked option to delete only the current user's Usque profiles, preferences, logs, caches, and Credential Manager entries. Cancel leaves the product installed.

Confirming Uninstall starts Windows Installer, which then:

1. asks the GUI and Engine to disconnect and exit, with a bounded force fallback for an unresponsive older build;
2. stops the Agent;
3. removes Usque WFP Kill Switch objects;
4. restores journaled routes, DNS, and system-proxy state;
5. removes the Usque-owned Wintun adapter;
6. removes the service, program files, shortcut, and clean machine journal.

The shared Wintun driver package stays, because another application may use it. A successful uninstall must not leave an Usque Wintun adapter.

The data-deletion option cannot be undone and does not affect other Windows users. Leave it unchecked to keep local data for a later reinstall. Silent uninstall (`msiexec /x {ProductCode} /qn`, or the registered `QuietUninstallString`) also keeps data unless an administrator sets `USQUE_REMOVE_USER_DATA=1`. Upgrades never show the confirmation dialog and never purge user data. Re-running the MSI while Usque is installed still offers the same default-off deletion checkbox on the maintenance remove path.

MSI Repair, Modify, and Patch are not supported, and the Start Menu shortcut is
non-advertised so launching it cannot trigger MSI self-repair. Repair could stop
the Agent or overwrite the crash-recovery start mode while privileged network
state is active. The installer explains this if Repair is selected, and
command-line maintenance is rejected before `StopServices`. Use the supported
major-upgrade path, or uninstall and reinstall while leaving the data-deletion
checkbox off.

If recovery fails, uninstall stops rather than leaving privileged network
residue behind. Windows install, recovery, upgrade, connected-uninstall, and
platform-state restoration tests belong on a snapshot VM. Externally observed
IPv4, IPv6, DNS, Kill Switch, and route leak tests belong on the independent
controlled-network observer. Neither belongs on a daily-driver machine.
Development-machine limits are in [CONTRIBUTING.md](../CONTRIBUTING.md).

## Android and Android TV

The release needs Android 8.0 / API 26 or later, including compatible Android TV devices. Use the arm64-v8a package on ARMv8, x86_64 on x64, or armeabi-v7a on ARMv7. The universal APK contains all three ABIs and is larger; use it only when the per-ABI package cannot be determined.

The pre-1.0 APK is signed by a fixed, project-controlled certificate and is not on Google Play. Android may require a manual install or ADB. Check the APK signing-certificate SHA-256 before install or upgrade.

The package name `io.github.georgexie2333.usque` and current official Android release certificate are registered to a verified developer identity through [Android developer verification](https://developer.android.com/developer-verification). This registration confirms developer identity and signing-key ownership; it does not put Usque on Google Play or mean that Google reviewed the app's content. Android may still require approval for the installation source or another sideloading confirmation.

Android asks for VPN consent only when VPN output is first enabled. SOCKS5 and HTTP-only modes do not request `VpnService`. The in-app Kill Switch keeps the VPN interface up while Usque is connecting, reconnecting, or recovering so other apps stay on the tunnel. That does not survive the app process being killed. Open **Settings → System integration → Open Always-on VPN settings** and enable both **Always-on VPN** and **Block connections without VPN**. Boot recovery needs both **Start Usque after reboot** in that panel and **Connect this Profile automatically** on the active profile.

**Per-app proxy** is an Android app setting, not part of a Profile. When it is off, every app uses the VPN tunnel. When it is on, only the apps you check use the tunnel; newly installed apps stay off the tunnel until you select them. Select all checks the apps currently visible in the picker — it does not turn the filter off. Usque itself is never listed. If Always-on VPN and **Block connections without VPN** are enabled, apps you did not select are blocked instead of going around the tunnel. The filter applies only while VPN output is on.

Uninstalling the app removes its Android Keystore entries and private data the way Android usually does. Export a WARP Secret before uninstalling if you want to keep that identity. Secrets never appear in diagnostics or ordinary settings backups.

## Updates

When automatic checks are enabled, each new Usque application process checks once after local state initialization. Returning from the background or reopening the window does not check again. Turning the switch off disables that startup check; **Check now** always performs a live request.

Only a non-prerelease GitHub Release is offered. Usque does not download it in the background: the Settings page first shows the release version, architecture, and package size. After you choose Download, Usque requires the exact package and `release-manifest.json` from the same release, streams into a private `.part` file, checks the declared size and SHA-256, and atomically publishes the completed file. Downloads can be cancelled and retried. Failed and partial downloads are removed immediately; abandoned update packages are removed after seven days.

On Windows, **Restart and update** flushes local settings, disconnects the Engine normally, and starts the signed `usque-update.exe` helper. The helper checks the MSI digest, Authenticode signer, UpgradeCode, ProductVersion, architecture, and installed variant before waiting for the GUI to exit and running Windows Installer in passive, no-restart mode. It deletes the MSI at a terminal result and starts the installed application again unless Windows requires a reboot. Validate the real upgrade and failure-recovery paths only in a snapshot-enabled VM.

On Android, **Install update** verifies that the APK stays in Usque's private cache and has the same package name and signing identity, a higher version code, the advertised version, and native code for the running ABI. Android may first open the permission page for installing unknown apps; Usque then submits the APK with `PackageInstaller` and Android shows its normal confirmation UI. Success, failure, cancellation, package replacement, and the next startup all clean the cached APK. Validate this path only on a dedicated phone or TV.

Treat each update as a new package: the app performs these checks automatically, while the release page remains available for independent filename, SHA-256, signer, architecture, SBOM, and attestation review.

The GitHub release workflow does not install packages on physical devices or run long VPN tests. Signing and supply-chain steps are in [RELEASE.md](RELEASE.md).
