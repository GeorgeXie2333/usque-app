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
   Known items: uninstall wording shown during upgrades, the install-scope
   subtitle shown while cleaning up, the empty warning row that still takes
   space on the uninstall progress page, an invisible high-contrast track in
   setup, and a seamless hand-off from the setup window to the uninstall
   window.
4. **Structure (P4):** split `main.cpp` and `windows/ui.rs` into engine
   callbacks, state, layout and drawing. This may land with batch 2 or 3.
5. **Records (P5):** validation record and any user-facing documentation
   changes in [Installation and removal](INSTALLATION.md).

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

## Validation boundary

Workstation checks cover the pure models, compile-only builds, inert preview
replays and the inert authoring matrix. Real installation, upgrade, uninstall,
rollback and registration-cleanup progress requires `usque-snapshot-vm` and is
`not_run` until such a run exists. The exported uninstall counters allow a
later isolated run to show whether MSI restarted or enlarged its progress.
