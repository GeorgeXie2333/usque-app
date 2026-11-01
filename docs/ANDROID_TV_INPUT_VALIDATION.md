# Android TV text input validation

## Candidate and scope

Date: 2026-11-01. Windows x64 workstation; no Android TV device, Fire TV device,
or Android emulator was used. This covers the issue #148 working-tree changes
on `dev`, based on `30a7a6615e6a1034f4be0256b6ef8133850841d4`. The change was not
committed for this run; this record is not evidence for a later release candidate.

The source fingerprint below covers changed/new first-party Dart and Kotlin
implementation and test files relative to that base. It is SHA-256 of sorted
UTF-8 records `repository-relative-path + NUL + lowercase-file-SHA256 + LF`.
Documentation and generated build outputs are excluded.

Source files: 31. Source fingerprint:
`954e9a926fccc20bfaad544c9ba15dafb27d171f77fc48efd2776c3793606dc6`.

This initial candidate has the deferred-focus race documented in the follow-up
below. Its passing suites did not cover an intervening activation or navigation
within the same frame. Keep these results as the original record, not as proof
that the later race was absent.

## Behavior and boundaries

All production Material text inputs use the shared TV adapter. Navigation focus
does not open the input connection; explicit activation starts editing. The
first Back ends that session while retaining the draft and field focus. A later
Back follows existing nested-route, dialog, busy and unsaved-changes policies.
Phone and desktop inputs retain their ordinary behavior.

Android identifies TV mode from television configuration, Leanback or the Fire
TV system feature, not display size or manufacturer. The UI-only channel carries
an opaque editing session and lifecycle reasons, never text, credentials or
network configuration. Legacy pre-IME Back and modern system Back share a tested
policy. Whole legacy key sequences and stale callbacks cannot close the next
editing session. Initial zero IME insets do not imply editing has ended.

The adapter unregisters native Back callbacks on session completion, Activity
stop and destruction. Flutter revokes sessions when a field loses focus, is
disabled, is covered by another route or is disposed. Completing an IME action
does not restore focus over the destination chosen by a submission callback.
The existing framework view's input/inset callbacks remain installed.

## Commands and results

Flutter/Dart resolve from the SDK declared in the ignored Android
`local.properties`: Flutter `3.44.7`, framework
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`, matching CI. Python was the bundled
Python `3.12.14`; helper scripts used PowerShell 7.

| Command | Result |
| --- | --- |
| `flutter pub get --enforce-lockfile` | passed; lockfile unchanged |
| `dart format --output=none --set-exit-if-changed lib test` | passed |
| `flutter analyze --no-pub` | passed; no issues |
| `flutter test --no-pub test/tv_input_regression_test.dart test/tv_text_input_test.dart test/tv_navigation_test.dart --reporter expanded` | passed; 26 tests |
| `flutter test --no-pub` | passed; 1541 passed, 35 platform-conditioned skips |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | passed |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | passed |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2` | passed |
| `& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .` | passed |
| `flutter build windows --release --no-pub --split-debug-info=build/symbols/windows` | passed; final source rebuilt |
| `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| `flutter build apk --debug --config-only --no-pub` | passed; configuration only |
| `.\gradlew.bat --no-daemon :app:ktlintCheck` | passed |
| `.\gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed; 531 JVM tests, including 6 TV policy tests; no failures/skips |
| `pwsh -NoProfile -File tool/check_source.ps1` | passed in the Windows helper's initialized MSVC environment |
| `python tool/check_repository_policy.py` | passed in the independent 5751-file source snapshot; `REPOSITORY_POLICY_OK` |
| `git diff --check` | passed |

Flutter commands run in `apps/usque_gui`; Gradle commands run in its `android`
directory. Root helper commands run from the repository root. The aggregate
check used the pinned Flutter bin and bundled Python on process-local PATH, with
Ruff `0.16.0`, PSScriptAnalyzer `1.25.0` and Buf `1.72.0`.

The repository-policy checker recursively enumerates ignored build outputs in
the working directory. Those workstation scans were interrupted without a
result. Its unchanged exact command is therefore also run in an independent
source snapshot containing every tracked file plus the new source/documentation
files, without ignored build caches. That distinction is recorded in the table;
the interrupted original-directory scans are not reported as passes.

The initial DNS navigation regression test failed before the adapter was added:
D-pad focus requested the keyboard without an activation press. The final test
asserts the desired navigation behavior. Additional fixtures cover explicit
activation and repeats, native and Flutter Back, dirty nested pages, account
dialogs, stale/pending callbacks, password semantics, numeric formatting,
validation focus, Next, disabled/read-only fields, native-environment startup,
directional fallback, and English/Chinese light/dark input at 200% text size.

## P2 focus-restoration follow-up

Date: 2026-11-01, on the same `dev` base and workstation. This changes only
Flutter focus coordination and its regression tests; the Android adapter and
native policy remain unchanged. The updated working tree contains 32 changed/new
source and test files. Its fingerprint, using the same algorithm above, is
`26febe042cdf23bf460fff61b17348b79e3770934de17293627410757fddf2e6`.

Deferred restoration now captures a controller revision and the exact focus
node. New editing sessions, primary-focus changes, explicit directional
navigation and background lifecycle transitions invalidate that revision.
Before restoring, the post-frame callback flushes pending framework focus
requests and verifies that the original controller, node and revision are still
current. It cannot steal focus from a newer editor or a non-editor control.

Directional shortcuts consult live editing state through an action, so Back
followed by a D-pad event before the next build already uses navigation behavior.
While editing, that action is disabled and ordinary text-editing shortcuts keep
working.

All four new race regressions failed before the fix: activating another field
with immediate or delayed native acknowledgement, pressing D-pad before a
rebuild, and requesting button focus earlier in the same post-frame phase. A
fifth test confirms that arrows still move the cursor during multiline editing.

| Rerun command | Result |
| --- | --- |
| `flutter pub get --enforce-lockfile` | passed; pinned SDK reverified |
| `dart format --output=none --set-exit-if-changed lib test` | passed |
| `flutter analyze --no-pub` | passed; no issues |
| `flutter test --no-pub test/tv_focus_restore_test.dart test/tv_input_regression_test.dart test/tv_text_input_test.dart test/tv_navigation_test.dart --reporter expanded` | passed; 31 tests |
| `flutter test --no-pub` | passed; 1546 passed, 35 platform-conditioned skips |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | passed |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | first run failed in the renewal fixture below; complete rerun passed |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2` | passed |
| `& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .` | passed |
| `flutter build windows --release --no-pub --split-debug-info=build/symbols/windows` | passed |
| `pwsh -NoProfile -File tool/check_source.ps1` | passed in the Windows helper's initialized MSVC environment |
| `python tool/check_repository_policy.py` | passed in the independent 5752-file source snapshot |
| `git diff --check` | passed |

The first full Rust rerun failed in
`proton_vpn::renew::tests::due_certificate_renewal_uses_the_existing_session_and_publishes_the_same_identity`:
the worker returned `Err(Proton(ConnectionFailure { kind: Transport, ... }))`
instead of `Ok(())`. Rust source was unchanged in this follow-up.

An additional diagnostic command,
`& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy -Package usque-transport`,
failed on `dead_code` (`InternalResponse.last_modified`) and `clippy::never_loop`
in the standalone package's feature combination. The required workspace Clippy
command passed, including a rerun. No lint suppression or Rust source change
was made to obtain that result.

The renewal fixture was isolated with
`cargo test --workspace --all-targets --locked proton_vpn::renew::tests::due_certificate_renewal_uses_the_existing_session_and_publishes_the_same_identity -- --exact`
after initializing the supported native environment with the Windows helper;
the selected test passed. Its result does not replace the complete helper-based
test gate in the table.

The full Android JVM/lint and arm64 Rust checks were not repeated for this
Dart-only follow-up; their unchanged-source results remain in the initial table.
The aggregate check still includes Kotlin lint. Device/runtime limitations below
remain unchanged.

## Not run

- `not_run`: Android TV and Fire TV remote/IME runtime validation, including
  actual OS callback ordering, keyboard overlay behavior and VoiceView/TalkBack.
  JVM host doubles and Flutter semantics tests do not establish these results.
- `not_run`: Android emulator validation, as explicitly excluded for this task.
- `not_run`: Linux native build/goldens and Windows ARM64 build on this host.
- `not_run`: isolated VPN, leak, install, upgrade and platform-state restoration
  checks. No VPN, TUN, route, DNS, firewall or system-proxy changes were exercised.

No release APK/MSI was produced or installed, no signing identity was accessed,
and no issue comment, closure or publication was performed. Workstation
automation and compile results are not a claim of Fire OS IME compatibility.
