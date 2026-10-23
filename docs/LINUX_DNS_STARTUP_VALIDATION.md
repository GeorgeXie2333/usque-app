# Linux DNS snapshot and startup repair — 2026-10-23

This engineering record covers the uncommitted changes based on
`983999a05430f2763927637ff56dfa551b52a54b`. It is not a release-candidate
attestation. The user's affected host is a separate Debian 13 amd64 physical
machine, not this workstation's WSL environment or the reporter of issue #126.
The earlier Ubuntu 24 investigation and October 20 validation remain historical.

The 39 changed non-Markdown files have SHA-256
`8952f20d3d7b38c4d5946904f742ab833a55c73aa7b8b92567df1d18735609c3`.
This hashes sorted repository-relative UTF-8 paths, a NUL, exact working-tree
file bytes and a NUL for each changed/new file, excluding ignored files. The
Linux validation copy matched all changed source bytes. Documentation is
excluded from this code fingerprint and checked separately; the base plus this
fingerprint identifies the code validated here, not a signed release artifact.

The supplied beta.2 archive records a physical-snapshot failure after 143 ms:
command rejected, busctl, exit 1, with no selected-backend detail. The Agent is
clean at journal generation zero and cleanup completed. The evidence places the
failure before TUN/firewall/route application and transport handshaking, but does
not identify the exact failed property. The archive has no GUI timing evidence.

## Batch 1: NetworkManager DNS snapshot

The IPv6 snapshot now reads `IP6Config.Nameservers` (`aay`); the IPv4 snapshot
continues to read `IP4Config.NameserverData` (`aa{sv}`). IPv6 records require
exactly 16 bytes in range. Empty lists and absent-family `/` configuration paths
are accepted. Filtering, deduplication, the eight-address limit and physical
DNS owner/scope checks remain in force. Malformed replies and read errors stop
preparation without applying privileged state or switching a partially applied
DNS backend.

`LinuxFailureDetail.dns_snapshot_operation = 9` adds a fixed operation enum.
Snapshot errors retain the selected backend, command adapter and bounded exit
status/errno. Unknown optional operations normalize to unspecified without
discarding the older failure fields. Agent protocol v3 is unchanged. No raw
busctl stderr, paths, property values or DNS addresses enter diagnostic evidence.

The regression suite checks both real D-Bus property shapes, absent/empty
families, invalid byte lengths/ranges, filtering and duplicate removal, command
failure evidence, unknown-operation compatibility, and clean coordinator state
after a snapshot rejection. A private D-Bus daemon and synthetic NetworkManager
service exercise the production parser through real `busctl`; the fixture
never contacts the host system bus. The old IPv6 property does not exist in
this fixture, so the former implementation fails this regression.

## Batch 2: responsive startup and bounded observations

Production Linux registers an application-owned preferences adapter. Native
workers perform reads and durable writes; Dart isolates encode/decode JSON.
The existing application-ID/legacy-executable directory choice, file format,
prefix filtering and unknown keys are retained. Writes are serialized, a missing
migration key does not trigger a write, and normal exit drains queued changes.
An unreadable or malformed saved file produces a retryable loading error without
overwriting the file or resetting onboarding. Atomic replacement creates a
private `0600` file, including when replacing a legacy `0644` file.

Controller initialization starts after the first frame. Native desktop
preferences, icon, installation-record and autostart reads run off GTK. Event
socket connection/authentication also runs on a worker; cancelled, superseded
or destroyed owners cannot publish a late socket, and obsolete FDs are closed.
Flutter remains pinned to 3.44.7 with its existing runner thread policy.

The GUI supplies optional export request field 3 and the Engine writes a
separate `gui-startup.json`. The first 60 seconds contain at most 16 fixed
stage/source/status records with relative milliseconds and optional Dart/GTK
maximum loop gaps. Missing observations are unavailable; invalid observations
are rejected independently of the rest of the ZIP. These are GUI-process
observations, not authoritative platform state or proof of absence of stalls.

Tests cover blocked storage with live Dart timers, unknown-key preservation,
no-op removal, ordered writes, save failure, invalid-JSON retry, first-frame
startup/error retry, zero versus absent timestamps and bounded timeout evidence.
Native fixtures hold disk and socket workers for two seconds while the GLib
loop continues, and check legacy paths, file mode, durable acknowledgement,
exit draining, cancellation, owner destruction and overlapping connections.

## Validation

Windows amd64 and a native-filesystem source copy under WSL Ubuntu 26.04 amd64
use Rust 1.97.1 and Flutter 3.44.7 at
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`. WSL provides deterministic checks and
compilation only; it is neither the affected Debian machine nor an isolated VPN
acceptance environment. Commands below run from the repository root unless the
GUI or Android directory is stated. Flutter commands use the SDK resolved from
ignored `android/local.properties`; both hosts completed locked pub resolution.

| Gate | Exact command | Result |
| --- | --- | --- |
| Rust format | `cargo fmt --all --check` | passed |
| Linux Rust static | `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed |
| Linux Rust tests | `cargo test --workspace --all-targets --locked` | passed: 1902 passed, four ignored; private D-Bus test run separately |
| Linux sidecars | `cargo build --release --locked -p usque-engine -p usque-agent -p usque-update` | passed |
| Windows Rust static | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | passed |
| Windows Rust tests | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | passed: 2052 passed, 13 ignored |
| Windows Rust release | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | passed |
| Protocol | `buf lint`; `buf format --exit-code --diff`; `buf breaking --against '.git#ref=HEAD'` | passed |
| Generated diagnostic contract | `python tool/generate_diagnostics_contract.py --check` | passed |
| Python static | `ruff check tool`; `ruff format --check tool` | passed |
| Python tests | `python -m unittest discover -s tool -p "test_*.py" -v` | passed: 308 discovered, 110 platform/tool skips |
| Linux package contracts | `python -m unittest discover -s tool -p "test_linux_package.py" -v` | passed: 20 discovered, one desktop-file-utils skip |
| Workflow | `actionlint -no-color` | passed with 1.7.12 |
| Flutter dependencies, both hosts (GUI) | `flutter pub get --enforce-lockfile` | passed |
| Dart format, both hosts (GUI) | `dart format --output=none --set-exit-if-changed lib test` | passed |
| Flutter analysis, both hosts (GUI) | `flutter analyze --no-pub` | passed |
| Linux widgets (GUI) | `flutter test --no-pub --exclude-tags golden` | passed: 1200 passed, one skip |
| Linux goldens (GUI) | `flutter test --no-pub --tags linux-golden` | passed: 19 passed |
| Linux preview (GUI) | `flutter build linux --debug --no-pub -t lib/main_preview.dart` | passed |
| Linux production (GUI) | `flutter build linux --release --no-pub -t lib/main.dart` | passed |
| Windows widgets/goldens (GUI) | `flutter test --no-pub` | passed: 1292 passed, 19 skipped Linux goldens |
| Windows plugin junctions (GUI) | `& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .` | passed |
| Windows Flutter release (GUI) | `flutter build windows --release --no-pub --split-debug-info=build/symbols/windows` | passed |
| Android Rust | `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| Android configuration (GUI) | `flutter build apk --debug --config-only --no-pub` | passed |
| Kotlin style (Android) | `./gradlew.bat --no-daemon :app:ktlintCheck` | passed |
| Kotlin tests/lint (Android) | `./gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed, including debug JNI prerequisites |
| Aggregate checks | `pwsh -NoProfile -File tool/check_source.ps1` | passed in the Linux source copy |
| Repository policy | `python tool/check_repository_policy.py`; `git diff --check` | passed in a complete source-only snapshot |

All native fixture commands below completed successfully:

```sh
cmake --build apps/usque_gui/build/linux/x64/release --target usque_linux_socket_test
apps/usque_gui/build/linux/x64/release/runner/usque_linux_socket_test
cmake --build apps/usque_gui/build/linux/x64/release --target usque_proton_verification_test
apps/usque_gui/build/linux/x64/release/runner/usque_proton_verification_test
cmake --build apps/usque_gui/build/linux/x64/release --target usque_linux_preferences_test
apps/usque_gui/build/linux/x64/release/runner/usque_linux_preferences_test
cmake --build apps/usque_gui/build/linux/x64/release --target usque_linux_startup_io_test usque_nm_dns_fixture
apps/usque_gui/build/linux/x64/release/runner/usque_linux_startup_io_test
USQUE_NM_DNS_FIXTURE="$PWD/apps/usque_gui/build/linux/x64/release/runner/usque_nm_dns_fixture" \
  cargo test --locked -p usque-agent --lib private_network_manager_bus -- --ignored
```

Earlier failed runs are not counted as passes. The native file-mode test exposed
GLib's preservation of the old file mode; the writer was corrected and rerun.
Three existing Python/Node fixtures timed out during concurrent compilation;
the full unchanged Python suite then passed separately with its original
timeouts. An ignored local command wrapper initially constructed the wrong
fixture path; the corrected explicit private-bus command passed. Windows Buf's
temporary-clone sandbox error was resolved by running the same check with the
necessary temporary-file access. No assertion or failure-protection rule was
weakened.

The unchanged repository policy checker ran in a temporary source-only snapshot
of all 5574 tracked/non-ignored files, preserving policy inputs and link targets
without recursively scanning generated build caches. Build products, raw logs,
diagnostic archives and local SDK configuration remain ignored.

## Acceptance still required

| Environment / behavior | Status |
| --- | --- |
| Affected physical Debian 13 amd64: cold/warm startup, drag/minimize responsiveness, retries and original immediate TUN error | `not_run`: host unavailable |
| Isolated Linux snapshot VM: real NetworkManager/resolved, single/dual NIC, connect/disconnect, network change, Agent restart and recovery | `not_run`: snapshot and independent management channel unavailable |
| Independent IPv4/IPv6/DNS/kill-switch observation | `not_run`: network observer unavailable |
| Native Ubuntu 22.04 amd64/arm64 package baseline and signed package verification | `not_run`: required native runners unavailable; Ubuntu 26.04 compilation does not establish that baseline |

No application package was installed or published, no live TUN/DNS/route/firewall
or proxy mutation was performed, and no official signing material was accessed.
These changes fix verified source defects and pass the listed local regressions.
They do not yet establish that the user's original physical-machine symptoms
have been resolved.
