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
- proxy/TUN throughput ratios at 20 ms, 100 ms, and 300 ms RTT for one and four
  concurrent HTTP CONNECT and SOCKS5 TCP streams;
- the independent gateway packet-capture digest;
- an `attachments` mapping from safe relative paths to SHA-256.

Every test and device result has an `evidence_sha256` that must resolve to an
attachment in the same evidence directory. The validator rejects missing
attachments, path traversal, digest changes, incomplete devices/tests, a soak
shorter than 24 hours, fewer than 100 cycles, throughput below 90% of the Go
oracle, p95 latency above 110%, or memory above 125%.

For `v0.1.1-beta.2`, the proxy matrix is an additional release gate. At every
declared RTT, each single-stream HTTP CONNECT and SOCKS5 TCP result must reach at
least 80% of the same-build TUN baseline, and each four-stream aggregate must
reach at least 90%. The laboratory must use the exact candidate binary and the
same endpoint, payload, duration, ingress family, and physical link for each
proxy/TUN pair. A local loopback benchmark or a unit test cannot satisfy this
gate.

For Windows, `uninstall.restore` evidence must include before/after inventories
for the Agent service, Usque WFP provider/sublayer/filters, Usque Wintun adapter,
owned routes, interface DNS, system-proxy owner marker, installation directory,
Start Menu shortcut, and `%ProgramData%\Usque\agent\recovery-v1.json`. The
uninstall test must begin with an active journaled VPN transaction and must also
cover a stale-adapter recovery process after an Agent crash. User Profiles and
Credential Manager records are expected to remain unless **Clear all data** was
explicitly exercised first; the shared Wintun driver package is out of scope.

The exact required test IDs and device IDs live in
`tool/release_contract.py`; changing them is a reviewable release-contract
change. The current device IDs are:

| Platform | Device ID | Artifact variant |
| --- | --- | --- |
| Windows | `windows-10-19045-x64-v2` | x64-v2 MSI |
| Windows | `windows-11-x64-v2` | x64-v2 MSI |
| Android | `android-8-arm64` | arm64-v8a APK |
| Android | `android-api33-arm64` | arm64-v8a APK |
| Android | `android-current-arm64` | arm64-v8a APK |
| Android TV | `android-tv-arm64` | arm64-v8a APK |

Evidence attachments are retained as protected workflow artifacts for 90 days.
The public prerelease receives the validated evidence JSON and its attachment
hashes, not raw captures that may contain laboratory addresses.
