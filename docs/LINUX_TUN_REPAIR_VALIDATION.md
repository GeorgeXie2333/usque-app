# Linux TUN diagnostics and egress repair — 2026-10-20

This local engineering record covers the uncommitted working tree based on
`d795e995cac330e9155cea545e82de0823addbaf`. It does not establish the original
Ubuntu 24 amd64 incident's trigger or constitute exact-candidate release evidence.
The diagnostic archive established a platform-startup failure with insufficient
stage/timeline evidence; it did not identify a failing DNS adapter or physical
route. Historical test counts are not acceptance for this change.

## Batch 1: failure diagnosis and evidence ownership

The Agent appends optional bounded Linux details to protocol v3 errors. Initial
startup and protected replacement retain typed transport failures. Resolved and
NetworkManager probe rejections are preserved in that order; successful fallback
is successful startup. DNS ownership, application readback and compare/restore
rules remain mandatory. No backend switch follows a partially applied change.

Primary and cleanup failures remain separate, including immediate Agent rollback
and later Engine-owned rollback. A bounded optional cleanup detail transports the
Agent's immediate cleanup result without replacing the primary error. A separate
owner retains an asynchronous cleanup result without writing into later attempts.
Existing exports now distinguish attempted startup failure from a fresh, stale
or unavailable read-only platform observation. The ten-second freshness boundary
and Engine generation apply to current observations.

Privacy checks cover malicious message/stderr-like text, paths and addresses,
invalid enum/numeric details and physical DNS data. Only fixed enums and bounded
numbers enter failure evidence. Authentication failure closes IPC, with limited
fixed journal logging. EOF does not prove authentication rejection. No UI layout,
static DNS backend, UDP combination mode, journal schema or protocol version was
changed.

## Batch 2: physical route and network generation

Exact socket authorization uses non-dump RTM_GETROUTE for the Engine UID, actual
lease mark and socket tuple. The returned unicast OIF is validated, bound and
queried again. DNS retains its owner/scope constraint. Unsupported, unresolved,
loopback or self-TUN results fail without choosing a candidate NIC. Main-table
hints cannot establish family unavailability.

Private normalized route/rule fingerprints detect same-interface gateway,
metric and policy changes. Own tables, own TUN routes and cache bookkeeping are
excluded. Notifications include rules, with the five-second snapshot fallback.
Revocation and durable generation precede publishing a baseline. Failed reads
retain last-good data and suspend grants; failed persistence cannot publish new
evidence. Restarted active sessions require verified Resume and revocation.

Socket cleanup attempts both mark and Agent device-binding removal and verifies
readback. Unknown nft outcomes retain the FD and retire its mark. Revocation
removes authorization even when a socket refuses field cleanup. Recovery keeps
the host guard until dependent socket cleanup is confirmed. Fresh externally
bound sockets are rejected. Policy-only DNS discovery and rules that need an
unbound TCP source port remain outside the supported guarantees.

## Workstation validation

Windows and WSL Ubuntu 26.04 amd64 use Rust 1.97.1 and Flutter 3.44.7 at
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`. The Linux host is not the package
baseline and is not an isolated network VM. SDK paths in ignored local
configuration are restored after cross-platform checks.

Final command results are recorded below. Earlier failures
included incomplete locale catalogues, an out-of-date failure-code cardinality
fixture, sandbox/SDK-path tool failures and missing Windows plugin symlinks; these are not counted as passes.

| Gate | Command | Final result |
| --- | --- | --- |
| Rust formatting | `cargo fmt --all --check` | passed |
| Linux Rust static | `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed |
| Linux Rust tests | `cargo test --workspace --all-targets --locked` | passed: 1861 tests, three ignored |
| Windows Rust static | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | passed |
| Windows Rust tests | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | passed: 2019 tests, 13 ignored |
| Windows Rust release | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | passed |
| Linux Rust release | `cargo build --release --locked -p usque-engine -p usque-agent -p usque-update` | passed |
| Protocol | `buf lint`; `buf format --exit-code --diff`; `buf breaking --against '.git#ref=HEAD'` | passed |
| Aggregate static checks | `pwsh -NoProfile -File tool/check_source.ps1` | passed on WSL with the existing PowerShell executable |
| Generated contract | `python tool/generate_diagnostics_contract.py --check` | passed |
| Python lint | `ruff check tool`; `ruff format --check tool` | passed |
| Python tests | `python -m unittest discover -s tool -p "test_*.py" -v` | passed: 280 tests, 110 platform/tool skips |
| Linux package fixtures | `python -m unittest discover -s tool -p "test_linux_package.py" -v` | passed: 20 tests, one desktop-file-utils skip |
| Dart format | `dart format --output=none --set-exit-if-changed lib test` | passed |
| Flutter analysis | `flutter analyze --no-pub` | passed |
| Linux widgets | `flutter test --no-pub --exclude-tags golden` | passed: 1184 tests, one skip |
| Linux goldens | `flutter test --no-pub --tags linux-golden` | passed: 19 tests |
| Linux preview | `flutter build linux --debug --no-pub -t lib/main_preview.dart` | passed |
| Linux production | `flutter build linux --release --no-pub -t lib/main.dart` | passed |
| Linux native fixtures | CMake targets `usque_linux_socket_test`, `usque_proton_verification_test`, then their runner executables | passed |
| Windows Flutter | pinned `pub get --enforce-lockfile`, format, analyze, `flutter test --no-pub`, plugin-junction helper and release build from CONTRIBUTING | passed: 1276 tests, 19 skips; release compile passed |
| Android Rust | `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| Android Kotlin | debug config-only, `./gradlew --no-daemon :app:ktlintCheck`, `./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed; debug JNI prerequisites compiled for all three ABIs |
| Repository policy | `python tool/check_repository_policy.py`; `git diff --check` | passed; policy in complete source-only snapshot |

The policy check initially spent more than twenty minutes recursively walking
ignored local build/cache directories. It was interrupted and rerun with the
unchanged checker in a temporary source-only snapshot containing all tracked
files and non-ignored new source files. This preserves every checked-in policy
input and link target while excluding generated build caches. No rule or
fail-closed check was relaxed.

Batch 1 regressions passed in the final Rust suite: typed TUN/proxy transport
classification, dual DNS rejection and lazy fallback, primary/cleanup failure,
old and invalid Agent details, bounded numeric/enumeration validation, retained
startup evidence owners, cancellation/deadline dropping of pending owners, stale/wrong-generation platform observations
and secret/path/address exclusion from the archive.

Batch 2 regressions passed in the same final suite: lookup tuple/UID/mark/scope
encoding, single resolved output versus unsupported ECMP/route forms, own route
exclusion, gateway/metric/rule fingerprints, unchanged-interface generation,
repeated snapshot failure and recovery, failed journal persistence, retained FD
and mark-slot cleanup, DNS owner/scope and restarted/unavailable grant admission.
These fixtures establish source behavior, not native kernel or leak acceptance.

## Isolated acceptance and result limits

| Acceptance | Status |
| --- | --- |
| Ubuntu 24 amd64 snapshot VM: single/dual NIC, connect, switch, Agent restart and recovery | `not_run`: no verified snapshot VM with independent management channel |
| Independently observed IPv4/IPv6/DNS leaks | `not_run`: no independent network observer |
| Native Ubuntu 22.04 amd64/arm64 AppImage/deb/rpm baseline matrix | `not_run`: no matching baseline build nodes |
| Package installation, update, removal and publication | `not_run`: not requested |

The change repairs the confirmed source defects. It does not claim that the
reported Ubuntu 24 failure has been reproduced or fixed. 这些源码缺陷已修复；原始故障
触发点、隔离 VM 生命周期与独立泄漏观察仍需另行验收。
