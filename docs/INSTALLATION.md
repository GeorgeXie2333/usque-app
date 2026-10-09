# Installation and removal

Download packages from this repository's
[GitHub Releases page](https://github.com/GeorgeXie2333/usque-app/releases).

## Version scope

This guide covers v0.3.0. `Cargo.toml` and `apps/usque_gui/pubspec.yaml` declare
application version 0.3.0. The version and package names below do not by
themselves establish that a release has been published. The
[v0.3.0 readiness review](RELEASE_V0.3.0_READINESS.md) retains its historical
source and evidence limits. For an installed release, use its release notes and
the guide at the matching Git tag.

The Windows upgrade recovery fix was introduced in v0.2.5; the multilingual EXE
installer arrived in v0.2.6. The original v0.2.4 MSI does not have those fixes.
See [Upgrade](#upgrade) if that version cannot uninstall.

### Configuration compatibility when upgrading

v0.2.9 uses configuration schema 21. The current source uses schema 24,
adding unified routing rules and Ads to the WARP encrypted DNS and account-specific
Zero Trust endpoint settings introduced in schemas 22 and 23. Opening the newer app migrates and saves older configuration. The
older app rejects a newer schema, so installing v0.2.9 again cannot restore
access to migrated data. Windows also rejects installer downgrades. No reverse
configuration migration or WARP Secret import is provided.

If returning to an older version is essential, arrange a recoverable pre-upgrade
backup of the old installation and user data, including the platform-protected
credentials, before upgrading. Confirm the device-specific restoration steps
with the maintainer; a copied configuration file, diagnostic export or WARP
Secret export is not a complete account or rollback backup. Do not lower
`schema_version`, replace migration backups, or delete recovery records to make
an older app accept the data. For a failed upgrade, keep the data and recovery
records and use a newer official fix or obtain version-specific recovery
guidance; do not assume that reinstalling an older package is supported.

## Choose a package

| Platform | Requirements | Package |
| --- | --- | --- |
| Windows x64 | Windows 10 22H2, build 19045 or later; x86-64-v2 CPU | x64-v2 EXE |
| Windows ARM64 | Windows 10 22H2, build 19045 or later, native ARM64 | ARM64 EXE |
| Android / Android TV | Android 8.0, API 26 or later | APK matching the device's CPU architecture |
| Android / Android TV, architecture unknown | Android 8.0, API 26 or later | Larger universal APK containing all three architectures |

### Planned package names (v0.3.0 examples)

- `usque-v0.3.0-windows-x64-v2.exe`
- `usque-v0.3.0-windows-arm64.exe`
- `usque-v0.3.0-android-arm64-v8a.apk`
- `usque-v0.3.0-android-x86_64.apk`
- `usque-v0.3.0-android-armeabi-v7a.apk`
- `usque-v0.3.0-android-universal.apk`

The planned package set also includes `usque-v0.3.0-windows-x64-v2.msi` and
`usque-v0.3.0-windows-arm64.msi` for Usque's in-app update flow. Use the EXE for
manual Windows installation.

Each release includes `SHA256SUMS`, `release-manifest.json` and a software
component inventory (SPDX SBOM) for each package. Pull Request builds, local
validation packages and files from other sites are not official releases.

## Verify before installing

Download the package and `SHA256SUMS` from the same release. The examples below
use the planned v0.3.0 names; substitute the exact filename and published tag you
downloaded. These commands inspect files without installing or running them.

### Check the file SHA-256

In PowerShell, open the folder containing the download and run:

```powershell
$package = '.\usque-v0.3.0-windows-x64-v2.exe'
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

Pre-1.0 packages use the project's fixed self-signed certificate. When that
certificate is absent from Windows trust stores, the expected `$signature.Status`
is `UnknownError`, with a `StatusMessage` reporting that the certificate chain
ends in an untrusted root. Windows can also show an unknown-publisher warning.
`Valid` is also accepted if Windows already trusts the signature. For either
accepted result, the exact official package hash and full certificate SHA-256
must both match. Accept `UnknownError` only for the expected untrusted-root
result; stop for a different error message, `NotTrusted`, `HashMismatch`,
`NotSigned`, any other status, or a different signer certificate. Do not import
the certificate into Root or Trusted Publisher to change the status or hide
the warning. The identity policy is in
[Code signing](CODE_SIGNING.md).

### Check the Android signer

On a computer with Java and Android SDK Build Tools installed, use
[apksigner](https://developer.android.com/tools/apksigner) to verify the APK and
print its certificate. Replace the tool path with your installed Build Tools
directory:

```powershell
$apksignerPath = 'C:\path\to\Android\Sdk\build-tools\<version>\apksigner.bat'
& $apksignerPath verify --verbose --print-certs '.\usque-v0.3.0-android-arm64-v8a.apk'
if ($LASTEXITCODE -ne 0) { throw 'APK signature verification failed.' }
```

The command must succeed. Compare **Signer #1 certificate SHA-256 digest** with
**Android release certificate SHA-256** in the release notes. Compare the
certificate digest, not a public-key digest. Check the APK file hash separately
as described above. You can then copy that verified file to the Android device.

### Check the build provenance

If you have GitHub CLI, verify the attestation for the same downloaded file:

```powershell
gh attestation verify $package --repo GeorgeXie2333/usque-app --source-ref refs/tags/v0.3.0 --signer-workflow GeorgeXie2333/usque-app/.github/workflows/release.yml
if ($LASTEXITCODE -ne 0) { throw 'Build provenance verification failed.' }
```

This checks the file against the repository, source tag and release workflow
that produced its attestation. See the
[GitHub CLI reference](https://cli.github.com/manual/gh_attestation_verify) for
authentication and verification options. An unavailable attestation is not a
successful verification; a reported mismatch must be investigated before use.

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
Local validation packages are not substitutes for this official upgrade.

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

## Android and Android TV

Choose arm64-v8a for ARMv8, x86_64 for x64, or armeabi-v7a for ARMv7. If you do
not know the architecture, use the universal APK. Check its hash and signer
before installing or upgrading.

The app is distributed outside Google Play. Android may ask you to allow
installation from the browser or file manager you used to open the APK.
Official packages must use `io.github.georgexie2333.usque` and the published
release signing certificate, and that pair must remain **Registered** through
[Android developer verification](https://developer.android.com/developer-verification).
The maintainer must confirm its status before distribution, as required by
[Code signing](CODE_SIGNING.md#android-developer-verification); this guide does
not verify a candidate's current registration. Verification records developer
identity and key ownership; it is not Google Play distribution or a review of
the app's content. Source-permission and sideloading prompts can still appear.

Android now requires VPN consent during first-run setup, even if you later
choose only SOCKS5 or HTTP. Granting it may disconnect another active VPN but
does not start a Usque connection. Notification permission is optional. Setup
checks interrupted account operations before registering again; use **Check
result** or **Continue with saved account** when offered.

After setup, choose outputs in **Proxy → VPN and local proxies**, then connect
from Home. Proxy-only operation does not start a VPN. If Android has revoked VPN
consent, enabling VPN output requests it again.

### Keep apps blocked when the VPN ends

The in-app Kill Switch protects connecting and recovery while the VPN remains
running. HTTP/SOCKS chain startup and protected handoffs retain a blocking
interface until the replacement is ready; terminal failures retain it according
to the applied Kill Switch policy. Unconfirmed cleanup keeps protection.
Other chain sources retain their documented lifecycle. The in-app protection
cannot survive the VPN process ending. Without Android's system blocking,
ordinary network access resumes after the VPN ends. See the
[chain protection rules](CHAIN_PROXY.md).

Open **Settings → Connection & protection → Open Always-on VPN settings**. Enable
both **Always-on VPN** and **Block connections without VPN**.

For automatic startup after reboot, also enable **Start Usque when you sign in**
under **Settings → Application** and **Connect the current
account automatically on start** under **Connection & protection**. On Android this switch's description reads
**Start Usque after the device restarts. To connect automatically, also turn on “Connect the current account automatically on start”.** Windows
shows the same switch title, which starts Usque when you sign in to Windows.

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
physical DNS fallback. Explicit local and proxy DNS choices still apply. Apps
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

Maintainers must use the [required isolated environments](../CONTRIBUTING.md#development-machines)
for real install, upgrade, VPN and cleanup validation. A compile or file check
does not establish those results.

---

WARP is a trademark and/or registered trademark of Cloudflare, Inc. in the United States and other jurisdictions.
