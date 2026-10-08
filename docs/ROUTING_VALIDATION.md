# Routing implementation validation — 2026-10-08

This record covers the REJECT, Ads and unified-routing implementation on a
Windows development workstation. The source was the uncommitted working tree
based on `db1e5c8f396e5162973a52aa84e8b806aa3dcb09`. This record does not identify
an immutable commit containing all tested changes and is not release-candidate
or protected-runner evidence. See [Routing](ROUTING.md) for behavior and
[Contributing](../CONTRIBUTING.md) for the required checks and safety boundaries.

The initial check results below are retained as a checkpoint. The subsequent
independent review found defects not covered by that initial suite; the final
review corrections and revalidation are recorded later in this document.

## Initial commands and results

Commands below start at the repository root unless a directory is specified.
`flutter` and `dart` resolve to
`C:\Users\George\.local\share\flutter-3.44.7\bin`; the verified Flutter commit
is `84fc5cbb223bc12f83d65b647ff8a56caf779ffd`. Python resolves to
`C:\Users\George\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe`.
Rust is pinned to 1.97.1. Native helpers use their checked-in locked dependency,
MSVC, CMake and NDK selection rules. Logs remain in the ignored `target/`
directory and are not publication artifacts.

| Directory | Exact command | Result |
| --- | --- | --- |
| Root | `cargo fmt --all --check` | Passed in the initialized aggregate run |
| Root | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed |
| Root | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | Passed: 1,681 tests; 8 ignored by their existing conditions |
| Root | `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | Passed; compile only |
| GUI | `flutter pub get --enforce-lockfile` | Passed; lockfile preserved |
| GUI | `dart format --output=none --set-exit-if-changed lib test` | Passed: 230 files, no changes |
| GUI | `flutter analyze --no-pub` | Failed: 3 existing warnings in an ignored build-directory script; see below |
| GUI | `flutter analyze --no-pub lib test` | Passed; supplemental source check, not a replacement for full analysis |
| GUI | `flutter test --no-pub` | Passed: 1,003 tests, including Windows goldens |
| GUI | `& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .` | Passed |
| GUI | `flutter build windows --release --no-pub --split-debug-info=build/symbols/windows` | Passed; compile only |
| Root | `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed; no JNI output copied |
| GUI | `flutter build apk --debug --config-only --no-pub` | Passed; configuration only |
| GUI/android | `.\gradlew.bat --no-daemon :app:ktlintCheck` | Passed |
| GUI/android | `.\gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` | Passed |
| Root | `python -m ruff check tool` | Passed, Ruff 0.16.0 |
| Root | `python -m ruff format --check tool` | Passed: 29 files |
| Root | `Invoke-ScriptAnalyzer -Path tool -Recurse -Settings tool/PSScriptAnalyzerSettings.psd1` | Passed, PSScriptAnalyzer 1.25.0; findings explicitly checked |
| Root | `Invoke-ScriptAnalyzer -Path tool -Recurse -IncludeRule PSUseCorrectCasing` | Passed; findings explicitly checked |
| Root | `buf lint` | Passed, Buf 1.72.0 |
| Root | `buf format --exit-code --diff` | Passed; Git's GNU diff directory on process PATH |
| Root | `buf breaking --against '.git#ref=HEAD'` | Passed against the local base; supplemental, not the hosted PR-target check |
| Root | `pwsh -NoProfile -File tool/check_source.ps1` | Failed at Flutter analysis with the same 3 pre-existing warnings; preceding Rust and Dart checks passed |
| Root | `python tool/check_repository_policy.py` | Passed |
| Root | `git diff --check` | Passed |

GUI means `apps/usque_gui`; GUI/android means `apps/usque_gui/android`.
The aggregate runs after the Windows Rust helper initializes the native
environment in the same PowerShell session. Individual checks after its failing
stage are run separately; a failed aggregate is never reported as a pass.

The 8 ignored Rust tests comprise 2 isolated Wintun loading tests, 4 explicitly
opted-in live credential/network tests, 1 optional supplied OpenVPN profile test,
and 1 controlled performance benchmark. None was enabled as a workstation
workaround. The existing frozen Go reference and third-party source were not
modified.

## Environment findings

Full Flutter analysis reports `invalid_use_of_visible_for_testing_member` at
lines 67, 68 and 110 of the pre-existing, ignored
`apps/usque_gui/build/readme_screenshot_render_test.dart`. These invoke
`debugReset`/`debugEnable` outside an analyzer-recognized test directory.
The file is preserved. No analyzer exclusion, severity suppression or generated
baseline was added. Application and test sources pass scoped analysis.
An earlier aggregate invocation exceeded the Windows batch command-line limit
after repeated native environment setup. The final run used a fresh initialized
session and deduplicated only its process PATH; it reached Flutter analysis and
reported the warnings above. The system PATH was not changed.

During fresh native binding compilation, the pinned NDK's `libclang.dll` could
not locate its existing `libwinpthread-1.dll` dependency from the Cargo build
script process. The identical, hash-verified NDK DLL was copied only beside the
affected `boring-sys` build-script executables inside ignored Cargo build-cache
directories. Fresh binding generation then completed. No third-party source,
toolchain version, system PATH, certificate store or installed application was
changed. The release build can emit the existing quiche linker-output warning
for creation of its import library; this is distinct from Clippy's successful
`-D warnings` result.

## Coverage and review

- Normalization and rule selection: IDNA, casing/trailing dots, domain label
  boundaries, IPv4/IPv6 longest prefix, mapped addresses, duplicates, exact
  conflicts, nested exceptions, stable IDs, schema migration and persistence.
- Configuration boundaries: JSON/protobuf round trips, explicit empty versus
  missing routing, legacy-client write rejection, field-scoped updates and
  shared network settings.
- Transport: local HTTP 403 and SOCKS5 0x02 rejection, UDP drops, candidate
  filtering, no rejection fallback, pre-pool/pre-flow checks and existing
  direct-path cleanup. Memory-only TUN tests cover IPv4/IPv6 rejection and
  synthetic DNS; random malformed IP packets must not panic or create oversized
  replies. Fragment/extension-header destination checks do not depend on NAT
  parsing.
- DNS/Ads: local REFUSED without upstream admission, CNAME checks, mixed
  questions, shared-IP hints, TTL/generation handling, bounded complete-category
  matching, malformed-catalog property tests and last-valid-data retention.
- UI: conflict row identification, parent/child notices, failed-save draft
  retention, older-engine read-only state, missing-Ads behavior, keyboard and
  D-pad action selection, large text and localization catalog checks. Added
  English desktop, Chinese dark phone and English TV 200% routing goldens;
  refreshed two affected settings goldens and visually reviewed all five.

## Not run and scope limits

| Validation | Status / reason |
| --- | --- |
| Native Windows VPN/TUN, WFP, route/DNS restoration and crash recovery | `not_run`: no approved snapshot VM with independent management channel |
| Android device/emulator VPN lifecycle and TV device interaction | `not_run`: no designated isolated device; Kotlin/widget checks do not establish device behavior |
| External IPv4/IPv6/DNS/direct-rule leak observation | `not_run`: no designated network observer |
| Controlled repeated performance sampling | `not_run`: no performance lab |
| Three-ABI JNI build, APK packaging, MSI packaging/install, signing and publication | `not_run`: outside the requested compile-only scope |

No installed VPN was started, no generated package was installed, and no system
networking or signing state was exercised. Functional tests use fake engines,
memory paths or loopback peers. Missing isolated evidence remains `not_run`.

## Review corrections and revalidation — 2026-10-08

Three subagents independently reviewed transport, configuration/Geo data, and
Flutter/Android integration. The main agent verified the findings and performed
the fixes; the same reviewers then checked their assigned corrections again.
Their final targeted reviews reported no remaining findings in those areas.
Review is source evidence, not native VPN or external leak validation.

The corrections cover the Android routing capability bridge; authoritative
validation codes and rule IDs; visible conflict messages and uncertain-save
state; transactional legacy-client protection; Ads fallback preservation after
a failed primary write; explicit DNS-hint lifetimes; CNAME validation in all
handled reply paths and at the depth limit; candidate filtering before bounds;
and the SOCKS DNS relay's resolved IP routing. DNS server selection finishes
before routing validation, so policy refusal cannot trigger another resolver.
Raw SOCKS UDP DNS replies are correlated with their actual server endpoint,
transaction and questions, with a four-query, 4 KiB-per-query, four-second bound.

Regression tests include concurrent field saves while a legacy request owns the
lifecycle lock; both Ads update entry points failing to replace the primary;
IDNA conflicts through Android native validation and the Kotlin/Flutter error
bridge; conflict correction followed by an unconfirmed save; multiple DNS
servers with the same transaction; blocked CNAME replies over raw SOCKS UDP and
L4 TUN memory paths; IP rejection through edge DNS; protected IP DIRECT before
fallback; and allowed candidates beyond sixteen rejected addresses.

The final logs use the ignored `target/routing-fix-*` prefix. The commands and
directories are the exact ones in the initial table, with the following results:

| Checks rerun after corrections | Result |
| --- | --- |
| Windows Rust helper Clippy | Passed |
| Windows Rust helper workspace tests | Passed: 1,692 tests; the same 8 explicitly conditional tests ignored |
| Windows Rust helper release build | Passed; compile only |
| Flutter locked package resolution and Dart format | Passed; 230 files unchanged by the format check |
| Full Flutter analysis | Failed on the same 3 pre-existing ignored-script warnings |
| Supplemental `flutter analyze --no-pub lib test` | Passed |
| `flutter test --no-pub` | Passed: 1,005 tests, including exact Windows goldens; no new baseline changes needed |
| Plugin junction preparation and Windows Flutter release compile | Passed |
| Android arm64 Rust Clippy and debug configuration-only build | Passed |
| Kotlin ktlint check | Passed |
| Kotlin unit tests and Android lint | Passed |
| Buf lint/format, Ruff lint/format and both PSScriptAnalyzer checks | Passed |
| Aggregate source checker | Failed at full Flutter analysis on the same 3 pre-existing warnings; preceding Rust and Dart checks passed |
| Repository policy and whitespace checks | Passed |

One added raw-UDP test initially raced its own forced cancellation against
normal association closure. It now closes the client control stream, waits for
the association to finish, then cancels the fixture. The final workspace test
run above passed with that ordered cleanup. All isolated checks listed earlier
remain `not_run`; no packaging, installation, signing or publication was added.
