# Chain proxy fixes: validation record / 链式代理修复验证记录

Date: 2026-09-22. Baseline: `0f8afd95cba36db3ec07324a0a61512f4b6ec2c2`.
The initial worktree was clean. This record describes the subsequent uncommitted
candidate, identified by [the source manifest](CHAIN_PROXY_FIX_SOURCE_MANIFEST.json).
It does not replace or extend the claims of [the earlier record](CHAIN_PROXY_VALIDATION.md).

本次修复以源码和内存回归为依据。用户报告的真实 Android 冷 DNS 延迟与约
80 Mbps 吞吐症状仍需要隔离设备实测；本文不将编译、widget 或内存协议测试
当作真实设备性能、平台恢复或外部泄漏验证。

## Changes and evidence / 修复与证据

| Issue | Change | Regression evidence |
| --- | --- | --- |
| File import fails | Native message bytes remain read-only; validate length, own a copy, decode strict UTF-8 and erase only that copy. Picker, read, size and encoding failures remain distinct. | `chain_file_import_test.dart` uses actual `StandardMethodCodec` round trips for both formats, cancellation, malformed UTF-8 and the 128 KiB boundary. Four baseline tests failed before the fix. |
| Disabled entry and draft switching | Disabled settings without a live session show a neutral chain icon and “Not enabled / 未启用”. Live and disconnecting states take precedence. Controlled source selection preserves rejected switches. | Chain widget/navigation tests, retained golden comparisons, capability-disabled import/save/apply tests. The disabled-label regression failed on the baseline. |
| UDP receive race | Persistent UDP and raw IPv6 readers retain in-flight receives; one owner reassembles fragments and delivers through a bounded channel. | Abandoned receive waiters, IPv4/IPv6 at outer MTU 1280, normal and fragmented datagrams, cancellation and full command queues. |
| Protocol queue cycles | Separate input/output directions and lifecycle events; capacity is awaited in the protocol select loop. Bounded batches yield to control and timers. | WireGuard duplex saturation/resume/cancel; OpenVPN TCP/UDP 1024-packet saturation with paused IP delivery; input-capacity wakeup tests. |
| OpenVPN native queue loss | Stop plaintext submission before the pinned Core's 64-packet transport queue drop threshold. Reserve input/control capacity within the existing 256-packet/4 MiB bounds. | The new saturation test initially failed because all input was accepted while output was paused; source inspection confirmed Core's `TCP_OVERFLOW` drop. The threshold is now enforced before `tun_recv`. |
| WireGuard idle recovery | A separately testable handshake watch resets after both `None → Some` authentication and rekey. Idle key expiry does not itself fail the session. | Explicit 35-second boundary state-machine checks; actual memory rekey, keepalive, replay and AllowedIPs tests. Tokio virtual time is not claimed to advance BoringTun's private clock. |
| Final DNS delay and fallback | Filter by final families and AllowedIPs; no replacement of explicitly configured but excluded DNS. Two-candidate scheduler: immediate first, 250 ms backup, 1 s candidate and 4 s whole-question limits. Valid negative answers terminate. Empty candidates use the in-app rejection path. | Candidate filtering, deadlines/concurrency, negative answers, loser cancellation, empty-DNS SERVFAIL and Windows/Android synthetic DNS configuration. |
| Socket ownership on DNS cancellation | Exclusive UDP/raw/TCP wrappers retry cleanup when command queues are full. Raw creation joins unclaimed-handle reclamation. TCP cancellation aborts; a successful graceful shutdown retains buffered data and FIN. | Full command queue cleanup, creation cancellation before/after response, paused TCP transmission followed by shutdown/drop. |
| Windows selection/delete race | Configuration transaction precedes the configuration-library lock; deletion and selection validate against the same serialized state. | Deterministic selection/delete races in both lock orders. |
| Unreadable oversized records | New v2 records: serialized plaintext at most 192 KiB, ciphertext at most 256 KiB. Bounded v1 reads preserve larger historical records, IDs and references. No automatic rewrite. | JSON escaping expansion, old 192–256 KiB records, failed rewrite preserving bytes, legacy IPv6 metadata and incomplete-authentication recovery. |
| Password OpenVPN and multiple remotes | Up to 16 deduplicated same-protocol candidates, `remote-random`, exact `CLIENT_CERT 0/1`, password-only mode and `mssfix 0`. One native session per candidate. | Sanitized five-remote Proton fixture, contradictory modes, protocol/family/duplicate limits; native password-only TCP/UDP, TLS-crypt, encrypted private key, wrong credentials and CA rejection. |
| MSS zero semantics | A minimal documented pinned-Core patch preserves explicit zero through MSS calculation. Upstream original source digests remain unchanged. | A TCP SYN with MSS 1460 returns byte-identical through an authenticated memory session with `mssfix 0`; native source lock check. |
| Endpoint failure boundaries | Only startup DNS/dial/transport-close/timeout failures advance; auth/certificate/config/unknown protocol/cleanup failures stop. 120 s candidate budget within an absolute 180 s traffic-admission deadline. | Candidate ordering/permutation, fair-share budgets, terminal error classification, cleanup/cancellation and generation tests. |
| Compatibility | Schema stays 17, record reads v1/writes v2, capability field 37 is appended. Saved endpoint remains the first candidate; actual endpoint and attempts are separate metadata. | Rust wire snapshots, Dart decode, Kotlin metadata allowlist and stable selection-identity tests. |

The 180-second bound governs establishing the connection and admitting traffic.
Safe cleanup and a platform RPC already in flight can finish later; no late
callback is allowed to admit traffic after the deadline. Long operation waits
have outer margin, while acknowledgement, snapshots and cancellation keep short
timeouts. A connected tunnel never automatically cycles through the saved remotes.

The supplied Proton file is read only by an opt-in parser test. It is not copied
into the repository. The checked-in equivalent removes account comments and uses
test certificates and test `tls-crypt` material; none of those fixtures is a real
user credential.

## Independent review / 独立审查

Three independent `gpt-6-astra` reviewers examined protocol/transport,
security/storage/lifecycle, and GUI/import behavior. Findings were checked against
the implementation before changes. Confirmed follow-up fixes include raw socket
ownership, TCP DNS loser cleanup, legacy metadata compatibility, disconnecting
labels, native capacity wakeups and graceful TCP tail preservation. The final
delta review found no further definite defect in the queue counters and TCP
cleanup changes. Review is source evidence, not a claim of platform execution.

## Tools and commands / 工具链与命令

Host: Windows x64. Rust `1.97.1`; Flutter `3.44.7`, commit
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`; Android NDK `29.0.14206865`,
SDK CMake `3.22.1`; Buf `1.72.0`; Ruff `0.16.0`.
Cargo and Flutter dependency locks are unchanged from the baseline.

Every Windows helper invocation starts in a new PowerShell process. Commands
following a helper use its initialized MSVC/Ninja/libclang environment and
`RUSTFLAGS=-C target-cpu=x86-64-v2`. Python is the verified 3.12.14 interpreter;
Flutter/Dart resolve from the pinned SDK in ignored `android/local.properties`.

```powershell
cargo fmt --all --check
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2
& ./tool/build_windows_rust_release.ps1 -Variant arm64
python -X utf8 tool/check_openvpn_sources.py
cargo clippy -p usque-openvpn --all-targets --features interop-test --locked -- -D warnings
cargo test -p usque-openvpn --features interop-test --locked
cargo test --manifest-path third_party/ts_netstack_smoltcp_core/Cargo.toml --lib --locked
```

The optional supplied-file check uses `USQUE_CHAIN_OVPN_TEST_FILE` with
`cargo test -p usque-core --test chain_fix_regressions supplied_proton_file --locked -- --ignored`.
Only pass/fail and test names are logged; configuration contents are not printed.

From `apps/usque_gui`, using the fixed SDK executables:

```powershell
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub
& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .
flutter build windows --release --no-pub
flutter build apk --debug --config-only --no-pub
```

Android Rust commands run at the repository root; Gradle commands run under
`apps/usque_gui/android`. Configuration-only is not an APK build.

```powershell
& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy
& ./tool/build_android_rust.ps1 -CargoAction build -AbiFilter all
./gradlew.bat --no-daemon :app:ktlintCheck
./gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug
buf lint
buf format --exit-code --diff
python -X utf8 tool/check_repository_policy.py
git diff --check
pwsh -NoProfile -File tool/check_source.ps1
```

The aggregate runs only after the Windows helper initializes the same process's
native environment. It supplements the explicit tests and builds above.

## Results / 检查结果

| Check | Result for this candidate |
| --- | --- |
| Rust format and workspace Clippy in initialized Windows environment | Passed; locked dependencies and warnings-as-errors. |
| Windows helper workspace tests | 1,195 passed, 7 explicitly ignored. The supplied-file parser and performance harness are opt-in and run separately. The other five require isolated Wintun or enrolled live-connection environments. |
| OpenVPN source/notice locks | Passed, five native source locks. |
| OpenVPN `interop-test` Clippy and tests | Passed; 14 tests, including TCP/UDP saturation, authentication, encrypted private key, TLS-crypt and rekey. Native protocol initialization remains serialized. |
| Patched netcore unit tests | 10 passed. |
| Supplied Proton file, unchanged input | One opt-in parser test passed; five candidates retained. No remote connection attempted. |
| Flutter locked dependency resolution, formatting and analysis | Passed; 172 Dart files unchanged by format check, analyzer reports no issues. |
| Complete Flutter widget/golden suite | 629 passed on the pinned Windows SDK. Six intentionally changed golden files were visually reviewed. |
| Windows Flutter release | Passed after the plugin-junction helper; compile-only. |
| Android arm64 Clippy | Passed with the pinned NDK/CMake helper. |
| Android JNI debug builds | Passed for arm64-v8a, armeabi-v7a and x86_64. |
| Android debug configuration-only, Kotlin format, unit tests and lint | Passed; 218 Kotlin tests across 31 suites, no failures/errors/skips. |
| Buf lint/format, repository policy and `git diff --check` | Passed separately. |
| Matched memory benchmark | 180 baseline + 180 fixed samples completed; no sample stalled or lost a counted echo. |
| Windows x64-v2 native release | Passed through the Windows native build helper. |
| Windows ARM64 native release | Blocked by missing ARM64 C++ Build Tools. |
| Aggregate `check_source.ps1` | Ran in the initialized native environment. Rust format/Clippy, Dart format/analysis, ktlint and Ruff passed; stopped at PSScriptAnalyzer module import because host software-restriction policy blocks `ScriptAnalyzer.format.ps1xml`. The aggregate did **not** pass. Buf was run separately. |

The existing aggregate policy blocker was not bypassed. No toolchain component
or policy was changed to turn an unavailable check into a pass. Full results
supersede the intermediate failures that drove the fixes; earlier records are
left intact. Raw local logs live under ignored `target/chain-fix-validation`.

## Performance / 性能

The opt-in [memory harness](../crates/usque-transport/src/wireguard/benchmark.rs)
is identical on the archived baseline and fixed source. It runs authenticated
duplex echo across IPv4/IPv6, inner MTU 1280/1420/1500, 1/8 logical UDP port
streams, simulated RTT 0/20/80 ms and five repetitions per combination. The
in-flight limit is 32. Zero RTT replies are delivered immediately; positive RTT
uses the host timer. Zero RTT has 8192 packets per repetition; others have 256.
This is a protocol/queue/allocation measurement, not a WARP or Android network
benchmark. Logical port streams are not independent smoltcp TCP congestion flows.

Build both with `cargo test -p usque-transport --release --lib --features wireguard --locked --no-run` in the
supported Windows environment. The baseline is a clean archive of the baseline
commit with only this test module/file added. Preserve separate test executables
and run the following filter alone, without other builds:

```text
wireguard::benchmark::memory_performance --exact --ignored --nocapture --test-threads=1
```

The same target directory may cache same-version workspace crates across two
source roots. Between builds, clean only `usque-core`, `usque-openvpn`,
`usque-transport`, `usque-geo` and `usque-protocol` release artifacts, or use
separate target directories. Do not delete the whole target tree. Build logs
must identify the correct source roots. The checked-in measurement file contains
sanitized numbers and harness/executable hashes, never raw configuration or traffic.

Packet counts, completions, allocation calls/bytes, latency and process CPU are
recorded. Windows process CPU has coarse quantization; a zero sample is below
the measurement quantum, not proof of zero CPU. Report aggregate CPU as well as
per-combination medians and spread. Earlier timer-delayed zero-RTT exploratory
runs are not the final comparison.

Results are in [the measurement dataset](CHAIN_PROXY_FIX_MEASUREMENTS.json).
For each identical IPv4/IPv6, MTU, port-count and RTT combination it includes five
samples, median, minimum, maximum and median absolute deviation (MAD). The table
below pools the 60 samples in each RTT group; rates count inner IP packet bytes
in **one direction**, despite the simultaneous authenticated echo in the other direction.

| Simulated RTT | Baseline median Mb/s | Fixed median Mb/s | Baseline median of per-run p95 latency | Fixed median of per-run p95 latency |
| --- | ---: | ---: | ---: | ---: |
| 0 ms | 2,927.781 | 2,663.815 | 0.159 ms | 0.186 ms |
| 20 ms | 11.728 | 11.702 | 31.779 ms | 31.911 ms |
| 80 ms | 3.893 | 3.896 | 94.728 ms | 94.678 ms |

Both versions delivered 522,240 counted echoes. Aggregate process CPU was
2,062.5 ms (baseline) and 1,937.5 ms (fixed); do not interpret the coarse
per-sample zeros as zero work. Allocation calls were 2,642,760 and 2,643,121;
cumulative requested allocation bytes were 3,734,004,480 and 3,734,476,864.
These are cumulative allocation traffic, **not** peak resident memory. The
additional bounded directional queues add about two allocations per session.

The zero-RTT protocol-loop median is approximately **9.0% lower**, not an
improvement. The positive-RTT groups are essentially unchanged and constrained
by the fixed 32-packet window and Windows timer resolution. These measurements
include the shared slice-input compatibility path in both revisions; they do not
exercise the shipping final-stack `Bytes` ownership and `PacketBatch` reductions.
They therefore establish reproducibility and bounded progress, not faster
end-to-end Android networking. 本次不宣称真实 80 Mbps 问题已经解决，也不承诺目标速率。

## Not run / 未运行

- Windows ARM64 native release: installed Visual Studio lacks the ARM64 C++
  component; the helper fails with `Visual Studio C++ Build Tools for arm64 were not found`.
- Real Windows VPN/Wintun, route/DNS/WFP cleanup and installer lifecycle:
  `not_run`, no isolated snapshot VM.
- Android/TV device import-provider, Doze/Always-on/Lockdown, reboot, upgrade,
  platform handoff and network-change lifecycle: `not_run`, no dedicated device.
- External IPv4/IPv6/DNS leak observations: `not_run`, no isolated observer.
- Fixed-endpoint WARP-only / direct VPN / WARP→WireGuard performance, cold/hot DNS
  and the reported 80 Mbps symptom: `not_run`, no isolated performance setup.

Missing isolated evidence is neither a pass nor a publication prerequisite.
No MSI or release APK was made or installed; no official signing, push or
publication was performed. Generated JNI libraries, build products and raw logs
remain outside the committed source set.
