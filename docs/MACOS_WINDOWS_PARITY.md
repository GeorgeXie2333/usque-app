# macOS comparison with Windows

Target: macOS 13 or later, separate native arm64 and x86_64 packages. This work
continues from clean `dev` at `948de61c`, with the earlier experimental backup
retained for review. It is not yet a **macOS full-feature version**.

`main` was restored with history-preserving revert `0759ff02`; its tracked tree
matches `1d44597d`. At the owner's request, the preserved macOS development
commits were fast-forwarded onto `dev`, and further development stays there. Do not
merge this work into `main` before the owner requests it again.

The owner is using this workstation. Current validation is background-only:
no application/browser launches, WindowServer smoke tests, file dialogs,
notifications or system authorization prompts. Compile-only checks and synthetic
contract fixtures continue without changing host credentials or network settings.

## Feature comparison

“Implemented” describes code, “isolated” describes fake/local fixtures or build
checks, and “device pending” requires actual macOS permissions, providers or
hardware. None of the isolated results establishes physical-egress safety.

| Windows capability | macOS implementation | Validation and remaining difference |
| --- | --- | --- |
| MASQUE proxy, HTTP/SOCKS TCP/UDP | Existing shared runtime, with ordinary-user Engine as proxy-only owner | Shared Rust regression; no new live-account/network acceptance |
| Proxy-mode network changes | Read-only route, interface and selected DNS snapshots advance the network generation when source IPs stay the same | Constructed Core Foundation fixtures; actual sleep/network-switch acceptance remains pending |
| Full-system VPN | Packet Tunnel is the sole runtime owner; Engine proxy sockets enter the same system data plane | Provider adapters and loopback contracts; production capability remains false |
| Country/custom rules, DIRECT/REJECT/PROXY, advertising rules | Shared Rust immutable policy snapshots; separate flow, DNS and filter providers | Rule/epoch contracts; same-IP domain split and real `enforceRoutes` egress pending |
| IPv4/IPv6 and encrypted DNS | Shared runtime and separate DNS provider; encrypted-upstream errors do not request plaintext fallback | IPv6 loopback and policy contracts; physical/tunnel DNS, leakage and LAN device acceptance pending |
| Kill Switch and reconnect | Independent protection before tunnel admission, credential-less restart blocked; Engine loss is not Stop | Isolated lifecycle/fault contracts; actual provider crash, sleep and network-change protection pending |
| Chain exits and account managers | macOS wires the shared HTTP, SOCKS, OpenVPN, WireGuard, WARP WireGuard, VPN Gate and Proton handlers; numeric WARP endpoint overrides survive the dedicated secret handoff | Engine preparation/DTO tests across all families; live service acceptance pending; existing experimental flags retained |
| Account bootstrap protection | Existing connected WARP runtime can be reused; VPN Gate may create a listener-free base WARP runtime while retaining saved chain settings | Directory requests stay inside WARP; no direct physical-network catalogue fallback; active/retained VPN and pending recovery still block bootstrap |
| Credentials and configuration | User Engine owns Keychain and encrypted chain storage; bounded zeroizing cache reuses unlocked records across connection changes; dedicated secret transfer and durable refresh acknowledgement | Injected Keychain and in-memory test vault; no host credentials read or replaced by tests |
| Connection scenes | Shared save/select/preview/apply workflow, account and chain binding, stale-session fencing and uncertain-write settlement enabled on macOS | Backend and macOS widget regressions; scene activation does not bypass output capabilities or pending proxy recovery |
| System proxy | Per-service SystemConfiguration transaction, Engine-lifetime network authorization reuse, silent rights preflight on cleanup, encrypted ownership journal, compare-and-restore; pending recovery blocks new sessions and key deletion | Fake preferences/authorization/failure injection; real authorization cancellation and crash recovery pending; newly created services require reapply |
| Continuous desktop events | Authenticated per-user Unix control/events sockets; bounded, fragmented/coalesced frame decoder | Engine and Dart tests; hidden-window event loop is implemented |
| Tray/background controls | Native menu bar status, connect/disconnect, VPN/proxy actions, hidden-window updates, explicit disconnect-and-quit | Native contract/type checks; real tray interaction pending |
| Output availability | Home, Proxy, tray and shortcuts honor Engine VPN/proxy capabilities; unavailable saved outputs can still be turned off; Settings explains unavailable VPN protection | Headless capability/refresh contracts; unsigned VPN stays disabled |
| Window lifecycle | Close-to-menu-bar default, Dock reopening, single-instance activation, 1040×760 default content with first-launch centering and remembered/clamped geometry | Native geometry/type checks and headless small-screen layouts; actual window interaction pending |
| Settings and shortcuts | Native Settings menu, Cmd+,; Cmd+1–4 and Cmd+S; existing unsaved-change guards | Dart widget and native contracts |
| Traditional Chinese | Existing Hong Kong/Taiwan catalogs now also drive native menus and app-owned file-dialog copy; framework controls use the same resolver and hidden windows follow system language changes | Headless language-switch/persistence regressions and native contracts; actual window display remains untested |
| Login startup and notifications | SMAppService default off, authorization/cancel retry, native notifications | Build/contracts; real permission dialogs and login reboot pending |
| Configuration import/export and diagnostics | Bounded batch file picker, JSON credential/config saves, diagnostics saved as ZIP | Dart regression/native contracts; real file-dialog acceptance pending |
| Zero Trust browser login | ASWebAuthenticationSession callback, strict organization/path/token validation, attempt fencing and cancellation | Isolated URL/lifecycle checks; actual identity-provider login pending |
| Proton guest registration and embedded browser | Removed on macOS; existing Proton account login remains available. Cloudflare Zero Trust continues to use the system authentication session | macOS source and packaging no longer include the CEF component; older published DMGs still contain it |
| Architecture-specific installation | Native arm64 and x86_64 DMG builders with drag-to-Applications layout, complete nested Mach-O/minimum-OS checks | Development image verification; Intel execution and macOS 13 hardware pending |
| Update selection and recovery | Maintenance foundation verifies version, native hardware architecture, name, size, hash and signing; same-volume atomic swap, retained backup and durable, candidate-bound health receipts | Temporary injected-verifier tests including Intel-to-arm64 migration and confirmation retries across restarts; live health authentication is not integrated and production installation remains disabled |
| Extension update and uninstall | Application transaction leaves extensions/user data separate; manual app removal preserves user data | Extension approval/restart orchestration and managed uninstall remain pending |
| Signed production distribution | Development packages are explicitly ad-hoc and not notarized | Developer ID, notarization, Gatekeeper distribution and full two-architecture acceptance pending |

## Reliability follow-up (2026-11-05)

This follow-up starts from `ffc5865c8ca90ab8c694f0ce992d3b0183673fb3`.
It corrects four issues without changing the existing networking capabilities:

- Quit waits for the account/network edit queue before asking the Engine for
  its final cleanup acknowledgement. A delayed second account save can no
  longer run after that acknowledgement.
- Connected provider observations refresh the reported transport, endpoint
  family and address availability after a path change, without repeating
  protection admission or starting another session.
- Running windows refresh their minimum size when moved between displays.
  Display removal or resolution changes recover reachable geometry; full-screen
  recovery waits until the window exits full screen.
- The Proton panel keeps its polling timer only while a job is active.
  Idle and terminal states stop the timer; a later job starts it again.

The queue-order and connected-path regressions were reproduced before their
fixes. A full Rust run also exposed a paused-clock UDP fixture that could
auto-advance while waiting for real loopback I/O. Its bounded wait now keeps
the runtime runnable; exact expiry and lease-release assertions remain in place.
Verification uses fake engines, synthetic display rectangles and local fixtures.
Actual display interaction and real-service/network acceptance remain separate;
no replacement installer or release is published by these changes.

Final validation of this working-tree follow-up:

| Check | Result |
| --- | --- |
| `cargo test --workspace --all-targets --locked --offline` | 2,071 passed, 3 ignored, no failures |
| `flutter test --no-pub --exclude-tags golden` | 1,526 passed, 1 skipped, no failures |
| `pwsh -NoProfile -File tool/check_source.ps1` | Passed, including strict workspace Clippy, format, Flutter analysis, Kotlin, Ruff, PowerShell and protobuf |
| Python tooling suite | 326 cases, 21 skipped, no failures/errors |
| Native desktop, network and maintenance contracts | 8 desktop groups, network contracts and 13 maintenance groups passed |
| `flutter build macos --debug --no-pub` | Apple Silicon Debug build passed; existing Xcode CoreSimulator warnings remain |
| Tracked-source repository policy and `git diff --check` | Passed |

## macOS Proton guest registration removal (2026-11-05)

Current source removes the Proton guest registration entry point, native CEF
component and Swift browser launcher. Packaging builds without a browser and
rejects stale browser helpers or Chromium frameworks in reused GUI output.
Existing Proton accounts can still sign in; Cloudflare Zero Trust retains its
`ASWebAuthenticationSession` flow. Older published installers and the validation
records below predate this removal and must not be treated as current browser
support or a newly built package.

Account/password sign-in, TOTP and saved-account/server management remain
available. If Proton requires browser verification, the GUI cancels that task
and explains the limitation. Engine IPC rejects guest registration and browser
handoff before lifecycle locks, storage or network work. Windows, Linux and
Android retain their verification flows.

Validation of the removal, on top of `66327193956d001baf845f4167f44f42ce9952fb`:

- Engine library: 273 tests passed, including a new macOS IPC admission test;
  strict Engine Clippy and workspace Rust formatting passed.
- Proton GUI and localization: 109 tests passed; focused Flutter analysis clean.
- Package fixtures: 14 tests passed; other-platform native verification bridge:
  14 tests passed; retained macOS desktop contracts: 8 groups passed.
- Complete Python tooling suite: 305 passed, 21 skipped, no failures/errors.
- macOS Debug build passed; the output contains no Proton verification helper
  or Chromium framework. Existing Xcode CoreSimulator version warnings remain.
- Python Ruff, tracked-source repository policy and whitespace checks passed.

These checks did not launch the app/browser, access real accounts or modify host
network settings. No replacement installer was packaged or published.

## PR #147 follow-up (2026-11-05)

This follow-up addresses the [November 4 review](https://github.com/GeorgeXie2333/usque-app/pull/147#pullrequestreview-5765085399).
It does not establish Windows-equivalent system-wide networking. The existing
development installer predates this source; no replacement installer was built
or published.

| Requested behavior | Current source and acceptance status |
| --- | --- |
| System-wide capture and Kill Switch | Provider implementation remains a signed experiment with unresolved physical-egress acceptance. `vpn`, platform lockdown and TUN capability remain false. Proxy-only limitations remain visible. Signed activation, routing/DNS, protection/crash and external IPv4/IPv6 leak results are `not_run`. |
| Connection scenarios | macOS uses shared save/select/preview/apply, account/chain binding, stale-session and uncertain-write settlement. Activation waits for proxy recovery. Unavailable outputs remain unavailable. |
| Keyboard and Settings | Cmd+1–4, Cmd+S and Cmd+, retain the corresponding shared actions. Native Settings uses the same unsaved-change guard. Native connection/output commands are disabled during dialogs and unsaved/saving edits, and checked again when received. |
| Menu bar, background and quit | Home and menu bar share connection availability, retaining cancellation while connecting. macOS Disconnect now acknowledges completed cleanup. A disconnected cached snapshot cannot skip Engine cleanup; empty or non-completed replies cannot approve Quit. Pending Quit blocks new connection/output actions. Failure keeps the window and retry path. Native window/Dock/multi-display interaction remains `not_run`. |
| Keychain and proxy authorization | Existing bounded successful-read cache and process-local authorization remain unchanged. A failed startup rollback registers the encrypted journal before stopping its listener; final-activation failure retains its runtime and guard. Hot-update cleanup also recovers a journal without a live guard. Stop retries authorization and Clear All cannot delete recovery keys first. Actual permission dialogs, locked/managed Keychain behavior and host restoration remain `not_run`. |
| VPN Gate directory | Reuse a connected base WARP or automatically create a temporary WARP runtime without proxy listeners. Saved chain/output settings stay intact. Active or retained system-VPN ownership and pending proxy cleanup block a new temporary runtime. Cached preparation, listing, favorites and removal remain offline. Live directory/node service acceptance is `not_run`. |
| Chain exits and account login | Shared HTTP/SOCKS, OpenVPN, WireGuard, VPN Gate, existing Proton account login and Zero Trust entry points remain subject to their existing capability/prerequisite checks. Real authentication, traffic, DNS and system authentication acceptance are `not_run`. Proton guest registration and its bundled browser have been removed from macOS. |

These limitations require signing resources, an appropriate test environment or
interactive real-service/device acceptance. A protected-runner report is not a
publication prerequisite; unavailable checks must remain `not_run`, and no
capability flag substitutes for implementation or acceptance.

### Historical validation for the PR #147 follow-up

Final code: `df5bf9bdbc24df7f78d792e318ec70ec76638272`, following
`236faa2bef4f27f6f635bcaab7c9526997f75fcc`. Host: Apple Silicon macOS;
deployment target: macOS 13. Pinned Flutter 3.44.7 and repository toolchains.
The last follow-up changes Dart only; final format, analysis, full Flutter
regression and Debug build were repeated afterward. Rust sources were unchanged.

| Command/check | Result |
| --- | --- |
| `cargo test --workspace --all-targets --locked --offline` | 2,069 passed, 3 ignored, no failures |
| `pwsh -NoProfile -File tool/check_source.ps1` | Passed, including strict workspace Clippy, format, Flutter analysis, Kotlin, Ruff, PowerShell and protobuf |
| Final Dart format and `flutter analyze --no-pub` | Passed; no analysis issues |
| `flutter test --no-pub --exclude-tags golden` | 1,518 passed, 1 skipped, no failures in the final complete run |
| `python3 -m unittest discover -s tool -p 'test_*.py'` | 354 cases, 21 skipped, no failures/errors |
| `bash tool/test_macos_desktop_contracts.sh` | 10 groups passed |
| `bash tool/test_macos_native_contracts.sh` | Passed; no provider installed or activated |
| `bash tool/build_macos_network_prototype.sh --typecheck --arch arm64` and `--arch x86_64` | Four providers and control helper passed for both architectures |
| `cargo check -p usque-engine --locked --offline --target x86_64-apple-darwin` | Passed; no Intel hardware execution |
| `flutter build macos --debug --no-pub` | Passed after final Dart change; Xcode reported an unrelated out-of-date iOS simulator framework |
| Committed-source repository policy and `git diff --check` | Passed |

CEF was rebuilt with `python3 tool/build_macos_proton_verification.py --arch
arm64`. Its 22 harness tests passed. The browser-free guard command
`python3 tool/test_macos_proton_verification.py --app
build/macos-proton-verification/arm64/UsqueProtonVerification.app
--no-interactive --output
build/macos-proton-verification/arm64/review-no-interactive.json` passed six
rejection/handoff cases, with zero renderer smokes. These checks do not initialize
CEF or a native browser window. Full authentication-proxy failure/exit,
official challenge and Intel CEF execution remain `not_run`; the historical
full-smoke failure has not been established as fixed.

Tests use simulated credentials/authorization, temporary files and local test
channels. No real Keychain records, host proxy/DNS/routes, system VPN or external
account sessions were changed. Other-platform native builds, Windows golden
images, signed distribution, real permission dialogs and device/network
acceptance were not repeated. The earlier source-bound records below retain
their original results and package identities.

## Keychain access

The macOS vault retains successfully read credentials in zeroizing memory for
the Engine lifetime, shared by its clones. Connecting, disconnecting and
reconnecting can reuse those records without repeating the same native Keychain
read. The cache holds at most 256 records of at most 2,560 bytes each; eviction
and the last owner's exit drop and zeroize the values. Debug output omits both
credentials and account identifiers.

Explicit writes and deletes invalidate the cached record before native access,
including failures. Writes still reach Keychain and subsequent reads still verify
the stored value before caching it. Missing records, failed/cancelled reads and
invalid sizes are not cached. Native operations and cache changes run under the
same blocking-worker lock, including when an async caller is cancelled.

The first read of each item, a new Engine, eviction, updates or a locked Keychain
can still require macOS approval. This does not change Keychain ACLs, unlock the
login Keychain automatically or grant another application access. Already loaded
credentials remain usable for this Engine session, as with active transport
credentials; external Keychain edits are observed after cache invalidation or
Engine restart. Real Keychain password dialogs remain device-pending; tests use
only a synthetic injected store.

## System-proxy authorization

Enabling the system proxy can require the macOS administrator dialog. Engine
requests `system.services.systemconfiguration.network`, the right checked by
[Apple's SCHelper](https://github.com/apple-oss-distributions/configd/blob/main/SystemConfiguration.fproj/helper/SCHelper_server.c),
and retains that process-local authorization across disconnect, reconnect and
recovery. It no longer requests the unrelated broad `system.preferences` right
or discards its authorization after each connection. Passwords and authorization
external forms are never stored or passed to another Usque process.

Cleanup checks the network right without allowing interaction before opening an
authorized preferences session. If the check fails, the encrypted recovery journal
remains for an explicit retry. A fresh Engine's background recovery creates no
authorization reference. This does not bypass macOS policy: the first enable,
Engine restart, expired/revoked rights or managed-device policy can require new
approval. SCHelper performs its own checks at lock/commit/apply, so a concurrent
policy change after preflight remains subject to macOS behavior.

The regression fixtures cover one authorization across connect/disconnect/reconnect,
cancellation/retry, revoked-right preflight and shared context lifetime. They do not
display a real password dialog or change host network preferences. Actual dialog
counts, long-running sessions, cancellation and crash recovery remain device-pending.

## PR #147 follow-up (2026-11-01)

Explicit Disconnect/exit retries can renew an expired system-proxy authorization.
The first cleanup attempt, startup recovery and process detach remain
noninteractive; cancelling an explicit retry retains the same encrypted journal
and exact transaction for another retry. Disabling the system-proxy output from
its settings is also an explicit authorization path.

The GUI now supplies its parent PID to the macOS Engine. The Engine watches its
actual parent relationship, detaches local frontends after GUI exit and releases
the IPC lease after detach. A reused PID cannot keep an orphan alive. This does
not stop an independently protected Network Extension session.

The shared-runtime refusal fixture waits for its asynchronous socket callback
and verifies cancellation plus a non-connected state before teardown. Packaging
fixtures skip only unavailable symlink capabilities; portable Mach-O/parser
checks still run, and other filesystem errors still fail the suite.

macOS now draws the shared 40-pixel Flutter caption over a transparent AppKit
frame. AppKit retains accessible traffic-light controls, native drag/double-click
and close-to-tray handling. Home omits its redundant page heading, as Windows
already does; the navigation label remains. The default content size remains
1040×760, including the Flutter caption.

Before integrating upstream dev, focused validation passed: 50 Flutter tests,
18 packaging tests, 283 Engine/runtime Rust tests, strict Clippy, Flutter
analysis, 10 native desktop contract groups and an Apple Silicon debug GUI build.
The follow-up integrates upstream dev `57d7624864d940c322e16c855476fa235d182346`,
retaining both native Settings navigation and the new account/scenario routes,
and preserving the system Traditional Chinese locale resolver. Tests use mock authorization,
credential stores and temporary state. Actual authorization dialogs, native
window interactions, real VPN/proxy traffic and Intel execution were not run.
No replacement installer or release was published.

Final integration source: `90c57bf9d5ef1ec0607a99ae5dd1ae76e9220495`.

- `cargo test --workspace --all-targets --locked --offline`: 2,062 passed,
  3 ignored, no failures.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`
  and `pwsh -NoProfile -File tool/check_source.ps1`: passed. The latter covers
  Rust/Dart formatting, Flutter analysis, Kotlin, Ruff, PowerShell and Protobuf.
- Full non-golden Flutter run: 1,499 passed, 1 skipped, 1 preview-disposal test
  timed out at 30 seconds. All 7 tests in `preview_test.dart` then passed in an
  isolated retry. The full command therefore exited nonzero; it is not recorded
  as an entirely green single run. Earlier merge-check locale failures were
  corrected; the 13 locale/desktop-shortcut checks passed afterward.
- Python tool suite: 354 cases, 21 skipped, no failures (includes the 18
  packaging cases). Its initial restricted run could not open test sockets;
  the permitted rerun passed.
- `cargo check -p usque-engine --locked --offline --target x86_64-apple-darwin`:
  passed. This is not Intel hardware execution.
- Final Apple Silicon debug GUI build and committed-source repository policy:
  passed. Xcode logged an unrelated iOS simulator version warning while the
  macOS build succeeded. A separate invisible AppKit geometry probe confirmed
  a 1040×760 content view with the custom frame; window interactions remain
  untested.

The updated [mock UI comparison](MACOS_QA_CHECKLIST.md#可重現的介面比較) includes
both Flutter captions, preserves historical images separately and omits the
native AppKit traffic-light buttons. No host proxy, DNS, routes or actual
Keychain records were modified by these checks.

## Earlier complete-suite checks

Development host: Apple Silicon, current macOS, Xcode 27.1. Native deployment
targets and all embedded Mach-O files must support macOS 13. Cross compilation
does not establish Intel runtime compatibility.

The following counts belong to the earlier desktop candidate, rather than a
claim that every suite was repeated by a later background follow-up.

- Full Rust workspace: 1,940 passed, 3 ignored, including 28 macOS Engine
  checks for grouped proxy ownership, active-apply recovery, pending-recovery
  status, chain configuration and the provider adapter.
- Flutter non-golden suite: 1,296 passed, 1 skipped. This includes macOS Cmd
  shortcuts, bounded event framing, and layout checks with current macOS font paths.
- Native desktop contracts: 7 groups; maintenance contracts: 9 groups;
  package fixtures: 11 tests. Native sources are checked for arm64 and x86_64.
- Python tool regression: 323 cases, 21 skipped, no failures. macOS temporary
  directory fixtures resolve the system `/var` alias; production private-export
  code still rejects symlink ancestors and overwrites.
- Repository source checks passed: Rust format/strict Clippy, Dart format and
  analyze, Kotlin lint, Python Ruff, PowerShell analyzer and Protobuf lint/format.
- Both desktop/Engine/Network Extension/CEF/maintenance architectures have
  built successfully. Development DMGs include all 20 matching Mach-O files;
  image and ad-hoc signature checks do not establish runtime acceptance.

Earlier CEF candidates returned a success frame followed by a teardown watchdog
and exit 2. The popup fix prevents Chromium from accessing the user's Keychain
in this strictly in-memory, one-shot process. A focused renderer smoke then
returned success and exited 0, with the storage policy observed on renderer,
GPU and utility children. The complete smoke still fails its authentication-proxy
failure/exit check. This is partial evidence, not full verification acceptance;
the unsigned release launcher continues to refuse real credential handoffs.

The earlier [candidate validation record](MACOS_DESKTOP_VALIDATION.json) binds
its build, artifacts and incomplete device results separately. That candidate's
review is preserved in [PR #3](https://github.com/JustinL330/usque-app/pull/3),
targeting `dev`; subsequent work continues directly on `dev`.
- The earlier [network prototype record](MACOS_NETWORK_VALIDATION.json) preserves
  the first milestone's distinct code candidates and results.

These checks use local fixtures and synthetic adapters. They do not install an
application, activate extensions, change host proxy/DNS/routes, write actual
account credentials or establish external VPN sessions. CEF smoke uses a data
URL and temporary user directory, with no official challenge or account.

See the [development packaging guide](MACOS_DEVELOPMENT_PACKAGING.md) and
[maintenance foundation](../tool/macos-maintenance/README.md) for reproducible
commands and the corresponding boundaries.

## Background follow-up

The [background validation record](MACOS_BACKGROUND_VALIDATION.json) binds the
follow-up source tree and fresh development DMGs separately from the earlier
candidate. It records 8 network-observation fixtures, 20 CEF harness tests,
12 package fixtures, 13 maintenance contract groups and the 7 desktop/7 network
native groups. Python regression completed in two reviewed, noninteractive
partitions: 344 cases, 21 skipped, no failures. Source checks passed; the package
fixtures and Ruff passed again after correcting the pinned SDK's architecture-specific
V8 data filename. The earlier full Rust/Flutter suites were not repeated.

GUI, Engine, CEF and maintenance were compiled for both architectures; unchanged
Network Extension components were reused. Both fresh images passed their 20
Mach-O checks, required CEF data/helper checks and ad-hoc/image integrity checks.
Nothing was installed or launched as an application. That candidate's CEF executable
contracts and actual proxy/renderer smoke were not run, so the historical
authentication and teardown failures remain unresolved acceptance items.

## Traditional Chinese follow-up

Select **繁體中文（香港）** or **繁體中文（台灣）** in Settings → Language,
or follow the system language. The existing 21 application catalogs and saved
preference names are preserved. AppKit menu items, application-owned dialog copy,
errors follow the resolved Chinese catalog. That historical candidate's bundled
CEF window title also followed it. The removed CEF component used its supported `zh-TW` locale for both Traditional variants; the official
verification website controls its own page language.

The [Traditional Chinese validation record](MACOS_TRADITIONAL_CHINESE_VALIDATION.json)
binds the source tree to fresh arm64/x86_64 development images. The final Flutter
non-golden suite passed 1,302 tests with 1 skipped; native desktop contracts passed
10 groups, CEF's fake/pure harness passed 21 tests and package fixtures passed 12.
Static analysis and compile-only builds passed. No application/CEF process,
native window, permission prompt or system integration was started for this work.
Visual display, official verification and signed network acceptance remain pending.

## Remaining acceptance

The [user-requested test checklist](MACOS_QA_CHECKLIST.md) records the current
VPN/proxy/chain, UI, window and account results, with clearly labelled headless
Windows/macOS renders. It also preserves the historical CEF failure and device
acceptance items; macOS no longer ships that browser. A simulated account or successful
component build is never counted as a real Zero Trust or Proton registration.

Before exposing the VPN capability, signed providers must simultaneously prove
same-IP domain splitting, IPv4/IPv6, DNS separation, encrypted DNS failure
behavior, independent filtering, and no unauthorized physical egress during
component termination, reconnect, sleep or network changes. Missing credentials
after extension restart must stay blocked until authenticated handoff succeeds.

Permission cancellation, provider version mismatch, existing Proton account
authentication, extension replacement, actual upgrade/rollback/uninstall and
both Intel and Apple Silicon on macOS 13/current systems remain separate device
tests. A failed network prototype must preserve evidence and revise the
architecture; it must not enable full VPN or silently switch to PF.
