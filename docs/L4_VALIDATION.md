# L4 development validation

This is the historical initial-L4 validation record. The subsequent high-load
backpressure/stop fix and its separate validation scope are recorded in
[L4 backpressure follow-up](L4_BACKPRESSURE_FIX.md).

Validation date: 2026-09-09. Scope: this uncommitted working tree on a Windows
development machine. This is a development record, not signed release evidence
or a claim about real Cloudflare throughput/leak safety.

## Completed checks

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| Windows helper, `-Variant x64-v2 -CargoAction clippy` | Passed, locked workspace/all-targets |
| Windows helper, `-Variant x64-v2 -CargoAction test` | 878 passed; two existing credential-dependent live tests ignored |
| Windows helper, `-Variant x64-v2` | Release compile passed; no MSI created or installed |
| Vendored stack `cargo test --manifest-path third_party/ts_netstack_smoltcp_core/Cargo.toml --lib --locked` | Five passed, including allocation, cancellation and socket-slot ownership |
| Flutter `pub get --enforce-lockfile` | Passed without lockfile changes |
| Dart `format --output=none --set-exit-if-changed lib test` | Passed |
| Flutter `analyze --no-pub` | Passed |
| Flutter `test --no-pub` | 410 passed, including the existing exact Windows goldens |
| Windows plugin-junction helper and Flutter `build windows --release --no-pub` | Passed; application not launched |
| Android Rust helper, `-AbiFilter arm64-v8a -CargoAction clippy` | Passed |
| Android Rust helper, `-AbiFilter all -CargoAction build` | Debug JNI builds passed for arm64-v8a, armeabi-v7a and x86_64; no device install |
| Flutter `build apk --debug --config-only --no-pub` | Passed; no device install |
| Gradle `:app:ktlintCheck :app:testDebugUnitTest :app:lintDebug` | Passed; 161 JVM tests, zero failures/errors/skips |
| Ruff 0.16.0 `check tool` and `format --check tool` | Passed |
| Python `-m unittest discover -s tool -p 'test_*.py' -v` | 70 passed |
| Buf 1.72.0 lint and format checks | Passed |
| Buf breaking check against `.git#ref=HEAD` | Passed; this is a local comparison, not a PR-target CI run |
| `actionlint -no-color` (1.7.12) | Passed |
| Frozen Go oracle `go mod verify`, `go test ./...`, archive verification | Passed; archived source unchanged |
| Repository policy and `git diff --check` | Passed |

Flutter/Dart commands used the SDK resolved from `android/local.properties`:
Flutter 3.44.7, revision `84fc5cbb223bc12f83d65b647ff8a56caf779ffd`.
Windows Rust operations used `tool/build_windows_rust_release.ps1`; Android
Rust operations used the pinned NDK/CMake helper. Gradle's debug validation also
compiled the JNI variants through that helper. Generated JNI/build outputs are
not included in source changes. No image golden was regenerated.

The newly added tests cover explicit mode/SNI contracts, schema migration,
classic CONNECT/1xx/2xx/403, early DATA+FIN, zero-copy ownership, GOAWAY,
concurrent single-session reuse, HTTP pre-read bytes, SOCKS UDP rejection,
in-memory IPv4/IPv6 TUN TCP, DNS resolver preservation, malformed input,
packet checksums, TC replies, allocation downshift and cancellation cleanup.
They are not a substitute for the full controlled network-fault matrix.

## Blocked static check

`tool/check_source.ps1` was attempted in the initialized Windows environment.
Its Rust, Dart, Flutter, Kotlin and Ruff stages passed. It stopped while
importing PSScriptAnalyzer 1.25.0: Windows software restriction policies blocked
`ScriptAnalyzer.format.ps1xml`. Consequently the PSScriptAnalyzer settings and
correct-casing passes, and the aggregate command as a whole, are **not passed**.
No execution-policy, Group Policy, signature or trust-store bypass was applied.
Buf was subsequently checked separately. No PowerShell script was changed.

## Not run

- Live Consumer/WARP+ and Zero Trust Cloudflare CONNECT tests. The fixed
  upstream vectors and ephemeral loopback TLS peer are not live account evidence.
- Windows snapshot-VM install/upgrade/recovery, Wintun and Kill Switch tests.
- Dedicated Android device/emulator lifecycle, Doze, Always-on and Lockdown tests.
- Independent IPv4/IPv6/DNS/ordinary-UDP/candidate-path leak observation.
- Controlled seven-run performance comparisons and extended network-fault runs.
- MSI/APK installation, release signing, packaging, publication and deployment.

These statuses remain `not_run`. Protected-runner reports are supplemental,
not new publication prerequisites. The experimental label and explicit opt-in
remain in place; Auto and default networking behavior are unchanged.

See [L4 behavior and limitations](L4_PROXY.md) and the
[performance sampling contract](l4-performance-scenarios.json).
