# Code signing policy

This policy says which Usque packages are signed, who may sign them, and how to tell an official signature from a local or third-party one. How the release workflow loads keys and checks artifacts is in [RELEASE.md](RELEASE.md). How to verify a download is in [INSTALLATION.md](INSTALLATION.md).

## Official signatures

Only packages attached to a GitHub Release for this repository, with matching checksums and signer fingerprints, are official. Pull Request artifacts, Actions development outputs, fork builds, and local validation packages are not, even if they are signed.

Pre-1.0 official packages use two fixed, project-controlled self-signed identities:

- Windows Authenticode for the MSI and the project EXE/DLL files inside it
- an Android release certificate for every official APK

Those identities are not a public CA and are not in the Windows Root or Trusted Publisher stores. Windows will show an unknown-publisher warning. That is expected. The installer does not install the certificate into the machine trust stores.

A v1.0.0 change of signing identity is a separate release. Until then, the pre-1.0 fingerprints published on the current GitHub Release remain the only official ones.

## What is signed

| Artifact | Signer |
| --- | --- |
| Official Windows MSI | project Authenticode identity |
| Project EXE/DLL files inside that MSI | same identity |
| Official per-ABI and universal APKs | project Android release certificate |
| Official Wintun DLL (`amd64` / `arm64`) | original vendor signature; Usque redistributes those files and does not re-sign them |
| Local validation MSI/APK | a throwaway identity created on the build machine; never official |

Unsigned project binaries must not ship in an official Windows package. A signer mismatch, a modified Wintun DLL, or a missing official fingerprint fails the release.

## Where keys live

Official private keys exist only as `release-signing` GitHub Environment secrets, plus encrypted offline backups held by the release maintainer. They must not appear in the repository, issues, pull requests, logs, caches, artifacts, or unencrypted disk on a development machine.

Public fingerprints are repository or environment variables (`WINDOWS_SIGNER_SHA256`, `ANDROID_SIGNER_SHA256`) and are printed in the GitHub Release notes. The SHA-256 is over the raw certificate (DER), 64 hex characters.

Only the release maintainer may approve `release-signing` and `release-publish`. A local MSI or APK cannot replace a failed or missing GitHub Actions build.

## What users should check

Before installing:

1. Download the package from the [GitHub Releases page](https://github.com/GeorgeXie2333/usque-app/releases).
2. Compare the full package SHA-256 with the digest GitHub shows for that asset.
3. Compare the signer fingerprint with the value in that release's notes.
4. Check the GitHub artifact attestation when it is available.

Do not import a signing certificate from an unofficial package, and do not turn off antivirus or the firewall to make an installer run.

On Windows, after install, the Agent accepts the official self-signed identity only when Windows has checked the Authenticode digest and the certificate fingerprint matches the packaged value. Any other chain result is rejected.

## Rotation and compromise

Keep the current identities until a reviewed v1.0.0 (or later) signing change. Do not rotate the official pre-1.0 keys for convenience.

If an official private key may have leaked, or if a package appears with the official fingerprint but not from this repository's GitHub Release:

- revoke trust in that identity in the release notes
- stop producing packages with it
- report the event through [SECURITY.md](../SECURITY.md)
- publish replacement packages under a new identity, with upgrade notes

A lost backup of an official key is treated the same as a compromise: do not invent a second "official" key for the same SemVer line.

## Local and development signing

`tool/build_windows_local_validation.ps1` and similar helpers may create a temporary self-signed identity, sign a validation package, then delete the key. Those packages are for table checks and isolated VM work only. They must not be published, renamed to look like a GitHub Release, or installed on a daily-driver machine.

Debug and unsigned Android builds used on a developer device are not release certificates. Do not reuse the official Android keystore on a development host.

## Maintainer rules

- Do not commit PFX, keystore, or password files.
- Do not re-sign Wintun or any other third-party binary that already has a vendor signature.
- Do not add the project certificate to Root or Trusted Publisher on user machines.
- Do not sign a package whose contents were not produced by the approved release workflow for that tag.
- Signing-key or release-chain issues are vulnerabilities; handle them privately as in [SECURITY.md](../SECURITY.md).
