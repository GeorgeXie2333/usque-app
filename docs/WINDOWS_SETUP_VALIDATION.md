# Windows setup implementation validation, 2026-10-04

This record covers the native installer, persistent uninstall window and
current-user completion options implemented on top of
`57e0e9c33d6b9908ee49d93860d136753a9d1fc1` in an uncommitted working tree. It does not identify a complete,
reproducible release candidate. These workstation results are not installation,
network-restoration or publication evidence.

## Implemented behavior

- The EXE uses a native C++ window for installation, upgrade, completion,
  maintenance and registration cleanup. WiX 5.0.2 Burn and the existing MSI
  retain ownership of the installation transaction and upgrade order.
- Rust/Win32 keeps the uninstall window open while an MSI worker reports
  actions, progress, cancellation availability, file-use questions and results.
  Personal-data deletion is off by default. Rollback, deletion and registration
  cleanup cannot be interrupted through the window.
- Completion applies current-user desktop and login-startup choices through
  shared native shell operations before opening the app. Each result is separate;
  retries only repeat failed operations. New installations default to no desktop
  link, no login startup and opening the application.
  When restart is required, the installer can still apply the first two choices
  without executing an older GUI that may be awaiting replacement.
- Both interfaces use a shared 21-language catalog, native accessible controls,
  scrollable content and fixed footer buttons. Separate preview-only executables
  exercise simulated states without entering the installation/removal paths.

## Completed workstation checks

The host used Rust 1.97.1, Flutter 3.44.7 at
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`, Python 3.12.14, WiX 5.0.2,
Ruff 0.16.0, PSScriptAnalyzer 1.25.0, Buf 1.72.0 and actionlint 1.7.12.
`python`, `flutter` and `dart` below denote those verified executables.
PowerShell commands ran from the repository root unless noted.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed, workspace and all targets with locked dependencies |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | Passed, workspace and all targets with locked dependencies |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2` | Passed, compile-only release binaries |
| `pwsh -NoProfile -File tool/check_source.ps1` | Passed after initializing the Windows helper environment; Rust, Dart, Flutter analysis, Kotlin style, Python, PowerShell and protobuf checks |
| `python -m unittest discover -s tool -p "test_*.py" -v` | 113 tests, passed; 5 Linux-only cases skipped on Windows |
| `ruff check tool` and `ruff format --check tool` | Passed |
| `& ./apps/usque_gui/build/windows/x64/tests/Release/usque_zero_trust_test.exe` | Passed, `windows_runner_test: ok`; inert registry, pipe and shortcut fixtures |
| `python tool/check_repository_policy.py` | Passed |
| `actionlint -no-color` and `git diff --check` | Passed |

From `apps/usque_gui`, the complete required sequence passed:

```powershell
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub
& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .
flutter build windows --release --no-pub --split-debug-info=build/symbols/windows
```

The Flutter suite ran 899 tests, including the Windows golden tests. An existing
ignored screenshot-generation script under the Flutter build directory caused
three analyzer warnings on the first attempt. Only that generated file was
temporarily moved outside the Flutter project for analysis, then restored with
an identical hash; no source exclusion or warning suppression was added. The
first Flutter compile attempt could not find Git in its child process. Repeating
the same build with an explicit process PATH containing the pinned SDK and Git
succeeded. Neither environment failure was reported as a pass.

After the full workspace checks, focused uninstall checks were repeated with
`-Package usque-uninstall` for the helper's `test`, `clippy` and default release
actions. The suite passed 29 tests, including cancellation permissions,
irreversible deletion, partial results, restart confirmation and a preview
callback that must never call the operating system. The preview executable was
also compiled using the documented locked, explicit-target command in
[Contributing](../CONTRIBUTING.md#windows-rust-and-msi-authoring).

The final x64 bootstrapper build used:

```powershell
& ./tool/build_windows_bootstrapper.ps1 -Variant x64-v2 -OutputDirectory target/bootstrapper-delivery-v2 -Test -Preview -PythonPath 'C:\Users\George\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe'
```

Native state, per-item result protocol, short-lived child-output capture,
cancellation, failure-exit and sanitized-detail tests passed. Separate shipping
and preview executables compiled with `/W4 /WX /MT`. The shared catalog contains
108 keys for each of the 21 languages; the focused catalog/dependency suite also
passed after the final translations were added.

The final shared shell API additionally rejects removal commands and invalid
installer targets, checks the original user, and balances its calling thread's
COM initialization. Those boundaries have inert native tests. The complete
Flutter/Windows sequence above was repeated after introducing this shared
entry point.

Native preview checks sampled Chinese, English and Arabic, light/dark themes,
simulated high contrast and 200% layout, keyboard focus, file-use prompts,
partial completion, rollback, cancellation and restart results. They caught and
corrected dark checkbox contrast, theme repainting, disabled-control focus,
missing progress/detail accessibility names, and unsafe reuse of restart
confirmation buttons. They did not execute installation, deletion or restart.

Both complete inert installer matrices passed:

```powershell
& ./tool/test_windows_installer_authoring.ps1 -Variant x64-v2 -BootstrapperPath target/bootstrapper-final/usque-setup.exe -OutputDirectory target/installer-authoring-x64-20261004-r4
& ./tool/test_windows_installer_authoring.ps1 -Variant arm64 -BootstrapperPath target/installer-authoring-ba-fixture-arm64/usque-setup.exe -OutputDirectory target/installer-authoring-arm64-20261004
```

Each run validated all 21 MSI languages separately with ICE, all 20 language
transforms, bundle extraction, Burn detach/reattach, package replacement
rejection cases, Japanese ICE03 rejection, quiet launchers in PowerShell 7 and
5.1, and argument handling in Legacy, Standard and Windows modes. Signing tests
used inert doubles or temporary Burn test identities, with cleanup, never
official signing material or certificate trust stores. No fixture bundle or MSI
was executed. Local logs and `authoring-results.json` remain in the ignored
output directories.

The ARM64 authoring run used an explicitly inert matching-architecture PE
fixture, not a compiled Usque bootstrapper. That result proves the packaging
structure only. Initial authoring failures in progress-text attributes and
fixture extraction-directory preparation were corrected before these full runs.

## Cleanup, trust and privacy review

The setup process retains the initiating user's unprivileged identity; the
installed runner rejects elevated, service and impersonated setup-option calls.
Verified executables stay locked against modification through process creation.
MSI still owns administrator approval, impersonated user-data cleanup and
privileged network restoration. A failed restoration prevents removal from
being treated as successful. Same-version replacements retain the existing
upgrade behavior and never invoke MSI repair.

Unknown shortcuts and login-startup entries are not overwritten or removed.
Deletion targets only the initiating Windows user's local data and is never
replayed automatically after failure. MSI removal and Burn registration cleanup
have separate outcomes; retrying registration cleanup cannot delete data again.
Displayed details contain selected stage/result information rather than raw MSI
records. Saving details is manual and local; no automatic upload was added.

## Checks not run

| Check | Status and limit |
| --- | --- |
| Actual native ARM64 C++/Rust/Flutter compilation and execution on this workstation | `not_run`: ARM64 Visual Studio C++ tools are absent; hosted architecture gates remain configured |
| Real installation, update, replacement, removal, cleanup and reboot | `not_run`: require the prescribed snapshot VM and independent management channel |
| User A with administrator B approving UAC | `not_run`: source and inert tests were reviewed, but they do not establish cross-account Windows behavior |
| Connected uninstall, failed network recovery, Wintun lifecycle, crash recovery and external leak observation | `not_run`: require the appropriate protected environments |
| Full Windows 10/11 × architecture × language × scaling/accessibility matrix | `not_run`: preview sampling and semantic control inspection are not the complete matrix or a screen-reader end-to-end test |
| Official signing, release upload or publication | `not_run`: not requested |

Protected-runner validation remains supplemental and does not become a
publication prerequisite. An unavailable check is not a pass. See
[Contributing](../CONTRIBUTING.md) for authoritative safety and acceptance rules.

## Visual refinement after independent screenshot review, 2026-10-04

This follow-up retains the uncommitted-source limitation above. It addresses
the native windows' control colors, form alignment, option spacing, auxiliary
actions, partial-completion notice and keyboard-focus bounds. The two windows
use approximately 680 by 480 logical pixels of client content, with native
window framing and work-area constraints. Larger text and longer translations
use scrolling while the footer remains visible.

Ordinary themes use the existing canvas and system font, with the Usque orange
accent (`#F48120`). Secondary text is 13 logical pixels, compared with the
15-pixel body text. Its light color `#626F81` has a calculated 4.93:1 contrast
against the existing `#FAFBFD` canvas; the existing dark secondary color has
6.87:1 contrast against `#181D25`. High-contrast mode retains system colors.
Irreversible-deletion warnings retain normal body size and foreground emphasis.

The native language selector and progress control retain their accessibility
providers and keyboard behavior. The partial-completion notice uses actual
per-item results, rather than inferring failure from translated text. These
visual changes do not add privileged operations, diagnostics, uploads, or
personal-data targets. The existing cancellation, consent, retry and restart
boundaries still apply.

The following checks were rerun successfully for this follow-up:

```powershell
cargo fmt --all --check
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2
cargo build --locked --release --target x86_64-pc-windows-msvc -p usque-uninstall --features preview --bin usque-uninstall-preview
& ./tool/build_windows_bootstrapper.ps1 -Variant x64-v2 -OutputDirectory target/bootstrapper-visual-refinement -Test -Preview -PythonPath 'C:\Users\George\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe'
pwsh -NoProfile -File tool/check_source.ps1
python -m unittest discover -s tool -p "test_windows_setup_*.py" -v
python tool/check_repository_policy.py
git diff --check
```

The Rust helper initialized the native environment for Cargo and aggregate
checks. The full workspace tests passed, including 29 uninstall tests. The
bootstrapper's production/preview compilation and existing state tests passed;
all 21 languages validated. The Python setup suite passed eight tests. The
aggregate check passed with the same generated screenshot-script isolation and
hash-verified restoration described above. Package authoring, runner operations,
shared text and release contracts were unchanged in this visual follow-up;
earlier package-matrix results are recorded separately above.

An independent agent compared the new native screenshots with the initial
seven-image review. It confirmed the control, alignment, grouping, notice,
focus and uninstall-description improvements without finding overlaps or
truncation in those samples. Its remaining footer-proportion feedback led to
200-pixel preferred main-button widths and 20-pixel bottom spacing, retaining
measured multi-line heights and narrow-work-area stacking. Real installation,
restoration, ARM64 native execution and the full device/locale matrix remain
`not_run` under the same limits.

## Action hierarchy and visual review, 2026-10-06

This follow-up was checked against base commit
`ddedbd259b9d9bf4cb470217a71749a14feaaa7c` plus uncommitted source changes.
It is workstation validation, not evidence for an immutable release candidate.
The earlier dated results remain historical; the results below cover this
follow-up.

The already-installed page now makes **Open Usque** the primary action and
**Uninstall** a secondary action. The completion page retains **Finish and
open**. Action routing and native state tests cover the changed maintenance
order. Ordinary themes use warm `#F5F4F1` and near-black `#0E0E10` canvases with
neutral borders and the existing orange accent. License text has explicit
foreground/background colors, and native rich-text and scrollbar themes use
the public Windows theme API with a fallback and reentrancy guards. High
contrast retains system colors.

Invalid installation folders now explain the failed constraint inline.
Cancelled, failed, partially deleted and restart-required outcomes have
distinct descriptions. Failure guidance uses selected numeric result codes;
raw MSI records, paths and ProductCodes are not retained or displayed. A failed
installation requiring restart asks the user to save work and restart, rather
than offering a retry that the current page cannot perform. Registration
cleanup guidance follows its current result while retaining the historical
numeric MSI code separately in details.

Uninstall warning text uses native STATIC rendering; an independent owner-drawn
decoration paints only the accent line. This removes duplicate text painting
and respects measured wrapping and RTL layout. The footer measures translated
button labels and stacks when the available work area is narrow. Details can
be explicitly hidden, and a close request during a non-cancellable stage
explains why the user must wait. Deletion remains opt-in; returning from a
partial deletion clears the selection. Registration cleanup retries cannot
repeat MSI removal or data deletion, and restart still requires a separate
confirmation. These changes add no privileged operations or personal-data
targets.

The following commands completed successfully. Cargo and aggregate checks ran
in the native environment initialized by the Windows Rust helper. Python was
the verified bundled executable at the explicit path below. Flutter resolved
from `android/local.properties` matched the CI pin, including full revision
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`; locked dependency resolution completed
without changes to the lockfiles.

```powershell
cargo fmt --all --check
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2
cargo build --locked --release --target x86_64-pc-windows-msvc -p usque-uninstall --features preview --bin usque-uninstall-preview
& ./tool/build_windows_bootstrapper.ps1 -Variant x64-v2 -OutputDirectory target/bootstrapper-ux-final -PythonPath 'C:\Users\George\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe' -Test -Preview
flutter --version --machine
Push-Location apps/usque_gui
flutter pub get --enforce-lockfile
Pop-Location
pwsh -NoProfile -File tool/check_source.ps1
python -m ruff check tool
python -m ruff format --check tool
python -m unittest discover -s tool -p 'test_*.py' -v
python -m unittest discover -s tool -p 'test_windows_setup_*.py' -v
python tool/check_repository_policy.py
git diff --check
```

The full Rust suite passed 1,648 tests in 23 suites, with eight existing ignored
tests, including 47 uninstall tests. The shipping and preview bootstrapper
compiled with `/W4 /WX`; native state tests passed. All 21 catalog languages
contain the same 126 keys, including localized recovery guidance and path
constraints. The complete Python suite passed 108 tests and skipped five
Linux-only tests; the focused setup suite passed all eight tests. The final
aggregate check passed Rust, Dart/Flutter, Kotlin, Python, PowerShell and
protobuf checks.

The ignored generated README screenshot script had three pre-existing analyzer
warnings. Only that file was temporarily moved within the workspace for the
aggregate run and restored in a `finally` block, without source changes or lint
exclusions. Its restored SHA-256 was
`4812E7580267FE59EE80CF4C2EB410F3392B651AB0AF95BBA7613A66D34ABE19`.

Both complete inert authoring matrices passed:

```powershell
& ./tool/test_windows_installer_authoring.ps1 -Variant x64-v2 -BootstrapperPath target/bootstrapper-ux-final/usque-setup.exe -OutputDirectory target/installer-authoring-ux-x64-final-20261006
& ./tool/test_windows_installer_authoring.ps1 -Variant arm64 -BootstrapperPath target/installer-authoring-ba-fixture-arm64/usque-setup.exe -OutputDirectory target/installer-authoring-ux-arm64-20261006
```

Each matrix completed all 13 checks, including 21 ICE cultures, 20 transforms,
PowerShell 7/5.1 quiet-launcher doubles, all three argument-transport modes,
replacement rejection cases, malformed Japanese ICE03 rejection and inert Burn
signing/detach/reattach tests. The x64 run used the final compiled bootstrapper.
The ARM64 run used an explicitly inert matching-architecture PE fixture and
proves authoring structure only. Temporary non-exportable test identities were
removed with their keys; no trust store or official signing material was used.
Neither matrix executed an MSI or a bundle.

An ARM64 native bootstrapper build was attempted with
`& ./tool/build_windows_bootstrapper.ps1 -Variant arm64 -OutputDirectory target/bootstrapper-arm64 -Test -Preview -PythonPath 'C:\Users\George\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe'`.
The workstation lacks the required Visual Studio ARM64 tools, so native ARM64
compilation and tests remain `not_run`.

Final visual review used 39 raw screenshots from the independent preview
executables. Samples cover Chinese, English and Arabic RTL, light/dark themes,
simulated high contrast and 200% layout, license text, invalid folders and
scrolling, maintenance, completion, partial options, file-use prompts,
cancellation, rollback, failures/details, deletion warnings, partial deletion
and restart confirmation. Independent review found no remaining visible text
overlap or button truncation in those samples. It confirmed the primary Open
action, corrected failed/restart guidance and cleared deletion choice after
returning to confirmation. Local screenshots and logs remain in ignored build
directories; they are not published release evidence. All created preview
windows were closed after inspection.

Real installation, upgrade, removal, network restoration, cross-account UAC
and reboot remain `not_run`, as do native ARM64 execution and the complete
Windows/version/language/DPI/text-size/screen-reader matrix. Preview keyboard
and semantic-control inspection do not establish a Narrator end-to-end pass.
No official packaging, signing, upload or publication was requested or run.
