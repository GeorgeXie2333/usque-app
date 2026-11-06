# Windows setup and uninstall UI refactor

This plan was agreed on 2026-11-06. It covers the presentation of the native
setup window (`packaging/windows/bootstrapper`, WiX 5.0.2 Burn) and the
persistent uninstall window (`crates/usque-uninstall`). Burn, the MSI and the
uninstall helper keep ownership of the transaction, recovery, rollback, data
deletion and registration cleanup; this work changes what the windows show.
Each batch is an independently reviewable change. The starting source is
`bdbafdd6`.

## Design decisions

- Keep the C++ Burn bootstrapper application and the Rust uninstall window.
  The uninstall window must run from the installed folder without the Burn
  cache and owns optional data deletion and registration cleanup. Share
  presentation rules, design tokens and the 21-language catalog instead of
  merging the programs.
- Align colors, type, spacing and controls with the Flutter application.
  Generate both native palettes from one checked-in token source.
- Replace the native progress control with a custom-drawn bar that supports
  determinate, waiting, rollback and error states. High contrast keeps system
  colors. Value and state stay available to screen readers.
- Fix progress behavior first, as its own change, before the visual refactor.

## Progress presentation rules

Raw engine numbers never reach the control. A pure model maps them onto one
bar in permille:

| Window | Waiting | Phase ranges | Terminal |
| --- | --- | --- | --- |
| Setup | Detection and planning | Cache 0–150, execution 150–950 | Success fills to 1000 |
| Uninstall | Verification, elevation, costing | Script generation 0–100, execution 100–900 | Registration cleanup holds at 900 or more |

- The presented value never decreases within an attempt. A new MSI session,
  a progress addition, or a nested session restarting its count holds the bar
  until the engine catches up.
- The bar leaves the waiting state once, at the first measurable report, and
  never returns to an indeterminate animation.
- Rollback freezes the bar at its last forward value. Status text explains the
  rollback; the bar does not run backward.
- Windows receive progress-control messages only when the presented state
  changes. Labels that can change without a page change reserve the height of
  their tallest variant, so status and cancellation messages cannot move the bar.

The two adapters differ because their inputs differ. The uninstall window
decodes MSI progress records itself; the setup window receives Burn's cache
and execute percentages, which Burn already derives from MSI. Each model has
its own deterministic tests, including a randomized forward-only sweep.

## Findings that motivated the rules

- Uninstall: every MSI master reset restarted the bar at zero; execution-time
  progress additions reduced the percentage; raw fractions switched the
  control between marquee and fill; the registration stage forced a marquee
  after a nearly full bar. The 100 ms timer also rewrote the marquee style,
  range and animation on every tick.
- Setup: only Burn's `OnProgress` was shown. In WiX 5.0.2 it advances once per
  completed cache or execute action and steps back during rollback, so the bar
  stalled and jumped. Every MSI action and cancel-permission record rebuilt
  the whole page (hide, show, relayout, full repaint). The subtitle switched
  between texts of different height, which moved the bar in some languages.

## Implementation batches

1. **Progress behavior (P0/P1):** presentation models and tests, change-only
   control updates, reserved label heights, a `--preview=replay` uninstall
   scene and an upgrade-shaped setup preview that reproduce the former
   reversals, and numeric MSI progress counters in saved uninstall details.
2. **Visual system (P2):** shared token source and generator, custom-drawn
   progress bar with an activity indication while values are held, shared
   button/field/notice drawing rules. Retain accessibility, high contrast,
   RTL and 200% layout.
3. **Flows and wording (P3):** inventory every page and state, review
   wireframes before implementation, then update pages and all 21 languages.
   See [Batch 3 flows and wording](#batch-3-flows-and-wording).
4. **Structure (P4):** keep pure logic out of window code and split the
   uninstall window module. `windows/ui` is now `mod.rs` (state, rendering,
   window procedure and tests), `layout.rs` (footer, measured text, field
   frames and placement) and `draw.rs` (owner-drawn and subclassed painting).
   The setup bootstrapper stays one translation unit: its `Application` class
   implements Burn's callback interface and owns the window, so a file split
   would only move code behind a header. Its pure logic already lives in
   `state.h`, `paint.h` and the generated `setup_tokens.h`, all covered by
   `state_test.cpp`.
5. **Records (P5):** each batch records its validation below, and
   [Installation and removal](INSTALLATION.md) describes the changed setup and
   uninstall behavior.

All five batches were implemented on 2026-11-06.

## Batch 2 visual system

- `packaging/windows/setup/tokens.json` holds 13 Flutter colours, each with
  the Dart token it mirrors, seven derived colours with their formulas and the
  shared metrics. `tool/generate_windows_setup_tokens.py` writes the
  checked-in `setup_tokens.h` and `tokens_generated.rs`; its test rejects stale
  projections and any value that differs from `usque_theme.dart`.
- `crates/usque-uninstall/src/paint.rs` and
  `packaging/windows/bootstrapper/paint.h` implement the same pure,
  anti-aliased rasterizer, bar renderer, bar motion and button roles, each
  with its own tests. Text stays native GDI.
- Progress: a 6-pixel pill with a hairline track and primary fill. The waiting
  segment travels over `UsqueMotion.scan`; values ease over
  `UsqueMotion.base`; after 1.2 seconds without progress a soft highlight
  moves across the fill; rollback uses the caution tone. With client-area
  animation off, values jump and nothing moves. The native control still owns
  range, value and marquee state for accessibility; only its painting and
  non-client edge are replaced. System high contrast keeps the native control.
  Mirrored (RTL) device contexts receive mirrored canvases.
- Buttons follow Flutter's filled, outlined and text roles (12-pixel radius,
  10 for text actions), Flutter's disabled opacities and a visible focus
  ring. Checkboxes use a 5-pixel radius and a 1.6-pixel outline in the muted
  tone, chosen over Flutter's hairline for 3:1 non-text contrast. Setup's
  option-result notice follows `WarningBanner`; the uninstall warning accent
  is caution for the deletion choice and danger for possible data loss.
- Painting only: no transaction, cancellation, deletion, registration or
  exit-code behavior changes, and both previews remain inert.

## Batch 3 flows and wording

Decisions confirmed on 2026-11-06: result pages lead with a status mark,
deletion uses the danger tone, setup names its real steps, the uninstall
window opens where the setup window was, and the failure page keeps **Back**.

- Setup's working page shows `working_note` ("keep this window open") instead
  of the install scope the user already accepted, and its step line names the
  MSI phase: preparing, closing Usque, updating program files, setting up the
  background service, removing the previous version, restoring network
  settings or finishing. `SetupStepKey` maps actions per context, so a fresh
  installation never claims to close or remove an older version, and unlisted
  actions keep the current step instead of repeating the title.
- Result pages in both windows draw a decorative 36-pixel mark above the
  title: success, caution (restart, records not cleaned, options not applied,
  administrator approval missing), danger (failure, possible data loss) or
  neutral (cancelled). The title still states the result.
- The uninstall confirmation drops the redundant "data will be kept" line;
  choosing deletion shows a caution `WarningBanner` and turns the action into a
  danger-toned **Uninstall and delete data**. Possible data loss after a
  failure uses the danger banner.
- The uninstall page hides empty rows, keeps the progress note during
  registration cleanup (the step line names it), hides the step line during a
  files-in-use question and frames the application list. Setup hides the
  version on the records-cleanup page and draws its leading footer action as
  a text action.
- The language list, installation folder, license and details panes are
  rounded filled fields with an accent border on focus. Native controls keep
  input, scrolling and accessibility inside the painted frame.
- Setup passes `--window-origin=X,Y` to the uninstall helper. The helper
  forwards it through its temporary copy, accepts it only for visible windows,
  bounds both coordinates and clamps the window into that monitor's work area.
  Settings and Programs and Features launches remain centered.
- Five strings were added in all 21 languages: `working_note`, `step_files`,
  `step_services`, `step_replacing` and `step_finishing`.
- Safety: presentation and launch placement only. Transaction, cancellation,
  deletion, registration and exit-code behavior are unchanged; the position
  argument cannot select a product, mode or data action.

## Privacy and safety analysis for batch 1

- The new uninstall detail line contains only counts of MSI progress
  sessions, additions and held reversals. It has no record text, product
  code, path, property value or identifier.
- Cancellation, rollback, data deletion, registration cleanup and exit codes
  are unchanged. Setup cache callbacks call the existing Burn base behavior;
  only presentation events are added and throttled to value changes.
- Both previews remain inert and cannot reach Burn, MSI or deletion paths.

## Batch 1 validation, 2026-11-06

Uncommitted working tree on `bdbafdd6`. Linux commands ran in WSL; Windows
commands ran from an exact temporary copy of the tracked tree, with a
temporary-only Android SDK locator so the helper could select CMake and
libclang.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` (Linux) | Passed |
| `cargo test --workspace --all-targets --locked` (Linux) | Passed |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | Passed |
| Helper-initialized `cargo clippy` and release build of `usque-uninstall` with `--features preview` | Passed |
| `& ./tool/build_windows_bootstrapper.ps1 -Variant x64-v2 -OutputDirectory <absolute> -Test -Preview` | Passed, `BOOTSTRAPPER_STATE_TESTS=PASS` |
| `& ./tool/test_windows_installer_authoring.ps1 -Variant x64-v2 ...` | Passed; bootstrapper and installer execution `not_run` by design |
| `python tool/check_repository_policy.py`, `git diff --check` | Passed |

Inert preview sampling read the native progress control every 30 ms during the
uninstall replay (zh-CN light, en-US dark with a refused cancellation,
simulated high contrast) and the setup preview (zh-CN, en-US, simulated high
contrast at 200%). Every run showed one waiting-to-fill transition, no
decreasing value and an unchanged bar position.

Not run: the arm64 bootstrapper compile (no ARM64 C++ tools on the host),
Windows PowerShell 5.1 checks (no scripts changed), and every real install,
upgrade, uninstall, rollback and registration-cleanup run, which require
`usque-snapshot-vm`.

## Batch 2 validation, 2026-11-06

Uncommitted working tree on `e247ecf0`, using the same Linux and temporary
Windows copy arrangement as batch 1.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` (Linux) | Passed |
| `cargo test --workspace --all-targets --locked` (Linux) | Passed |
| `ruff check tool`, `ruff format --check tool` | Passed |
| `python -m unittest discover -s tool -p "test_*.py"` (Linux) | 353 tests passed, 109 Windows-only skips |
| `python tool/generate_windows_setup_tokens.py --check` | Passed |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | Passed |
| Helper-initialized `cargo clippy` and release build of `usque-uninstall` with `--features preview` | Passed |
| `& ./tool/build_windows_bootstrapper.ps1 -Variant x64-v2 -OutputDirectory <absolute> -Test -Preview` | Passed, `BOOTSTRAPPER_STATE_TESTS=PASS` |
| `& ./tool/test_windows_installer_authoring.ps1 -Variant x64-v2 ...` | Passed; bootstrapper and installer execution `not_run` by design |
| `python tool/check_repository_policy.py`, `git diff --check` | Passed |

Inert preview review covered both windows in zh-CN light and dark, en-US,
ar-SA (RTL fill direction) and the simulated high-contrast palette. Repeated
30 ms sampling still showed one waiting-to-fill transition, no decreasing
value and an unchanged bar position. Screenshot captures of right-to-left
windows are mirrored by `PrintWindow`; the review accounted for that.

Not run: the arm64 bootstrapper compile, system high-contrast themes on a real
session, Windows PowerShell 5.1 checks (no scripts changed) and every real
install, upgrade or uninstall in `usque-snapshot-vm`.

## Batch 3 validation, 2026-11-06

Uncommitted working tree on `86f22806`, with the same arrangement and the same
command set as batch 2: Linux `cargo fmt`, workspace Clippy and tests, Ruff,
the Python suite (353 tests, 109 Windows-only skips), the token freshness
check, the Windows helper's workspace Clippy and tests, the preview Clippy and
release build, `build_windows_bootstrapper.ps1 -Test -Preview`
(`BOOTSTRAPPER_STATE_TESTS=PASS`), the x64-v2 inert authoring matrix, the
repository policy check and `git diff --check`. All passed.

Inert previews were captured for every uninstall scene and setup state, the
deletion choice, expanded details, the folder field, the license page and the
working page, in light, dark, ar-SA and simulated high contrast. A burst of 33
captures on a settled page showed no repaint flicker. Captures taken while a
preview window was still painting its first frame sometimes showed an empty
button or title; a control dump and repeated captures confirmed those controls
were present and painted.

Not run: the arm64 bootstrapper compile, a real system high-contrast session,
the live setup-to-uninstall hand-off (it requires an installed product) and
every real install, upgrade or uninstall in `usque-snapshot-vm`.

## Validation boundary

Workstation checks cover the pure models, compile-only builds, inert preview
replays and the inert authoring matrix. Real installation, upgrade, uninstall,
rollback and registration-cleanup progress requires `usque-snapshot-vm` and is
`not_run` until such a run exists. The exported uninstall counters allow a
later isolated run to show whether MSI restarted or enlarged its progress.
