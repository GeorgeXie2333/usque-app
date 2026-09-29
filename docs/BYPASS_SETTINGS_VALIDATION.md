# Custom VPN bypass validation

Date: 2026-09-30. Source: baseline
`f94ae76c88410f5a99b27aea96d6d9f229a0d713` plus the uncommitted bypass-settings
changes in this working tree. This is not a tagged release or immutable release
evidence.

## Behavior and safety review

- Country selections and existing CIDRs remain available in **VPN bypass
  settings**. Custom IPv4/IPv6, CIDR and domain entries share the apply workflow.
  Domains include themselves and label-boundary subdomains; core normalizes IDNA.
- Schema 19 retains existing country/address settings. Local control protobuf
  fields 23 (domains) and 41 (capability) are append-only; Android JSON and Flutter
  maps retain the same field across storage and IPC. MASQUE wire framing and the
  frozen Go oracle are unchanged.
- Custom-only rules do not need GEO downloads. Enabled but unreadable GEO data
  cannot be masked by a custom match. IP-only rules do not enable synthetic DNS.
- The shared transport policy covers VPN and HTTP/SOCKS5 entrypoints; the current
  chain/L4 data-plane tests also ran. Hostname rules continue using the existing
  direct DNS resolver; address literals use explicit networks. Applications using
  their own encrypted DNS are only identifiable by addresses at the VPN boundary.
- No Agent privilege interface, broad WFP permit, system-proxy override, signing,
  installer or release workflow was added. Existing protected sockets and exact
  egress leases own direct flows. Rule changes use cold reconfiguration, which
  disposes session DNS hints; TTL, conflict and generation checks remain intact.
- New errors retain an entry ordinal without domain text. Diagnostics exports
  remain allowlisted. No target-list logging or automatic uploads were added.
- The editor preserves rejected drafts, maps backend domain errors to original
  line numbers, prevents unsupported-engine edits, and sends only edited fields.
  Advanced reset preserves targets now managed on the bypass page.

## Commands and results

Commands start at repository root unless noted. `flutter` and `dart` below refer
to `C:\Users\George\.local\share\flutter-3.44.7\bin\flutter.bat` and
`dart.bat`, respectively. The SDK was read from Android local.properties and
verified as Flutter 3.44.7, commit
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`. `python` is the bundled Python
executable under the Codex primary runtime; its Ruff module is 0.16.0.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed in aggregate source checks. |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed. |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | Passed; 1322 tests, 8 ignored by the existing suite. |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | Passed; compile-only. |
| `& .\tool\build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed. |
| GUI: `flutter pub get --enforce-lockfile` | Passed; lockfile preserved. |
| GUI: `dart format --output=none --set-exit-if-changed lib test` | Passed. |
| GUI: `flutter analyze --no-pub` | Passed. |
| GUI: `flutter test --no-pub` | Passed 693 tests, including Windows goldens. |
| GUI: `flutter test --no-pub test/bypass_settings_test.dart test/audit_forms_test.dart` | Passed 13 tests. |
| GUI: `flutter test --no-pub test/app_test.dart --plain-name 'advanced defaults preserve countries managed on their own page'` | Passed. |
| GUI: `flutter test --no-pub test/ui_workflow_golden_test.dart --name 'QUIC traffic policy\|settings_desktop_groups' --update-goldens` | Passed on the pinned Windows SDK. Three affected images reviewed. |
| GUI: `& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .` | Passed. |
| GUI: `flutter build windows --release --no-pub --split-debug-info=build/symbols/windows` | Passed; compile-only Windows x64 build. |
| GUI: `flutter build apk --debug --config-only --no-pub` | Passed; configuration only. |
| Android: `.\gradlew.bat --no-daemon :app:ktlintCheck` | Passed. |
| Android: `.\gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` | Passed. |
| `buf lint` | Passed. |
| `buf format --exit-code --diff` | Passed. |
| `pwsh -NoProfile -File tool/check_source.ps1` after Windows helper initialization | Passed with pinned Dart and Python directories on process PATH. Initial missing-Dart PATH failure was corrected; no checks were weakened. |
| `python tool/check_repository_policy.py` | Passed. |
| `git diff --check` | Passed. |

Focused coverage includes migration and reset, JSON/protobuf round trips,
normalization and invalid input, country-independent routing, network/domain
boundaries, DNS hint conflicts/generations, protected loopback connection and
failure fallback, Windows synthetic DNS selection, Android TUN identity, and
old-engine read-only UI. The complete widget/native-layout suites cover English
and Chinese, light/dark themes, 200% text, keyboard and D-pad paths using a fake
engine. Golden changes reflect the renamed settings entry, its new summary,
the moved CIDR editor and updated direct-DNS scope text.

## Not run

- Windows VPN/Wintun, WFP/route/DNS mutation and recovery: `not_run`; no snapshot
  VM with independent management was used.
- Real Android VPN lifecycle and Always-on/Lockdown: `not_run`; no isolated
  emulator or dedicated device was used.
- Externally observed IPv4/IPv6/DNS/direct-rule leak testing: `not_run`; no
  protected network observer was used.
- Controlled performance sampling: `not_run`; no performance lab was used.
- MSI packaging/install, release APK, signing and publication: not requested
  and not run. JNI/build output, logs and failure screenshots are not committed.

These unavailable isolated checks are not passes. Workstation tests and builds
do not prove native VPN cleanup or absence of traffic leaks.
