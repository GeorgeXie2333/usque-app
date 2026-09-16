# Installation and removal

Download packages from this repository's
[GitHub Releases page](https://github.com/GeorgeXie2333/usque-app/releases).

## Version scope

This guide describes the current v0.2.7 source checkout. Development branches can
include changes that are not yet published. For an installed release, use its
release notes and the guide at the matching Git tag.

The Windows upgrade recovery fix was introduced in v0.2.5; the multilingual EXE
installer arrived in v0.2.6. The original v0.2.4 MSI does not have those fixes.
See [Upgrade](#upgrade) if that version cannot uninstall.

## Choose a package

| Platform | Requirements | Package |
| --- | --- | --- |
| Windows x64 | Windows 10 22H2, build 19045 or later | x64-v2 EXE |
| Windows ARM64 | Windows 10 22H2, build 19045 or later, native ARM64 | ARM64 EXE |
| Android / Android TV | Android 8.0, API 26 or later | APK matching the device's CPU architecture |
| Android / Android TV, architecture unknown | Android 8.0, API 26 or later | Larger universal APK containing all three architectures |

### Official package names (v0.2.7)

- `usque-v0.2.7-windows-x64-v2.exe`
- `usque-v0.2.7-windows-arm64.exe`
- `usque-v0.2.7-android-arm64-v8a.apk`
- `usque-v0.2.7-android-x86_64.apk`
- `usque-v0.2.7-android-armeabi-v7a.apk`
- `usque-v0.2.7-android-universal.apk`

The release also provides `usque-v0.2.7-windows-x64-v2.msi` and
`usque-v0.2.7-windows-arm64.msi` for Usque's in-app update flow. Use the EXE for
manual Windows installation.

Each release includes `SHA256SUMS`, `release-manifest.json` and a software
component inventory (SPDX SBOM) for each package. Pull Request builds, local
validation packages and files from other sites are not official releases.

## Verify before installing

Download the package and `SHA256SUMS` from the same release. The examples below
use v0.2.7; substitute the exact filename and tag you downloaded. These commands
inspect files without installing or running them.

### Check the file SHA-256

In PowerShell, open the folder containing the download and run:

```powershell
$package = '.\usque-v0.2.7-windows-x64-v2.exe'
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

Pre-1.0 packages use the project's fixed self-signed certificate. Windows can
report `NotTrusted` or show an unknown-publisher warning because that certificate
is not in its trust stores. Only proceed with that expected trust warning when
both the exact official package hash and the full certificate SHA-256 match.
Do not accept an unsigned file, a hash mismatch, a different signer or another
verification error. Do not import the certificate into Root or Trusted Publisher
to hide the warning. The identity policy is in [Code signing](CODE_SIGNING.md).

### Check the Android signer

On a computer with Java and Android SDK Build Tools installed, use
[apksigner](https://developer.android.com/tools/apksigner) to verify the APK and
print its certificate. Replace the tool path with your installed Build Tools
directory:

```powershell
$apksignerPath = 'C:\path\to\Android\Sdk\build-tools\<version>\apksigner.bat'
& $apksignerPath verify --verbose --print-certs '.\usque-v0.2.7-android-arm64-v8a.apk'
if ($LASTEXITCODE -ne 0) { throw 'APK signature verification failed.' }
```

The command must succeed. Compare **Signer #1 certificate SHA-256 digest** with
**Android release certificate SHA-256** in the release notes. Compare the
certificate digest, not a public-key digest. Check the APK file hash separately
as described above. You can then copy that verified file to the Android device.

### Check the build provenance

If you have GitHub CLI, verify the attestation for the same downloaded file:

```powershell
gh attestation verify $package --repo GeorgeXie2333/usque-app --source-ref refs/tags/v0.2.7 --signer-workflow GeorgeXie2333/usque-app/.github/workflows/release.yml
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
2. Run it and approve the administrator prompt. Choose an installation directory
   when asked. The installer selects its language from Windows; the app's
   language is configured separately.
3. Open Usque from the Start Menu and complete first-run setup. Installation
   itself does not start a VPN.

Usque installs a separate Agent service for privileged network operations.
After installation, the app can start it without another UAC prompt. Ordinary
disconnect restores that connection's network settings and keeps Usque's virtual
adapter for reuse. Full app exit starts adapter removal; Windows can take time
to finish it. See [Troubleshooting](#troubleshooting) if a restart or recovery fails.

### Upgrade

Use the app's [update flow](#updates), or run a verified newer official EXE.
The installer asks Usque to disconnect and exit, including when closing its
window would normally leave it in the tray. An unresponsive older process may
be forcibly closed after the installer's timeout.

Upgrades keep the install directory, accounts, credentials, settings, logs,
caches and recovery records. Downgrades are rejected. Same-version replacement
also replaces equal-version and unversioned application files together, so the
GUI, Engine and Agent stay in sync.

If v0.2.4 cannot uninstall, upgrade with a verified official v0.2.7 Windows
package, then uninstall the newer version if removal is your goal. The newer
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
2. Confirm removal. Leave **Delete user data** unchecked to retain your local
   accounts, settings and credentials for a later reinstall. Selecting it
   permanently deletes only the current Windows user's Usque data.
3. Allow Windows to complete removal. It may ask for administrator approval
   separately for the MSI and installer-bundle cleanup.

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

## Android and Android TV

Choose arm64-v8a for ARMv8, x86_64 for x64, or armeabi-v7a for ARMv7. If you do
not know the architecture, use the universal APK. Check its hash and signer
before installing or upgrading.

The app is distributed outside Google Play. Android may ask you to allow
installation from the browser or file manager you used to open the APK.
The package name `io.github.georgexie2333.usque` and official signing certificate
are registered through [Android developer verification](https://developer.android.com/developer-verification).
This verifies developer identity and key ownership; it is not Google Play
distribution or a review of the app's content. Source-permission and sideloading
prompts can still appear.

Android requests VPN consent only when VPN output is first enabled. SOCKS5 and
HTTP-only use does not request that permission.

### Keep apps blocked when the VPN ends

The in-app Kill Switch protects connecting and recovery while the VPN remains
running. It cannot survive the VPN process ending, and a terminal VPN Gate
failure also closes the VPN. Without Android's system blocking, ordinary network
access resumes after the VPN ends.

Open **Settings → System integration → Open Always-on VPN settings**. Enable
both **Always-on VPN** and **Block connections without VPN**.

For automatic startup after reboot, also enable **Start Usque after reboot**
under System integration and **Connect the current account automatically on
start** for the active account.

### Per-app proxy

This setting is shared across accounts and applies only while VPN output is on.
When off, every app uses the VPN. When on, only checked apps use it; newly
installed apps must be selected. **Select all** checks the apps currently shown
and does not disable the filter. Usque itself is not listed.

With **Block connections without VPN** enabled, unchecked apps are blocked
instead of using the network directly.

### Remove the app

Android removes Usque's private data and Keystore entries during uninstall.
You can export a Consumer WARP Secret beforehand, but Usque does not accept new
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

## Direct-country DNS privacy

Country-based direct rules are optional. System DNS sends matching domain
queries to the current network's DNS servers outside the VPN; DoH and DoT send
them to your chosen encrypted resolver without a plaintext fallback.

Other remote VPN queries use the final tunnel's DNS: WARP normally, VPN Gate
when enabled. Explicit local and proxy DNS choices still apply. Apps with their
own encrypted DNS hide names from Usque, which then classifies destinations by IP.
See [Direct DNS](encrypted-direct-dns.md) for setup and limitations.

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
