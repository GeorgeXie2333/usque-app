# Native size experiments / 原生体积实验

Date: 2026-09-25. Source: `8de7b46596fe7cbebaa06ec6867cf915fad0bb4e`. Native source and lockfiles are identical
across candidates; only the recorded build configuration changes. The
[measurement manifest](NATIVE_SIZE_MEASUREMENTS.json) contains artifact hashes,
configuration, benchmark-source hash, paired-run metadata and scenario summaries.
This is local screening, not signed release, Android device, WAN or protected-lab
performance evidence.

## Decision

Retain the current release profile and native C/C++ configuration. No experimental
compiler setting is shipped. The agreed 2% measurement-noise prerequisite was
not met consistently, so the data do not establish an absence of performance
regression. CPU-time medians are additionally quantized by `GetProcessTimes`;
equal medians are not proof of equal per-packet CPU cost.

Candidate D (`s` plus fat LTO) was not run: neither constituent Rust change met
the performance-priority screening requirement. Android runtime performance and
Windows ARM64 compilation were unavailable, not passed. This does not block
shipping the unchanged native profile or proceeding with the independent resource,
Dart-symbol and APK-compression work; their own checks still apply.

## Uncompressed native bytes

These are file bytes, not measured MSI or APK reductions. The Windows total is
the engine, Agent, updater and uninstaller; it excludes the benchmark executable.

| Candidate | Rust opt-level / LTO | Windows x64-v2 engine | Four Windows binaries | Android arm64 JNI |
| --- | --- | ---: | ---: | ---: |
| A | 3 / thin | 16,623,616 | 20,087,296 | 13,174,864 |
| B | 3 / fat | 15,770,624 | 19,096,576 | 13,174,864 |
| C | s / thin | 11,225,088 | 13,986,304 | 11,110,632 |
| E | 3 / thin; MSVC `/Gy /Gw` | 16,568,320 | 20,032,000 | not_run |

Android already compiles the native dependencies with function/data sections.
E therefore targets Windows only. The initial E attempt changed `CFLAGS` but
reused CMake archives; only ring rebuilt. That incomplete attempt is excluded.
The final E result follows scoped release cache cleaning of `usque-openvpn` and
`boring-sys`. Generated BoringSSL C11 and OpenVPN C++17 commands were checked for
both `/Gy` and `/Gw` before its test executable was accepted.

## Paired memory measurements

Each B, C and E comparison runs seven A/B pairs, reversing order on even pairs.
The disposable test child is pinned to logical CPU 0; the host power plan and
other processes are unchanged. No compiler or other task tests run concurrently
with sampling. The original ignored WireGuard harness covers IPv4/IPv6, MTUs
1280/1420/1500, 1/8 streams, and simulated RTTs 0/20/80 ms. Each of the 42 runs
completed 180 samples with matching sent/received counts.

The first internal round of each scenario is a warm-up and is excluded from
summaries. For each paired run and scenario, use the median of the remaining
rounds; report median paired candidate/baseline ratios. The table aggregates the
12 zero-RTT scenarios with a geometric mean to reduce simulated-delay dominance.
The manifest retains all 36 scenario summaries for each candidate. MAD is median absolute deviation
divided by the median of repeated baseline measurements.

| Candidate | Throughput ratio | CPU/bit ratio | p95 latency ratio | Largest baseline throughput MAD | Largest baseline p95 MAD |
| --- | ---: | ---: | ---: | ---: | ---: |
| B | 1.0023 | 1.0000 | 0.9806 | 8.85% | 33.98% |
| C | 0.9712 | 1.0000 | 0.9834 | 7.88% | 30.69% |
| E | 0.9664 | 1.0000 | 1.1201 | 6.36% | 24.90% |

Throughput ratios above 1 are faster; CPU and latency ratios below 1 are lower.
Noise and missing platform coverage prevent interpreting these aggregate ratios
as a production improvement or a guaranteed regression bound. Process peak
working sets are sampled separately and are recorded as observations, not app
memory budgets. The release-mode Rust test harness uses test panic behavior; it
does not replace the production `panic=abort` binaries or a controlled lab run.

## Correctness and reproduction

- A, B, C and verified E: 556 release transport tests passed per candidate; the
  one ignored memory benchmark was then executed explicitly for the paired runs.
  These include the existing memory and loopback protocol/data-path fixtures.
- E: five native source/notice locks verified; OpenVPN `interop-test` Clippy
  passed and all 14 socket-free TLS/CBC interoperability tests passed.
- Android default-profile Rust Clippy and all three release JNI ABIs passed.
- Default Windows CMake release caches were rebuilt after E and checked to
  exclude experimental section flags. Default four-binary sizes match A.

Follow [Contributing](../CONTRIBUTING.md) for the pinned native environment.
Use a fresh PowerShell child for each candidate. Set
`CARGO_PROFILE_RELEASE_OPT_LEVEL` to `3` or `s` and
`CARGO_PROFILE_RELEASE_LTO` to `thin` or `fat`; keep codegen units, panic, stripping,
features and CPU compatibility unchanged. Run the Windows helper and preserve
its outputs before another candidate overwrites them. Android experiments use
fresh Android-only shells and the pinned helper with release/arm64-v8a.

```powershell
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2
# Same initialized native environment, with the same CPU baseline:
$env:RUSTFLAGS = '-C target-cpu=x86-64-v2'
cargo test --release --locked --target x86_64-pc-windows-msvc -p usque-transport --features wireguard --lib --no-run --message-format=json
# Preserve the emitted test executable, then run its ordinary tests:
./transport-tests.exe --test-threads=1
# Run the memory-only harness, seven alternating pairs per comparison:
./transport-tests.exe wireguard::benchmark::memory_performance --exact --ignored --nocapture --test-threads=1
```

E additionally sets `CFLAGS` and `CXXFLAGS` to `/Gy /Gw`. Confirm the target
directory is inside the worktree, then clean only the affected release packages
before building E, and again after clearing those environment variables to
restore defaults:

```powershell
cargo clean -p usque-openvpn --release --target x86_64-pc-windows-msvc
cargo clean -p boring-sys --release --target x86_64-pc-windows-msvc
```

The additional exact native checks are:

```powershell
python tool/check_openvpn_sources.py
cargo clippy -p usque-openvpn --all-targets --features interop-test --locked -- -D warnings
cargo test -p usque-openvpn --features interop-test --locked
& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy
& ./tool/build_android_rust.ps1 -BuildProfile release -AbiFilter all -CargoAction build
```

Keep raw logs and experimental binaries local. No TUN, system networking,
installer execution, protected-runner invocation or publication was performed.
