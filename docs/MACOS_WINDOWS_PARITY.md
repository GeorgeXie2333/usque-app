# macOS comparison with Windows

Target: macOS 13 or later, separate native arm64 and x86_64 packages. This work
continues from clean `dev` at `948de61c`, with the earlier experimental backup
retained for review. It is not yet a **macOS full-feature version**.

`main` was restored with history-preserving revert `0759ff02`; its tracked tree
matches `1d44597d`. macOS development continues on a separate branch. Do not
merge this work into `main` before the owner requests it again.

## Feature comparison

“Implemented” describes code, “isolated” describes fake/local fixtures or build
checks, and “device pending” requires actual macOS permissions, providers or
hardware. None of the isolated results establishes physical-egress safety.

| Windows capability | macOS implementation | Validation and remaining difference |
| --- | --- | --- |
| MASQUE proxy, HTTP/SOCKS TCP/UDP | Existing shared runtime, with ordinary-user Engine as proxy-only owner | Shared Rust regression; no new live-account/network acceptance |
| Full-system VPN | Packet Tunnel is the sole runtime owner; Engine proxy sockets enter the same system data plane | Provider adapters and loopback contracts; production capability remains false |
| Country/custom rules, DIRECT/REJECT/PROXY, advertising rules | Shared Rust immutable policy snapshots; separate flow, DNS and filter providers | Rule/epoch contracts; same-IP domain split and real `enforceRoutes` egress pending |
| IPv4/IPv6 and encrypted DNS | Shared runtime and separate DNS provider; encrypted-upstream errors do not request plaintext fallback | IPv6 loopback and policy contracts; physical/tunnel DNS, leakage and LAN device acceptance pending |
| Kill Switch and reconnect | Independent protection before tunnel admission, credential-less restart blocked; Engine loss is not Stop | Isolated lifecycle/fault contracts; actual provider crash, sleep and network-change protection pending |
| Chain exits and account managers | macOS now wires the shared HTTP, SOCKS, OpenVPN, WireGuard, WARP WireGuard, VPN Gate and Proton handlers | Engine contract/regression tests; live service acceptance pending; existing experimental flags retained |
| Account bootstrap protection | Existing connected WARP runtime can be reused; unprotected standalone physical bootstrap remains refused | No fallback that bypasses authenticated protection |
| Credentials and configuration | User Engine owns Keychain and encrypted chain storage; dedicated secret transfer and durable refresh acknowledgement | In-memory test vault; no host credentials read or replaced by tests |
| System proxy | Per-service SystemConfiguration transaction, authorization retry, encrypted ownership journal, compare-and-restore; pending recovery blocks new sessions and key deletion | Fake preferences/failure injection; real authorization cancellation and crash recovery pending; newly created services require reapply |
| Continuous desktop events | Authenticated per-user Unix control/events sockets; bounded, fragmented/coalesced frame decoder | Engine and Dart tests; hidden-window event loop is implemented |
| Tray/background controls | Native menu bar status, connect/disconnect, VPN/proxy actions, hidden-window updates, explicit disconnect-and-quit | Native contract/type checks; real tray interaction pending |
| Window lifecycle | Close-to-menu-bar default, Dock reopening, single-instance activation, remembered/clamped window position | Native contract/type checks; signed release UI acceptance pending |
| Settings and shortcuts | Native Settings menu, Cmd+,; Cmd+1–4 and Cmd+S; existing unsaved-change guards | Dart widget and native contracts |
| Login startup and notifications | SMAppService default off, authorization/cancel retry, native notifications | Build/contracts; real permission dialogs and login reboot pending |
| Configuration import/export and diagnostics | Bounded batch file picker, JSON credential/config saves, diagnostics saved as ZIP | Dart regression/native contracts; real file-dialog acceptance pending |
| Zero Trust browser login | ASWebAuthenticationSession callback, strict organization/path/token validation, attempt fencing and cancellation | Isolated URL/lifecycle checks; actual identity-provider login pending |
| Proton verification window | Dedicated CEF component on all macOS 13+ targets, official-origin allowlist, in-memory context, authenticated loopback proxy, sandbox required | Framing/challenge checks; final rebuilt renderer/proxy shutdown smoke pending; official challenge and signed launcher pending |
| Architecture-specific installation | Native arm64 and x86_64 DMG builders with drag-to-Applications layout, complete nested Mach-O/minimum-OS checks | Development image verification; Intel execution and macOS 13 hardware pending |
| Update selection and recovery | Maintenance foundation verifies version, native hardware architecture, name, size, hash and signing; same-volume atomic swap with retained backup and hash-fenced recovery | Temporary injected-verifier tests including Intel-to-arm64 migration; production installation disabled because trust authority is unconfigured |
| Extension update and uninstall | Application transaction leaves extensions/user data separate; manual app removal preserves user data | Extension approval/restart orchestration and managed uninstall remain pending |
| Signed production distribution | Development packages are explicitly ad-hoc and not notarized | Developer ID, notarization, Gatekeeper distribution and full two-architecture acceptance pending |

## Current isolated checks

Development host: Apple Silicon, current macOS, Xcode 27.1. Native deployment
targets and all embedded Mach-O files must support macOS 13. Cross compilation
does not establish Intel runtime compatibility.

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
- Both final desktop/Engine/Network Extension/maintenance architectures have
  built successfully. Final DMG assembly waits for CEF shutdown verification.

An earlier CEF candidate returned a success frame and exited cleanly, but a fresh
verified-SDK rebuild exposed teardown failures (exit 2 after the success frame).
That final smoke remains failed until a new candidate proves clean shutdown
repeatedly. A framed success followed by a watchdog exit is never counted as a
pass, and the unsigned release launcher still refuses real credential handoffs.
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

## Remaining acceptance

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
