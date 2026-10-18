# Installation and removal

Download packages from this repository's
[GitHub Releases page](https://github.com/GeorgeXie2333/usque-app/releases).

## Version scope

This is the development-branch guide. [v0.3.2](https://github.com/GeorgeXie2333/usque-app/releases/tag/v0.3.2)
is the latest published release; `Cargo.toml`, `pubspec.yaml` and the release
workflow still declare v0.3.2 / 0.3.2+26. This branch also describes
unreleased changes, including Linux packages, which v0.3.2 does not contain.
For an installed release, use its release notes and the guide at the matching
Git tag; the
[v0.3.2 installation guide](https://github.com/GeorgeXie2333/usque-app/blob/v0.3.2/docs/INSTALLATION.md)
describes that published version.

The Windows upgrade recovery fix was introduced in v0.2.5; the multilingual EXE
installer arrived in v0.2.6. The original v0.2.4 MSI does not have those fixes.
See [Upgrade](#upgrade) if that version cannot uninstall.

### Configuration compatibility when upgrading

v0.3.1 uses configuration schema 24; v0.3.0 uses 23 and v0.2.9 uses 21.
Current source uses schema 26: schema 25 adds the shared MASQUE TLS fingerprint
and schema 26 adds optional IPv6 endpoint presets. Migrated and new
configurations use the Chrome 155 fingerprint unless you had explicitly chosen
another one; the original handshake is now labeled unsafe. Older configurations
start without an IPv6 preset. Other settings migrate as before.

The newer app saves the migrated configuration on first launch, and v0.3.1 or
older cannot read it. The same applies to the Windows recovery journal (version
6). Windows rejects installer downgrades, and there is no reverse migration.

If you may need to go back to an older version, back up the old installation
and user data, including the platform-protected credentials, before upgrading,
and ask the maintainer for restore steps for your device. A copied configuration
file, diagnostic export or WARP Secret export is not a complete backup. Do not
edit `schema_version` or delete recovery records to make an older version start.
If an upgrade fails, keep your data and recovery records and install a newer
official fix.

## Choose a package

| Platform | Requirements | Package |
| --- | --- | --- |
| Windows x64 | Windows 10 2004, build 19041 or later; x86-64-v2 CPU | x64-v2 EXE |
| Windows ARM64 | Windows 10 2004, build 19041 or later, native ARM64 | ARM64 EXE |
| Linux (unreleased; not in v0.3.2) | Ubuntu 22.04+, Debian 12+, supported Fedora; systemd | amd64/arm64 AppImage, deb or rpm; see [Linux desktop](LINUX_DESKTOP.md) |
| Android / Android TV | Android 8.0, API 26 or later | APK matching the device's CPU architecture |
| Android / Android TV, architecture unknown | Android 8.0, API 26 or later | Larger universal APK containing all three architectures |

### Package names (published v0.3.2 examples)

- `usque-v0.3.2-windows-x64-v2.exe`
- `usque-v0.3.2-windows-arm64.exe`
- `usque-v0.3.2-android-arm64-v8a.apk`
- `usque-v0.3.2-android-x86_64.apk`
- `usque-v0.3.2-android-armeabi-v7a.apk`
- `usque-v0.3.2-android-universal.apk`

The release also includes `usque-v0.3.2-windows-x64-v2.msi` and
`usque-v0.3.2-windows-arm64.msi` for Usque's in-app update flow. Use the EXE for
manual Windows installation. v0.3.2 has no Linux packages; the future Linux
filenames are listed in [Linux desktop](LINUX_DESKTOP.md#choose-and-verify-a-package).

Each release includes `SHA256SUMS`, `release-manifest.json` and a software
component inventory (SPDX SBOM) for each package. Pull Request builds, local
validation packages and files from other sites are not official releases.

## Verify before installing

Download the package and `SHA256SUMS` from the same release. The examples below
use the published v0.3.2 names; substitute the exact filename and published tag you
downloaded. These commands inspect files without installing or running them.

### Check the file SHA-256

In PowerShell, open the folder containing the download and run:

```powershell
$package = '.\usque-v0.3.2-windows-x64-v2.exe'
Get-FileHash -LiteralPath $package -Algorithm SHA256
```

For an APK, set `$package` to its filename instead. Compare the complete
64-character `Hash` with the entry for that exact filename in `SHA256SUMS`
and with the digest GitHub shows for that release asset. Hexadecimal letter case
does not matter. A difference means you must stop and download again from the
official release.

### Check the Windows signer

After the file-hash check, run this against the Windows EXE:

```powershell
$signature = Get-AuthenticodeSignature -LiteralPath $package
$signature | Format-List Status, StatusMessage
if ($null -eq $signature.SignerCertificate) {
    throw 'The package has no readable signing certificate.'
}
$certificateHasher = [System.Security.Cryptography.SHA256]::Create()
try {
    $certificateHash = $certificateHasher.ComputeHash($signature.SignerCertificate.RawData)
    [System.BitConverter]::ToString($certificateHash).Replace('-', '')
} finally {
    $certificateHasher.Dispose()
}
```

Compare the last line with **Windows Authenticode certificate SHA-256** in that
release's notes. This hashes the certificate's DER bytes. It is different from
the package hash and from the certificate's usual SHA-1 `Thumbprint` field.
[Microsoft's signature command reference](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.security/get-authenticodesignature)
describes the signature information returned by the command.

Pre-1.0 packages use the project's fixed self-signed certificate, so the
expected `$signature.Status` is `UnknownError`, with a `StatusMessage` saying
the certificate chain ends in an untrusted root. `Valid` is also fine if
Windows already trusts the signature. In both cases the package hash and the
certificate SHA-256 must match the release. Stop for any other status or
message (such as `NotTrusted`, `HashMismatch` or `NotSigned`) or a different
signer certificate. Do not import the certificate into Root or Trusted
Publisher to hide the warning. The identity policy is in
[Code signing](CODE_SIGNING.md).

### Check the Android signer

On a computer with Java and Android SDK Build Tools installed, use
[apksigner](https://developer.android.com/tools/apksigner) to verify the APK and
print its certificate. Replace the tool path with your installed Build Tools
directory:

```powershell
$apksignerPath = 'C:\path\to\Android\Sdk\build-tools\<version>\apksigner.bat'
& $apksignerPath verify --verbose --print-certs '.\usque-v0.3.2-android-arm64-v8a.apk'
if ($LASTEXITCODE -ne 0) { throw 'APK signature verification failed.' }
```

The command must succeed. Compare **Signer #1 certificate SHA-256 digest** with
**Android release certificate SHA-256** in the release notes. Compare the
certificate digest, not a public-key digest. Check the APK file hash separately
as described above. You can then copy that verified file to the Android device.

### Check the build provenance

If you have GitHub CLI, verify the attestation for the same downloaded file:

```powershell
gh attestation verify $package --repo GeorgeXie2333/usque-app --source-ref refs/tags/v0.3.2 --signer-workflow GeorgeXie2333/usque-app/.github/workflows/release.yml
if ($LASTEXITCODE -ne 0) { throw 'Build provenance verification failed.' }
```

This checks the file against the repository, source tag and release workflow
that produced its attestation. See the
[GitHub CLI reference](https://cli.github.com/manual/gh_attestation_verify) for
authentication and verification options. A missing attestation does not count
as a pass; do not use a file that reports a mismatch.

Do not run a package that asks you to disable antivirus, the firewall or endpoint
public-key checks. Stop if the filename, version, architecture, hash or signer
does not match the release.

## Windows

1. Choose the EXE for native x64 or ARM64 Windows and complete the checks above.
2. Run it, read the license agreement and select its acceptance checkbox. The
   first page shows the version and installation folder. Use **Change
   installation folder** to choose another location, or **Language** to change
   the installer's language. Usque's app language is configured separately.
3. Choose **Install** and approve Windows' administrator request. Progress and
   the result stay in the same installer window.
4. On **Installation complete**, optionally select **Create a desktop shortcut**
   (off by default). **Open Usque after setup** is selected by default. Under
   **More options**, **Start Usque when I sign in to Windows** reads your existing
   setting and is off for a new user. These choices affect only the Windows user
   who opened the installer. Choose **Finish** or **Finish and open**.

If an optional setting fails, Usque remains installed. The page identifies the
unfinished setting and offers a retry or **Skip and finish**. A shortcut owned
by another application is not overwritten. You can always open Usque from the
Start Menu and complete first-run setup. Installation itself does not start a
VPN or change your saved automatic-connection preference.

If installation requires a restart, desktop and login-startup choices remain
available. Opening Usque is disabled until Windows restarts. Save your work
before choosing **Restart now**, or choose **Restart later**.

Usque installs a separate Agent service for privileged network operations.
After installation, the app can start it without another UAC prompt. Ordinary
disconnect restores that connection's network settings and keeps Usque's virtual
adapter for reuse. Full app exit starts adapter removal; Windows can take time
to finish it. See [Troubleshooting](#troubleshooting) if a restart or recovery fails.

### Tray and keyboard controls

The tray icon shows amber while connecting or reconnecting, green when
connected, red on error and no status dot while disconnected. Right-click it to
open Usque, connect or disconnect, or change **TUN** and **System proxy**.
These switches use the same save and apply behavior as Home; system proxy is
unavailable while the HTTP local proxy
is off. **Disconnect and Exit** closes the app and starts adapter removal.

While the window is hidden or in the background, notifications report a session
that stays in reconnecting for five seconds, a connection error, and recovery
after an interruption that was announced. Brief reconnections and manual
disconnects stay silent. A notification names an active Kill Switch but does
not include Engine error details. Windows quiet hours still apply; clicking a
notification opens Usque. The app remembers its window position, size and
maximized state.

| Shortcut | Action |
| --- | --- |
| **Ctrl+1**, **Ctrl+2**, **Ctrl+3**, **Ctrl+4** | Open Home, Accounts, Proxy or Settings. |
| **Esc** outside a text field, **Alt+Left**, mouse back button | Leave a subpage; unapplied changes still require a choice. |
| **Ctrl+S** | Apply changes through the visible apply bar when available. |
| **F5** | Refresh the VPN Gate list or diagnostics timeline on that page. |

Apply and refresh shortcuts pause while their page is covered by a dialog or
popup. Section changes still check for unapplied edits. These shortcuts do not
apply to Android or Android TV.

### Upgrade

Use the app's [update flow](#updates), or run a verified newer official EXE.
The installer asks Usque to disconnect and exit, including when closing its
window would normally leave it in the tray. An unresponsive older process may
be forcibly closed after the installer's timeout.

Upgrades keep the install directory, accounts, credentials, settings, logs,
caches and recovery records. Downgrades are rejected. Same-version replacement
also replaces equal-version and unversioned application files together, so the
GUI, Engine and Agent stay in sync.

If v0.2.4 cannot uninstall, upgrade with a verified official Windows package
from v0.2.5 or later, then uninstall the newer version if removal is your goal. The newer
Agent can recover state that the older package could not clean up. If recovery
still fails, stop and report the error with sanitized diagnostics. Do not delete
the Agent, recovery journal or Windows network objects to bypass the failure.

An upgrade stops with an error if it cannot restore privileged network state.
The implementation and recovery ordering are documented in
[Windows lifecycle](windows-lifecycle.md#upgrade-ordering-and-payload-replacement).

### Uninstall

1. Open **Settings → Apps → Installed apps**, or **Programs and Features**, and
   choose Usque's uninstall action.
2. Leave **Also delete my local data** unchecked to keep your accounts, profiles,
   settings, logs, caches and saved credentials for a later reinstall. Selecting
   it changes the final button to **Uninstall and delete data** and permanently
   deletes only the current Windows user's Usque data.
3. Choose **Uninstall** and keep the window open until it shows the result.
   Windows may ask for administrator approval separately for removal and
   installer-record cleanup. The window reports each stage; when a stage cannot
   be cancelled, wait for it to finish.

You can also reopen the installer EXE and choose **Uninstall** to use the same
window. If the program was removed but its installer records could not be
cleaned up, **Retry** repeats only that cleanup. Other failures return to the
confirmation page after checking the installed state; deleting personal data
is never automatically retried. A deletion that already started may have
removed some data even when uninstall later fails. **View details** shows the
stage and error code, and **Save details** writes only that limited report.

Uninstall disconnects Usque, restores its route, DNS, proxy and firewall state,
and removes its virtual adapter, service and program files. The shared Wintun
driver package stays because another app may use it. Recovery failure stops
uninstall; report the error rather than manually deleting network resources.

Repair, Modify and Patch are unsupported. To reinstall, use a supported major
upgrade or uninstall and reinstall with data deletion left off. Running the EXE
again also offers removal with the same default-off data-deletion option.

Administrators should use the registered quiet uninstall command. Direct MSI
removal cannot clean an EXE bundle's registration; see
[administrator automation](windows-lifecycle.md#uninstall-and-administrator-automation).

If the result says Windows must restart, choose **Restart later** or **Restart
now**. The latter first asks you to save your work. Windows may still ask you
to close applications with unsaved work; the uninstaller does not force them
to close.

## Linux

Linux packages are not part of any published release yet, including v0.3.2.
Once published, use the [Linux desktop guide](LINUX_DESKTOP.md) to select one installation
format, verify the Ed25519 manifest, install the service and unlock the session
keyring. Updates require confirmation; removal restores Usque's network changes
before deleting the service and keeps personal data.

## Android and Android TV

Choose arm64-v8a for ARMv8, x86_64 for x64, or armeabi-v7a for ARMv7. If you do
not know the architecture, use the universal APK. Check its hash and signer
before installing or upgrading.

The app is distributed outside Google Play. Android may ask you to allow
installation from the browser or file manager you used to open the APK.
Official packages use the package name `io.github.georgexie2333.usque` and the
release signing certificate published in the release notes. The project
registers that pair through
[Android developer verification](https://developer.android.com/developer-verification);
see [Code signing](CODE_SIGNING.md#android-developer-verification). Registration
only identifies the developer and key; it is not a Google Play review, and
Android can still show sideloading prompts.

First-run setup requests only optional notification permission. VPN consent is
deferred until a connection needs VPN output; local-proxy-only operation does
not request it. Granting Android's VPN authorization may disconnect another
active VPN. Setup checks interrupted account operations before registering
again; use **Check result** or **Continue with saved account** when offered.

After setup, choose outputs in **Proxy → VPN and local proxies**, then connect
from Home. Proxy-only operation does not start a VPN. If Android has revoked VPN
consent, enabling VPN output requests it again.

### Quick Settings control

Add Usque's tile in Android's Quick Settings editor. Opening Quick Settings asks
the VPN service for current state; tapping the tile toggles the VPN frontend
without opening the app. While the tile shows a checking or working state you
can tap it again, for example after the service restarted. The tile reflects
the service's actual state, which can differ from what you tapped. If it cannot
reach the service, it opens the app to handle permissions or errors. The tile
does not replace Always-on VPN and system blocking.

### Keep apps blocked when the VPN ends

The in-app Kill Switch blocks traffic while Usque connects, reconnects or
switches a chain exit, as long as the VPN service is running. It cannot keep
blocking once the VPN process ends; without Android's system blocking, apps
then use the network directly. Chain-specific behavior is in the
[chain protection rules](CHAIN_PROXY.md).

Open **Settings → Connection & protection → Open Always-on VPN settings**. Enable
both **Always-on VPN** and **Block connections without VPN**.

To connect automatically after a reboot, also enable **Start Usque when you
sign in** under **Settings → Application** (on Android it starts Usque after the
device restarts) and **Connect the current account automatically on start**
under **Connection & protection**.

### Per-app proxy

This setting is shared across accounts and takes effect when the VPN is on.
When off, every app uses the VPN. When on, only checked apps use it; newly
installed apps must be selected. **Select all** checks the apps currently shown
and does not disable the filter. Usque itself is not listed.

With **Block connections without VPN** enabled, unchecked apps are blocked
instead of using the network directly.

### Remove the app

Android removes Usque's private data and Keystore entries during uninstall.
You can export a Consumer WARP® Secret beforehand, but Usque does not accept new
Secret imports, so that export cannot restore the account in Usque after a
reinstall. Secrets are excluded from diagnostics and ordinary settings backups.

## Updates

With automatic checks enabled, Usque checks once when a new app process starts.
Returning from the background or reopening the window does not trigger another
check. **Check now** always makes a live request. Only non-prerelease GitHub
Releases are offered.

The Settings page shows the version, architecture and size before downloading.
Choose **Download** to begin; it can be cancelled and retried. The app checks the
package against the same release's manifest, size, hash and signing requirements.
Failed and partial downloads are removed; abandoned packages expire after seven days.

- Windows: choose **Restart and update**. Usque saves settings and disconnects,
  then the platform installer runs without forcing a reboot. The app restarts
  after installation unless Windows requires a reboot.
- Linux: choose the update action and authorize installation after verification.
  The package format stays the same; apt/dnf installs deb/rpm, and AppImage
  replacement coordinates the matching background service.
- Android: choose **Install update** and accept Android's installer confirmation.
  Android may first ask for permission to install unknown apps.

Updates are not installed without confirmation. Detailed package checks are in
[the update verification reference](RELEASE.md#in-app-update-verification).

<a id="direct-country-dns-privacy"></a>

## Direct-country and custom-domain DNS privacy

Country-based and custom-domain direct rules are optional. System DNS sends
matching domain queries to the current network's DNS servers outside the VPN;
DoH and DoT send them to your chosen encrypted resolver without a plaintext
fallback.

Other remote VPN queries use the WARP tunnel or the selected final chain exit.
Configure WARP Plain DNS, DoH or DoT in **Settings → Advanced network settings →
WARP DNS**, then **Apply changes**; changing a connected session's DNS reconnects
it. These settings do not replace the final chain exit's DNS policy.
HTTP/SOCKS5 chain DNS defaults to verified Cloudflare® DoH through that proxy;
explicit custom or non-default inherited DNS uses TCP. Application-selected
UDP/53 queries through these exits use TCP to the selected resolver without a
physical DNS fallback. Apps
with their own encrypted DNS hide names from Usque, which then classifies
destinations by IP. See [WARP exit DNS](WARP_DNS.md),
[chain DNS choices](CHAIN_PROXY.md#http-and-socks5-exits--http-与-socks5-出口) and
[Direct DNS](encrypted-direct-dns.md) for setup and limitations.

Rule downloads can start while disconnected, but still obey Android Lockdown
and any remaining Windows Kill Switch. A blocked download can be retried and does
not replace a valid cached ruleset.

## Troubleshooting

| Problem | What to do |
| --- | --- |
| Windows shows an unknown publisher | Check the official file hash and certificate SHA-256. The fixed self-signed certificate is expected before v1.0; do not add it to a trust store. |
| A Usque adapter remains after disconnect | It is retained for reuse. Fully exit the app to start removal. |
| A connection reports recovery or adapter-removal failure | Keep the recovery journal. Report the error with sanitized diagnostics; do not remove services or network objects manually. |
| Android apps cannot connect with per-app filtering | Check the selected apps and system blocking settings. Unselected apps are blocked when Block connections without VPN is on. |
| A setting is saved but not active | Follow the pending-state message. Some changes apply only on the next manual connection. |

Use [Network Doctor](network-doctor.md) for local checks. Keep credentials and
raw diagnostic bundles out of public Issues; use [Security policy](../SECURITY.md)
for suspected vulnerabilities.

---

WARP is a trademark and/or registered trademark of Cloudflare, Inc. in the United States and other jurisdictions.
