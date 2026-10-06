# Zero Trust endpoint editing validation

Date: 2026-10-06. Issue: [#92](https://github.com/GeorgeXie2333/usque-app/issues/92).

The tested source was baseline `d76cd42fa6b6e586c22a813857409612a3765051`
plus the uncommitted endpoint-editing changes. This record does not identify a
uniquely recoverable complete source snapshot and is not release evidence.
The feature remains experimental.

## Behavior and safety review

- Each Advanced settings page visit starts with ZT IPv4/IPv6 locked. A red
  fullscreen warning requires an explicit risk and access-authorization checkbox.
  Cancel, Escape and Back decline. Confirmation is held only in memory for that
  page and account; account changes invalidate late confirmation replies.
- Schema 23 retains `managed_endpoint_ips` and adds an optional account-specific
  `zero_trust_endpoint_override`. Masked saves preserve other accounts and the
  shared Consumer address pair and selection. Reset stages registration-owned
  addresses; applying them clears the override. Successful registration also
  clears it. Interrupted credential replacement preserves the previous pair
  and override alongside the existing credential rollback.
- Capability field 45 gates the UI; catalog fields 8 and 9 carry registered
  addresses. Older engines stay read-only. Missing registration cannot authorize
  editing or silently substitute Consumer defaults. Address-family validation,
  registration validation, TLS and enrolled public-key pin checks remain active.
- Applying addresses uses the existing controlled cold-reconnect path, retaining
  its session ownership and fail-closed platform cleanup rules. No new privileged
  networking operation, logging, telemetry or diagnostic upload was added.
- UI tests use a fake engine. Rust deterministic tests use inert backends,
  memory peers and permitted loopback networking. Neither establishes real VPN
  restoration, external leak behavior or real-organization compatibility.

## Workstation checks

Commands below use the pinned SDKs and tools resolved as in
[Contributing](../CONTRIBUTING.md). Python was the verified bundled Python
3.12.14; Flutter was 3.44.7, full commit
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`, on Windows x64.

| Directory | Command | Final result |
| --- | --- | --- |
| Root | `cargo fmt --all --check` | Passed in the aggregate gate |
| Root | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed |
| Root | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | Passed; includes storage, masks, cold application, identity recovery and wire snapshots |
| Root | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | Passed; compile-only |
| Root, helper-initialized environment | `pwsh -NoProfile -File tool/check_source.ps1` | Passed, including Rust/Dart, Kotlin, pinned Ruff, PSScriptAnalyzer and Buf |
| `apps/usque_gui` | `flutter pub get --enforce-lockfile` | Passed; lockfile unchanged |
| `apps/usque_gui` | `dart format --output=none --set-exit-if-changed lib test` | Passed |
| `apps/usque_gui` | `flutter analyze --no-pub` | Passed; no issues |
| `apps/usque_gui` | `flutter test --no-pub` | Passed; 930 widget/model/golden tests |
| `apps/usque_gui` | `flutter test --no-pub test/ui_workflow_golden_test.dart --plain-name 'ZT warning golden' --update-goldens` | Passed; three new Windows-pinned baselines generated and visually reviewed |
| `apps/usque_gui` | `& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .` | Passed |
| `apps/usque_gui` | `flutter build windows --release --no-pub --split-debug-info=build/symbols/windows` | Passed; compile-only |
| Root | `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed |
| `apps/usque_gui` | `flutter build apk --debug --config-only --no-pub` | Passed; configuration-only |
| `apps/usque_gui/android` | `.\gradlew.bat --no-daemon :app:ktlintCheck` | Passed |
| `apps/usque_gui/android` | `.\gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` | Passed |
| Root | `buf lint` | Passed |
| Root | `buf format --exit-code --diff` | Passed |
| Root | `buf breaking --against '.git#ref=d76cd42fa6b6e586c22a813857409612a3765051' --against-config buf.yaml` | Passed |
| Root | `python tool/check_repository_policy.py` | Passed |
| Root | `git diff --check` | Passed |

The new workflow regressions cover refusal, acknowledgement, repeated opening,
page/account lifetime, failed persistence, registration reset and unavailable
engines/metadata. Accessibility checks cover English, Simplified Chinese and
Persian RTL at 200% text, narrow and landscape layouts, keyboard and Android
D-pad activation, semantics labels, target size and danger-surface text contrast.
The new screenshots cover desktop dark, phone light and Persian landscape.

An existing ignored generated screenshot test under
`apps/usque_gui/build/readme_screenshots/render_readme_test.dart` was causing
analyzer warnings. It was preserved at
`target/preserved-readme-screenshots/render_readme_test.dart`, outside the GUI
analysis tree. Generated JNI libraries, build outputs and logs remain ignored.

## Validation not run

| Environment | Status | Reason |
| --- | --- | --- |
| Dedicated organization and isolated Android/TV device or emulator | `not_run` | Real enrollment, VPN lifecycle, reconnect and device behavior were not exercised on this workstation |
| Windows snapshot VM and external network observer | `not_run` | Live tunnel cleanup, restoration and IPv4/IPv6/DNS leak observation require the distinct isolated environments |
| Ubuntu Flutter widget job | `not_run` | This session used Windows; the hosted Ubuntu suite remains a CI check |

Protected-runner evidence is supplemental and non-blocking. These unavailable
checks are not passes, and the local builds are not official release packages.

## Persistent Home notice follow-up — 2026-10-06

This follow-up implements the
[persistent-warning suggestion](https://github.com/GeorgeXie2333/usque-app/issues/92#issuecomment-6012739125).
The tested source was `cceed2a5fb0f2c36f4b023e220d5b355df8e803a` plus
uncommitted Home-warning changes; this likewise does not uniquely identify the
complete source snapshot. The earlier validation counts above remain historical.

Home now shows a non-dismissible danger banner above the connection controls when
the selected account has saved custom ZT addresses, or a connected/transitional
session still uses custom ZT addresses. A saved restore or account switch cannot
hide the warning while that custom session remains active. Registered addresses,
ordinary WARP, port/SNI-only edits and equivalent IPv6 spellings do not trigger
it. Missing registration metadata does not classify unknown addresses as custom.
The notice reads existing confirmed settings and identity metadata; it adds no
native API, probe, timer, persistence, logging or telemetry. All 21 catalogs and
the parallel user guides describe the warning.

| Directory | Command | Result for this follow-up |
| --- | --- | --- |
| Root | `cargo fmt --all --check` | Passed |
| Root | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed |
| Root | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | Passed |
| Root | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | Passed; compile-only |
| `apps/usque_gui` | `flutter pub get --enforce-lockfile` | Passed with the same pinned SDK and unchanged lockfile |
| `apps/usque_gui` | `dart format --output=none --set-exit-if-changed lib test` | Passed |
| `apps/usque_gui` | `flutter analyze --no-pub` | Passed; no issues |
| `apps/usque_gui` | `flutter test --no-pub test/home_zero_trust_endpoint_risk_test.dart` | Passed; 13 focused regressions |
| `apps/usque_gui` | `flutter test --no-pub test/ui_workflow_golden_test.dart --plain-name 'ZT Home risk golden' --update-goldens` | Passed; three new Windows-pinned Home screenshots visually reviewed |
| `apps/usque_gui` | `flutter test --no-pub` | Passed; 948 tests including all goldens |
| `apps/usque_gui` | `& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .` | Passed |
| `apps/usque_gui` | `flutter build windows --release --no-pub --split-debug-info=build/symbols/windows` | Passed; compile-only |
| Root | `python tool/check_repository_policy.py` | Passed |
| Root | `git diff --check` | Passed |

Regressions cover permanent display, confirmed save/restore updates without page
recreation, account switching, retained custom applied sessions, transitional
phases, ordinary WARP, numeric address equivalence, unavailable metadata,
200% text, semantics and Persian RTL. Screenshots cover desktop dark, phone light
and Persian landscape. The fake engine does not start a native VPN.

The current ignored screenshot helper
`apps/usque_gui/build/readme_screenshots_test.dart` was preserved at
`target/preserved-readme-screenshots/readme_screenshots_test.dart` to keep
generated screenshot code out of GUI analysis. No tracked helper was changed.

Live organization/VPN/device/TV and Ubuntu-runner validation remain `not_run`.
This UI change does not alter native networking or protocol behavior, and the
existing isolation and release-evidence limits still apply.
