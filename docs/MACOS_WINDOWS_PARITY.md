# macOS comparison with Windows

Target: macOS 13 or later, separate native arm64 and x86_64 packages. This work
continues from clean `dev` at `948de61c`, with the earlier experimental backup
retained for review. It is not yet a **macOS full-feature version**.

`main` was restored with history-preserving revert `0759ff02`; its tracked tree
matches `1d44597d`. At the owner's request, the preserved macOS development
commits were fast-forwarded onto `dev`, and further development stays there. Do not
merge this work into `main` before the owner requests it again.

The owner is using this workstation. Current validation is background-only:
no CEF/application/browser launches, WindowServer smoke tests, file dialogs,
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
| Account bootstrap protection | Existing connected WARP runtime can be reused; unprotected standalone physical bootstrap remains refused | No fallback that bypasses authenticated protection |
| Credentials and configuration | User Engine owns Keychain and encrypted chain storage; dedicated secret transfer and durable refresh acknowledgement | In-memory test vault; no host credentials read or replaced by tests |
| System proxy | Per-service SystemConfiguration transaction, Engine-lifetime network authorization reuse, silent rights preflight on cleanup, encrypted ownership journal, compare-and-restore; pending recovery blocks new sessions and key deletion | Fake preferences/authorization/failure injection; real authorization cancellation and crash recovery pending; newly created services require reapply |
| Continuous desktop events | Authenticated per-user Unix control/events sockets; bounded, fragmented/coalesced frame decoder | Engine and Dart tests; hidden-window event loop is implemented |
| Tray/background controls | Native menu bar status, connect/disconnect, VPN/proxy actions, hidden-window updates, explicit disconnect-and-quit | Native contract/type checks; real tray interaction pending |
| Output availability | Home, Proxy, tray and shortcuts honor Engine VPN/proxy capabilities; unavailable saved outputs can still be turned off; Settings explains unavailable VPN protection | Headless capability/refresh contracts; unsigned VPN stays disabled |
| Window lifecycle | Close-to-menu-bar default, Dock reopening, single-instance activation, 1040×760 default content with first-launch centering and remembered/clamped geometry | Native geometry/type checks and headless small-screen layouts; actual window interaction pending |
| Settings and shortcuts | Native Settings menu, Cmd+,; Cmd+1–4 and Cmd+S; existing unsaved-change guards | Dart widget and native contracts |
| Traditional Chinese | Existing Hong Kong/Taiwan catalogs now also drive native menus, app-owned file-dialog copy and CEF UI language; framework controls use the same resolver and hidden windows follow system language changes | Headless language-switch/persistence regressions and native contracts; actual window display remains untested |
| Login startup and notifications | SMAppService default off, authorization/cancel retry, native notifications | Build/contracts; real permission dialogs and login reboot pending |
| Configuration import/export and diagnostics | Bounded batch file picker, JSON credential/config saves, diagnostics saved as ZIP | Dart regression/native contracts; real file-dialog acceptance pending |
| Zero Trust browser login | ASWebAuthenticationSession callback, strict organization/path/token validation, attempt fencing and cancellation | Isolated URL/lifecycle checks; actual identity-provider login pending |
| Proton verification window | Dedicated CEF component on all macOS 13+ targets, official-origin allowlist, in-memory context, authenticated loopback proxy, sandbox required; Chromium storage does not access the user's Keychain | Five framing checks and a focused renderer exit pass; the complete proxy-failure smoke remains failed; official challenge and signed launcher pending |
| Architecture-specific installation | Native arm64 and x86_64 DMG builders with drag-to-Applications layout, complete nested Mach-O/minimum-OS checks | Development image verification; Intel execution and macOS 13 hardware pending |
| Update selection and recovery | Maintenance foundation verifies version, native hardware architecture, name, size, hash and signing; same-volume atomic swap, retained backup and durable, candidate-bound health receipts | Temporary injected-verifier tests including Intel-to-arm64 migration and confirmation retries across restarts; live health authentication is not integrated and production installation remains disabled |
| Extension update and uninstall | Application transaction leaves extensions/user data separate; manual app removal preserves user data | Extension approval/restart orchestration and managed uninstall remain pending |
| Signed production distribution | Development packages are explicitly ad-hoc and not notarized | Developer ID, notarization, Gatekeeper distribution and full two-architecture acceptance pending |

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

See the [development packaging guide](MACOS_DEVELOPMENT_PACKAGING.md),
[CEF component](../apps/usque_gui/macos/ProtonVerification/README.md), and
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
Nothing was installed or launched as an application. The current CEF executable
contracts and actual proxy/renderer smoke were not run, so the historical
authentication and teardown failures remain unresolved acceptance items.

## Traditional Chinese follow-up

Select **繁體中文（香港）** or **繁體中文（台灣）** in Settings → Language,
or follow the system language. The existing 21 application catalogs and saved
preference names are preserved. AppKit menu items, application-owned dialog copy,
errors and the bundled CEF window title now follow the resolved Chinese catalog.
CEF uses its supported `zh-TW` locale for both Traditional variants; the official
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
Windows/macOS renders. It also preserves the unresolved CEF failure and device
acceptance items surfaced in the root READMEs. A simulated account or successful
component build is never counted as a real Zero Trust or Proton registration.

Before exposing the VPN capability, signed providers must simultaneously prove
same-IP domain splitting, IPv4/IPv6, DNS separation, encrypted DNS failure
behavior, independent filtering, and no unauthorized physical egress during
component termination, reconnect, sleep or network changes. Missing credentials
after extension restart must stay blocked until authenticated handoff succeeds.

Permission cancellation, provider version mismatch, signed CEF launch, official
Proton challenge, extension replacement, actual upgrade/rollback/uninstall and
both Intel and Apple Silicon on macOS 13/current systems remain separate device
tests. A failed network prototype must preserve evidence and revise the
architecture; it must not enable full VPN or silently switch to PF.
