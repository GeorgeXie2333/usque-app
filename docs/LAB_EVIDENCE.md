# Isolated release-laboratory evidence

VPN tests must never run on the Codex development host. The Windows and Android
release jobs use only protected self-hosted laboratories with an independent
gateway capable of capture, DNS manipulation, UDP blocking/loss, single-stack
networks, sleep/wake, and network switching.

## Harness interfaces

The protected Windows runner exposes:

```powershell
C:\UsqueReleaseLab\run-windows-release.ps1 `
  -CandidateDirectory <absolute-directory> `
  -ManifestPath <absolute-release-manifest.json> `
  -EvidenceDirectory <absolute-output-directory>
```

It must create `windows-evidence.json` under the output directory.

The protected Android runner exposes:

```text
/opt/usque-release-lab/run-android-release \
  --candidate-directory <absolute-directory> \
  --manifest <absolute-release-manifest.json> \
  --evidence-directory <absolute-output-directory>
```

It must create `android-evidence.json` under the output directory.

Both harnesses must install and test only the files in the candidate directory.
They may not rebuild, re-sign, patch, or substitute a binary. Windows tests must
use dedicated machines; Android tests must use the declared physical devices,
TV device, and emulator.

## Evidence object

`tool/release_contract.py` is the authoritative validator. Each evidence JSON
object contains:

- `schema_version: 1`, `platform`, exact `tag`, exact 40-character `commit`,
  and SHA-256 of `release-manifest.json`;
- the complete platform artifact-name-to-SHA-256 mapping;
- the required device matrix and the exact artifact installed on each device;
- every mandatory test keyed by its stable test ID with `status: "passed"`;
- 100 or more connection cycles and a connected soak of at least 86,400
  seconds;
- measured oracle/candidate throughput, p95 latency, and stable memory;
- the independent gateway packet-capture digest;
- an `attachments` mapping from safe relative paths to SHA-256.

Every test and device result has an `evidence_sha256` that must resolve to an
attachment in the same evidence directory. The validator rejects missing
attachments, path traversal, digest changes, incomplete devices/tests, a soak
shorter than 24 hours, fewer than 100 cycles, throughput below 90% of the Go
oracle, p95 latency above 110%, or memory above 125%.

The exact required test IDs and device IDs live in
`tool/release_contract.py`; changing them is a reviewable release-contract
change. The current device IDs are:

| Platform | Device ID | Artifact variant |
| --- | --- | --- |
| Windows | `windows-10-19045-x64-v1` | x64-v1 MSI |
| Windows | `windows-11-x64-v2` | x64-v2 MSI |
| Windows | `windows-11-arm64` | ARM64 MSI |
| Android | `android-8-armv7` | armeabi-v7a APK |
| Android | `android-current-arm64` | arm64-v8a APK |
| Android | `android-api26-x86_64-emulator` | x86_64 APK |
| Android TV | `android-tv` | Universal APK |

Evidence attachments are retained as protected workflow artifacts for 90 days.
The public prerelease receives the validated evidence JSON and its attachment
hashes, not raw captures that may contain laboratory addresses.
