# Historical Android receive experiments

This consolidates the retired L4 receive-only, L4 buffer A/B and H3 buffer A/B
notes. It is a history record, **not a current build guide**. The resulting
[production policy](UDP_RECEIVE_BUFFER.md) targets 2 MiB for Windows and Android
QUIC sockets; the temporary build selectors are no longer supported.

## Candidates and observations

The comparison libraries were release-compiled arm64-v8a from successive
uncommitted working trees based on `5aa24a9`. That commit alone does not reproduce
the experimental APKs. The adjacent local manifests record source/artifact/native
library hashes and signing identity; version numbers alone do not identify a
candidate or native optimization level.

| Historical variant | Only intended change | User observation / evidence |
| --- | --- | --- |
| L4 portable receive | Portable UDP receive, native send selection retained | User reported no throughput improvement |
| L4 A | OS receive default plus socket observation | Control candidate, not a new default |
| L4 B | Ordinary 2 MiB receive-buffer request, same observation | User reported 892.9 Mbps down and 410.2 Mbps up |
| H3 A | Fixed CONNECT-IP H3 with OS receive default | User reported default H3 around 500 Mbps down |
| H3 B | Fixed CONNECT-IP H3 with ordinary 2 MiB request | User reported 725.9 Mbps down and 350.9 Mbps up |

Each A/B pair shared a temporary signer and observation code with its counterpart,
not with other pairs or official packages. The original H3-only pair did not
enable L4's larger-buffer request. No comparison intentionally changed MTU, QUIC
windows, TCP tiers, workers, application budgets, pacing, SNI/identity/pins,
DNS/UDP policy or endpoint/Kill Switch authority. Temporary private keys were
removed after artifact verification.

The earlier L4 follow-up diagnostic was exported after disconnect and could not
establish its effective receive capacity. The connected H3 B diagnostic received
on 2026-09-10 did confirm an accepted 2097152-byte request, 4194304-byte raw receive
capacity, 229376-byte send capacity, `recvmmsg` / `sendmmsg` and no reconnection.
It also contained socket and transport-to-TUN queue drops. A successful buffer
request does not establish a loss-free downstream pipeline.

These are user-provided, limited measurements, not controlled seven-run lab
comparisons. They do not prove a universal speed gain, a Windows improvement, or
that every remaining bottleneck has been removed. Real device lifecycle,
externally observed leaks and controlled performance runs were `not_run` on the
workstation. No publication prerequisite was added.

## Retained compatibility and local records

Earlier diagnostic markers remain accepted by the reader:
`android_l4_portable_recv_only`, `android_l4_rcvbuf_control`,
`android_l4_rcvbuf_2m`, `android_h3_rcvbuf_control`, and `android_h3_rcvbuf_2m`.
They describe those old candidates; they are not supported Cargo features or
application settings. Normal builds report `none` explicitly. Missing fields
from older producers remain unknown.

Formal socket diagnostics and append-only protobuf fields are retained. Raw
diagnostic ZIPs, screenshots, device identifiers, business data, APKs and signing
material do not belong in Git. Local manifests, checksums and README records
remain alongside the former APK locations in ignored `dist/android` directories.
The consolidation moved only the three temporary packaging scripts and five
experimental APKs to the Windows Recycle Bin, where they can be restored until
it is emptied. Unrelated historical builds, build caches, JNI output and local
SDK configuration were not cleanup targets.

## Historical workstation checks

These counts describe their earlier working-tree stages, not the consolidated
candidate. Current checks belong in the production policy's validation record.

| Stage | Rust | Flutter | Kotlin | Additional recorded status |
| --- | --- | --- | --- | --- |
| L4 buffer A/B | 925 passed, 2 live tests ignored | 414 passed | 172 passed | Windows Clippy/release compile, Android normal/A/B Clippy, Flutter analysis/Windows compile, Kotlin format/lint, Buf and 71 Python tests passed; PSScriptAnalyzer loading was blocked by OS policy |
| H3 buffer A/B | 927 passed, 2 live tests ignored | 414 passed | 173 passed | Windows Clippy/release compile, Android normal/A/B Clippy, Flutter analysis/format, Kotlin format/lint, policy, 71 Python tests and both separately invoked PSScriptAnalyzer passes passed |

Parser tests covered native 32/64-bit control-message layouts, truncation,
malformed input, missing loss, counter wrap, socket/attempt changes and bounded
history. Only numeric values and allowlisted status tokens crossed the
diagnostic boundary. These functional checks were not throughput measurements.
