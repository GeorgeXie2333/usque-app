# Chain proxy validation and size / 链式代理验证与体积

Date: 2026-09-22, Windows x64 development host. Base source commit:
`0931d1a0898461eed748c67488800a91da9e6630`, plus the uncommitted chain-proxy
implementation. The [measurement manifest](CHAIN_PROXY_MEASUREMENTS.json)
records source-file and artifact hashes; this is local compile/test evidence,
not a signed release or an installer comparison.

The implemented sources are **OpenVPN**, **WireGuard**,
**VPN Gate**, in that order. [The guide](CHAIN_PROXY.md) documents the import
workflow, compatibility, encrypted storage, DNS and lifecycle boundaries.
The two icons use the SVG paths supplied by the user, with theme colors and
OpenVPN's proportional 32×32-to-24×24 scaling. Later note: the repository does
not record an upstream source or license for these paths; see the
[current guide](CHAIN_PROXY.md#storage-compatibility-and-dependencies).

## Checks

| Check | Result |
| --- | --- |
| Rust workspace Clippy, locked, all targets | Passed |
| Rust workspace tests, locked, all targets | 1,174 passed; 5 intentionally ignored |
| OpenVPN source and license lock verification | 5 native source locks verified |
| OpenVPN `interop-test` Clippy | Passed |
| OpenVPN complete memory-only suite | 11 passed; exact Cargo gate and 8 consecutive additional full-suite runs passed |
| Android arm64 Rust Clippy | Passed |
| Android JNI, three ABIs, release, WireGuard off/on | Passed; no APK or device installation |
| Kotlin `ktlintCheck`, unit tests, Android lint | Passed; 215 unit tests, 0 failures/errors/skips |
| Flutter locked dependencies, format, analysis | Passed with the pinned SDK |
| Flutter full widget/golden suite | 612 passed |
| Windows x64 Flutter release build | Passed |
| Buf lint and format | Passed; append-only Rust/Dart wire snapshots passed |
| Repository policy and diff whitespace | Passed |
| Windows Rust x64-v1 and x64-v2 release builds | Passed |
| Windows ARM64 release builds | `not_run`: Visual Studio ARM64 C++ Build Tools absent |
| PowerShell PSScriptAnalyzer 1.25.0 | `not_run`: local software restriction policy blocks its format-data module |
| `tool/check_source.ps1` aggregate | Incomplete at PSScriptAnalyzer; not reported as passed |

The aggregate completed Rust format/Clippy, Dart format/analysis, Kotlin format
and Ruff checks before its environment failure. Both standalone
`Invoke-ScriptAnalyzer` commands encounter the same module-loading restriction.
The host policy was not changed. Buf was run independently to complete its gate.

Intermediate OpenVPN memory-suite runs intermittently stalled. The in-memory
server fixture temporarily modifies OpenVPN's process-global algorithm table;
another native-client initialization test did not share its serialization lock.
All native initialization tests now share that lock. With diagnostic
instrumentation removed and the original test flow restored, eight consecutive
full-suite runs and the exact Cargo gate passed. This is a test-fixture
concurrency correction, not evidence of a production hang fix.

Deterministic coverage includes schema 16→17 migration and failed-migration
preservation; legacy-writer protection; shared selection across accounts; encrypted
storage, DPAPI object binding, stale edits, failed writes and orphan cleanup;
dangerous directives, oversized input and malformed keys; L4 mode enforcement;
WireGuard authentication, data, replay rejection, AllowedIPs, rekey, Keepalive,
cancellation and backpressure; OpenVPN TCP/UDP TLS/CBC interoperability and rekey;
IPv4/IPv6 protocol UDP across memory stacks with 1280 MTU and payloads up to
9,032 bytes; import/cancel/apply/source switching, draft protection, phone large
text, TV focus and metadata privacy. Property-based fuzz tests cover arbitrary
import text/credentials and arbitrary, truncated or mutated IPv6 fragment
sequences. These additional integration tests do not change native release code.

The updated golden images were visually inspected with the actual fonts on
Windows. They include both supplied SVGs at 18, 20, 24 and 32 pixels, normal,
selected, disabled and focus states, light/dark themes, custom configuration
pages, the proxy entry and the retained VPN Gate navigation/layout.

## Native size comparison

The baseline was freshly rebuilt from an archive of the base commit. Preexisting
release binaries were also inventoried initially, but their source provenance was
unknown and they are **not** used for the deltas below. Baseline builds use that
commit's lockfile; WireGuard A/B builds use the same final source, lockfile,
toolchain, target and release optimization settings, changing only the
`wireguard` feature. Artifact sizes are uncompressed file bytes.

| Native component | Fresh baseline bytes | WireGuard off bytes | WireGuard on bytes | WireGuard increment | Whole feature increment |
| --- | ---: | ---: | ---: | ---: | ---: |
| Windows x64-v2 engine | 15,093,760 | 15,324,672 | 15,478,272 | 153,600 bytes / 0.146 MiB (1.00%) | 384,512 bytes (2.55%) |
| Android arm64-v8a JNI | 11,928,968 | 12,118,112 | 12,250,248 | 132,136 bytes / 0.126 MiB (1.09%) | 321,280 bytes (2.69%) |
| Android armeabi-v7a JNI | 7,959,612 | 8,079,484 | 8,192,796 | 113,312 bytes / 0.108 MiB (1.40%) | 233,184 bytes (2.93%) |
| Android x86_64 JNI | 13,403,744 | 13,631,688 | 13,812,088 | 180,400 bytes / 0.172 MiB (1.32%) | 408,344 bytes (3.05%) |

“Whole feature” compares the final native component with the freshly rebuilt
base component. It includes the shared importer/transport/model changes. It does
not include Flutter AOT code, SVGs, packaging compression or signatures. None of
the measured single-architecture WireGuard increments exceeds the 2 MiB analysis
threshold. Windows ARM64 has no measurement because its compiler was unavailable.

## GUI and SVG size comparison

The Windows GUI control uses the same final application code, replacing the SVG
widget with an empty box and omitting `flutter_svg` plus its SVG asset entries
in an ignored temporary build copy. Common dependency versions remain locked.
This measures the renderer, its reachable dependencies, asset-manifest changes
and assets together. It is separate from the native WireGuard comparison.

| Windows x64 GUI runtime files | Bytes |
| --- | ---: |
| Fresh baseline | 38,683,984 |
| Final application without SVG rendering | 38,835,569 |
| Final application with SVG rendering | 39,330,720 |
| SVG rendering/assets increment | 495,151 (0.472 MiB) |
| Of that, the two SVG source files | 3,100 |
| Entire GUI feature increment over baseline | 646,736 (0.617 MiB) |

Flutter `data/app.so` changes from 9,765,776 to 10,257,296 bytes in the SVG
comparison. The runtime-file total excludes PDBs and any separately staged Rust
engines/agents/updaters/uninstallers or Wintun DLL; stale, preexisting native files
in a GUI build folder cannot influence this metric. The manifest preserves the
counted file sets. Android Flutter AOT and compressed installer deltas were not
measured. Do not describe these raw component deltas as measured MSI/APK sizes.

## Reproduce

Use Rust 1.97.1, Flutter 3.44.7 at
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`, NDK 29.0.14206865 and SDK CMake 3.22.1.
Resolve Flutter from the project's local SDK configuration as described in
[Contributing](../CONTRIBUTING.md). `python` below means the verified Python 3.10+
runtime; it was Python 3.12.14 on this host. Native outputs and logs stay ignored.
For the baseline, export the recorded base commit with `git archive` into a
separate temporary tree, copy only the local SDK-path configuration, and use that
tree's checked-in helpers and lockfiles. Rebuild its native libraries and Windows
GUI with the same target and release settings before comparing file bytes.
Run each Windows helper invocation in a fresh PowerShell process: repeatedly
importing `vcvars` in one process can grow its environment beyond `cmd.exe`'s
command-line limit. Run the OpenVPN Cargo commands and aggregate in the same
process as one successful helper initialization.

```powershell
cargo fmt --all --check
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
python tool/check_openvpn_sources.py
# Same helper-initialized PowerShell session:
cargo clippy -p usque-openvpn --all-targets --features interop-test --locked -- -D warnings
cargo test -p usque-openvpn --features interop-test --locked
& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy
```

```powershell
# Native A/B: copy/hash each artifact before building the other feature setting.
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -DisableWireGuard
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2
& ./tool/build_android_rust.ps1 -BuildProfile release -AbiFilter all -CargoAction build -DisableWireGuard
& ./tool/build_android_rust.ps1 -BuildProfile release -AbiFilter all -CargoAction build
& ./tool/build_windows_rust_release.ps1 -Variant x64-v1
& ./tool/build_windows_rust_release.ps1 -Variant arm64
```

```powershell
Set-Location apps/usque_gui
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub
& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .
flutter build windows --release --no-pub
flutter build apk --debug --config-only --no-pub
Set-Location android
./gradlew.bat --no-daemon :app:ktlintCheck
./gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug
```

```powershell
# Repository root; initialize Windows native environment first for the aggregate.
buf lint
buf format --exit-code --diff
python tool/check_repository_policy.py
git diff --check
Invoke-ScriptAnalyzer -Path tool -Recurse -Settings tool/PSScriptAnalyzerSettings.psd1
Invoke-ScriptAnalyzer -Path tool -Recurse -IncludeRule PSUseCorrectCasing
pwsh -NoProfile -File tool/check_source.ps1
```

## Not run / 未运行

- Windows ARM64 Rust/Flutter release compilation and size: missing Visual Studio
  ARM64 tools.
- PSScriptAnalyzer and aggregate completion: blocked by host software policy.
- Real Windows VPN/Wintun/WFP/routes/DNS lifecycle: no isolated snapshot VM.
- Android device/TV VPN lifecycle, Keystore on real hardware, Always-on/Lockdown,
  Doze, process death and reboot: no dedicated device or isolated emulator.
- External IPv4/IPv6/DNS/direct-rule leak observation and controlled performance:
  no corresponding isolated observers/lab.
- MSI, release APK, installation, official signing, publication: outside this task.

隔离测试均为 `not_run`，没有用桌面 widget、golden 或内存协议测试替代真实平台验证。
缺少的补充环境不作为发布前置条件；同时也不构成验证通过。原生 WireGuard 增量与
SVG/Flutter 渲染增量分开报告，未把估算或旧产物差值写成安装包实测结论。
