# WARP single-port scan / 单端口扫描验证

Date: 2026-09-24. Environment: Windows x64 development workstation.
Baseline: `ba76374fc255e92f4469e28dea13750a479d9156`; candidate: that commit
plus the uncommitted single-port enumeration, concurrent observation and UI
changes. This is working-tree evidence, not an immutable source snapshot or
release certification. Previous implementation and registration records remain
historical; the [guide](WARP_WIREGUARD.md) describes current behavior.

## Why the previous scan was slower

The reference is warpscout
[`b49ef5e8a466164a64d669951521b58944346bc8`](https://github.com/vernette/warpscout/tree/b49ef5e8a466164a64d669951521b58944346bc8).
Its [`flags.go`](https://github.com/vernette/warpscout/blob/b49ef5e8a466164a64d669951521b58944346bc8/flags.go)
defaults to ten ordinary tunnel workers and a two-second request deadline;
`-through` reduces the default worker count to one. Its
[`tunnel.go`](https://github.com/vernette/warpscout/blob/b49ef5e8a466164a64d669951521b58944346bc8/tunnel.go)
normally stops at the first working port for an IP, using ports selected by
its preliminary reachability phase. A worker reuses its WireGuard device.

Usque previously enumerated every selected IP/port pair, including remaining
ports after success, with a new candidate tunnel and confirmed cleanup for each.
Candidate tunnels are serial over the shared MASQUE underlay. Each validates
HTTPS trace and metadata for both available address families, with a three-second
handshake deadline and five seconds per HTTPS request. Trace and metadata were
sequential within each family. These are source-level differences, not a
measurement of how much each contributed on the reporting user's network.
When disconnected, temporary MASQUE startup also adds latency before the first
probe; creating the independent scan identity adds a one-time registration.
Neither is repeated for each IP in the same task.

## Implemented rule and coverage

The user selected **one common port per IP**, including after failures. Pool
scans rotate 2408/500/1701/4500 across successive saved IPs; the single-IP mode
uses 2408 only. No other port of that IP is attempted during the task. This can
miss an IP that only works on a different port. Endpoint overrides remain
editable independently of scanning.

| Mode | Previous candidates | Current candidates |
| --- | ---: | ---: |
| Quick IPv4 | 280 | 70 |
| Quick IPv6 | 40 | 10 |
| Single IPv4/IPv6 address | 54 | 1 |
| Full IPv4 pool | 193,536 | 3,584 |

These counts describe scheduled candidates, not measured probes per second or
wall-clock speedup. Full IPv6 enumeration remains unavailable.

- Each encrypted job stores `single_port_v1` and its sampled IP list. Cursor,
  total, result indexes, pagination and per-IP port assignment survive reload.
  Progress includes unsuccessful IPs without inflating the count with skipped
  ports. Existing bounded result blocks and country indexes are retained.
- Records lacking a plan keep their old IP/port mapping for historical reads.
  Resume returns `scan_plan_changed` before starting a worker; old results stay
  available, and a new scan uses the new rule. Tests cover both legacy decoding
  and refusal to continue the exhaustive plan without deleting its results.
- Trace and metadata overlap inside each candidate, while candidate tunnels
  remain serial. A virtual-clock test with 200 ms trace and 300 ms metadata
  completes in 300 ms while reporting 200 ms trace latency. This is a scheduling
  regression fixture, not a live performance measurement.
- Invalid or failed trace drops pending metadata and cannot establish a usable
  endpoint. Metadata failure preserves valid trace data with unknown country.
  IPv4/IPv6, observed country versus node, cancellation, the 3/5/20-second
  deadlines and cleanup before the next candidate retain their existing rules.
- The interface explains the one-port assignment and legacy-task restart in
  all 21 catalogs. The Chinese target label no longer promises a deep port
  sweep; full-scan copy no longer assumes a multi-day port enumeration.

No dependency, protobuf field or OS networking path was added. Requests still
use the specified userspace tunnel, with no direct-network fallback. No account
key, response body or new endpoint-bearing log is introduced. Parallel HTTPS
is bounded to trace/metadata for the two available families within one tunnel.

## Validation

| Check | Result |
| --- | --- |
| Windows helper locked Rust Clippy | Passed |
| Windows helper locked workspace tests | 1,260 passed; 8 ignored under their existing conditions |
| Windows helper x64-v2 release build | Passed |
| Android arm64-v8a Rust Clippy | Passed with pinned NDK/CMake |
| Flutter locked resolution, format and analysis | Passed |
| Flutter complete Windows widget/golden suite | 654 passed |
| Windows Flutter release build and Android debug configuration | Passed; compile/configuration only |
| Android Kotlin unit tests, ktlint and lint | Completed without failures; Gradle reused 223 up-to-date JVM test results rather than re-executing them, with no failures/errors/skips |
| Aggregate source checks, including Buf lint/format | Passed |
| Repository policy and diff whitespace | Passed |

Two intended golden images were regenerated with the pinned Windows SDK and
visually reviewed: English/light and Chinese/dark scan panels. Both grow by
38 pixels for the new rule hint; content above the hint is pixel-identical.
The scoped widget run also covered 200% text and TV remote selection. The
remaining source-picker/shared-page goldens passed without baseline changes.

The eight ignored Rust cases still require live credentials/profiles, an
explicit live endpoint or isolated Wintun loading. They are excluded from the
passed count. Generated files and logs remain in ignored build directories.

## Commands

Rust 1.97.1 and Flutter 3.44.7
(`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`) follow the checked-in pins. Android
uses NDK 29.0.14206865 and SDK CMake 3.22.1. Commands follow
[Contributing](../CONTRIBUTING.md); `flutter` and `dart` denote the executables
under the SDK resolved from `local.properties`, and Python is the bundled
verified runtime.

From the repository root:

```powershell
cargo fmt --all --check
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2
& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy
```

From `apps/usque_gui`:

```powershell
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub test/warp_wireguard_test.dart test/chain_proxy_test.dart
flutter test --no-pub --update-goldens test/warp_wireguard_test.dart --tags golden
flutter test --no-pub
& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .
flutter build windows --release --no-pub
flutter build apk --debug --config-only --no-pub
```

The first scoped run identified the intended hint-related golden mismatch;
only the two affected baselines were then regenerated and reviewed. The final
full-suite result is the acceptance result, without `--update-goldens`.

From `apps/usque_gui/android`:

```powershell
.\gradlew.bat --no-daemon :app:ktlintCheck
.\gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug
```

From the root, with the pinned tools on PATH and the native environment
initialized through the Windows helper in the same shell:

```powershell
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy -Package usque-transport
pwsh -NoProfile -File tool/check_source.ps1
python -X utf8 tool/check_repository_policy.py
git diff --check
```

## Not run / 未运行

| Validation | Status and reason |
| --- | --- |
| Live scan throughput and comparison with warpscout on the same network | `not_run`: no dedicated MASQUE live-test configuration supplied |
| Live country observations, endpoint usability and registration | `not_run`: requires the live test configuration; simulated scheduling does not prove these results |
| Android/TV VPN, Doze, Lockdown and process recovery on device | `not_run`: no dedicated device or isolated emulator |
| Windows VPN/platform restoration and external IPv4/IPv6/DNS leak checks | `not_run`: requires the snapshot VM and independent network observer |
| Installation packages, signing, installation or publication | `not_run`: outside this delivery's scope |

扫描量已经按每个 IP 一个常用端口减少，但没有进行真实网络速度对照，不能把
候选数量的减少倍数直接当作提速倍数。所有网络观测及隔离环境验收仍需分别实测。
