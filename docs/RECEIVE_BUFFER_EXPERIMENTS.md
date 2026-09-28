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
The five experimental APKs and three temporary packaging scripts are no longer
kept there; only those manifests and checksums identify the tested binaries.

## Historical workstation checks

These counts describe their earlier working-tree stages, not the consolidated
candidate. The default-policy promotion has its own dated validation record below.
Current source changes require new checks.

| Stage | Rust | Flutter | Kotlin | Additional recorded status |
| --- | --- | --- | --- | --- |
| L4 buffer A/B | 925 passed, 2 live tests ignored | 414 passed | 172 passed | Windows Clippy/release compile, Android normal/A/B Clippy, Flutter analysis/Windows compile, Kotlin format/lint, Buf and 71 Python tests passed; PSScriptAnalyzer loading was blocked by OS policy |
| H3 buffer A/B | 927 passed, 2 live tests ignored | 414 passed | 173 passed | Windows Clippy/release compile, Android normal/A/B Clippy, Flutter analysis/format, Kotlin format/lint, policy, 71 Python tests and both separately invoked PSScriptAnalyzer passes passed |

Parser tests covered native 32/64-bit control-message layouts, truncation,
malformed input, missing loss, counter wrap, socket/attempt changes and bounded
history. Only numeric values and allowlisted status tokens crossed the
diagnostic boundary. These functional checks were not throughput measurements.

## Production promotion validation, 2026-09-10

This records the consolidated working-tree candidate based on `5aa24a9`, including
the receive observations, production default, retired build controls, UTF-8
boundary tests and contextual L4 hint/localization coverage. It is not a signed
artifact or protected-runner report. SDK paths were resolved locally;
Flutter 3.44.7 revision `84fc5cbb223bc12f83d65b647ff8a56caf779ffd`, Java 17 and
the pinned NDK/CMake were used. All Cargo actions below use locked dependencies.

| Command / sequence | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| `tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed |
| Same helper with `-CargoAction test` | 932 passed, 2 existing live tests ignored |
| Same helper's default release build | Passed, compile-only |
| `tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed; no receive-experiment features |
| `flutter pub get --enforce-lockfile`; `dart format --output=none --set-exit-if-changed lib test`; `flutter analyze --no-pub`; `flutter test --no-pub` | Passed; 425 tests including Windows goldens and all-locale transport-hint lookup |
| `tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .`; `flutter build windows --release --no-pub` from the GUI directory | Passed, compile-only; helper invoked via `../../tool/` |
| `flutter build apk --debug --config-only --no-pub`; `gradlew.bat --no-daemon :app:ktlintCheck`; `gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` | Passed; 176 Kotlin tests; fixed helper compiled all three debug JNI ABIs with no experiment |
| `buf lint`; `buf format --exit-code --diff`; `buf breaking --against '.git#ref=5aa24a9' --against-config buf.yaml` | Passed |
| `py -3 -m ruff check tool`; `py -3 -m ruff format --check tool`; `py -3 -m unittest discover -s tool -p 'test_*.py' -v` | Passed; Ruff 0.16.0, 71 Python tests |
| `Invoke-ScriptAnalyzer -Path tool -Recurse -Settings tool/PSScriptAnalyzerSettings.psd1`; separate `-IncludeRule PSUseCorrectCasing` pass | PSScriptAnalyzer 1.25.0, both passed without findings |
| `go mod verify`; `go test ./...` in `oracle/go`; `py -3 tool/verify_oracle_archive.py` | Passed; frozen reference unchanged |
| `py -3 tool/check_repository_policy.py`; `git diff --check` | Passed |

`tool/check_source.ps1`, after the supported Windows helper initialized the same
shell, passed Rust, Dart, Flutter, Kotlin and Ruff checks but **did not complete**:
its forced module import was blocked by the workstation software restriction
policy for `PSScriptAnalyzer/1.25.0/ScriptAnalyzer.format.ps1xml`. The two separately
invoked analyzer passes above and Buf checks did complete. No script, analyzer
module or OS policy was changed to bypass that restriction. No installer or new
APK was produced, installed or exercised for this promotion.

The mode-selector follow-up passed actual selection, draft preservation,
unsupported-engine, keyboard/D-pad, semantics, portrait/landscape and 200% text
tests. All 21 locale catalogs resolve their localized hint in explicit and
system-language modes. Two new real-font Windows goldens (English light and
Chinese dark/200%) were visually reviewed; existing baselines were not changed.
