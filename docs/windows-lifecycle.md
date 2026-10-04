# Windows installer and service lifecycle

This is the maintainer reference for installation, Agent recovery, upgrades and
uninstall. User steps are in [Installation and removal](INSTALLATION.md).
Development-machine limits and isolated test requirements remain in
[Contributing](../CONTRIBUTING.md#development-machines).

Current recovery journals use schema 5. Schemas 2–4 migrate conservatively;
older Agents cannot read schema 5. Agent protocol remains 3 and requires an
explicit capability for protected tunnel replacement. During HTTP/SOCKS VPN
settings or account replacement, the Agent journals a separate persistent WFP
guard before restoring the old operation and retains it until the successor
commits. An uncertain cleanup or RPC reply cannot authorize an unprotected
replacement. Explicit disconnect uses the authenticated abort path. Read the
[protected replacement contract](NETWORK_SETTINGS.md#protected-windows-operation-replacement)
for direct exceptions, failure handling and validation limits.

## Installer language and contents

The EXE has a native C++ setup window backed by WiX 5.0.2 Burn and the existing
MSI transaction. It initially selects the current Windows UI language and lets
the user change it before applying the transaction. It
ships Arabic, German, Spanish, Persian, French, Indonesian, Italian, Japanese,
Korean, Dutch, Polish, Brazilian Portuguese, Russian, Thai, Turkish, Ukrainian,
Vietnamese, Simplified Chinese, Hong Kong Chinese, and Taiwan Chinese
transforms, with English as the base and fallback. This choice affects the
installer interface only; Usque's language remains independently selectable in
the app.

The interactive installer:

- asks for administrator approval to install the Agent service (`usque-agent.exe`,
  service name `UsqueAgent`, display name "Usque Agent");
- lets you choose the install directory;
- installs the GUI, unprivileged engine, Agent, official Wintun DLL, and Start Menu shortcut;
- keeps that directory on a major upgrade;
- installs the Agent as a demand-start service and does not leave it running;
- does not start a VPN during install.

The setup and uninstall windows share the 21-language catalog in
`packaging/windows/setup/strings.json`. The MSI's existing custom UI remains
available for direct MSI deployments; the EXE suppresses that inner UI and
receives Burn progress, cancellation and files-in-use callbacks instead. The
outer window never substitutes successful progress for a successful final
transaction result. A same-version package with a different ProductCode still
uses the existing major-replacement policy, not MSI repair.

After successful installation, desktop-shortcut creation and login startup
are current-user operations outside the completed MSI transaction. The native
setup links the same shell-operation implementation as the Windows runner and
uses its in-process query/apply entry point, passing only the completed MSI's
known installation target. It never needs to launch the installed GUI to apply
these choices, including when a restart leaves an older GUI awaiting replacement.
The installed runner also handles `--query-setup-options` and `--setup-options` before
creating Flutter or starting the Engine. The latter accepts exactly one
`--desktop-shortcut=create|keep` and one
`--start-on-login=enable|disable|keep`. Its versioned JSON response reports each
option independently; retry does not repeat successful operations. Neither runner
command accepts an executable or filesystem target argument. New setup operations
reject elevated and service identities; they do not guess the original user
from an administrator token. The existing MSI `--remove-startup` path retains
its impersonation contract and removes only an owned desktop link with the
matching target and product marker. Unknown links and startup entries remain.
Desktop-link cleanup is best effort: an unavailable Desktop or a locked link
does not fail uninstall or add a user prompt. Startup-entry cleanup remains
checked independently, including when COM initialization prevents link cleanup.

Successful installation that requires a restart still offers desktop and
login-startup choices. Opening the app is disabled until after restart. The
chosen shell operations complete before leaving or requesting restart; failed
items remain separately retryable. A retry after a failed restart-bound
completion returns to the restart choice instead of unexpectedly restarting.

Native setup links the hash-locked WiX API and DUtil libraries. The lock,
upstream provenance and license are under `packaging/windows/setup`; the
bundle includes the complete WiX license and notices. The release signs the
native setup EXE before embedding it, using the same identity as the MSI and
other first-party Windows executables. Installer EXE inventories include these
two native dependencies.

## Agent startup, device reuse and recovery

After installation, an interactive Windows user can start the Agent through
Usque without another UAC prompt. The service ACL grants that user only start
and status-query access; stopping, deleting, or reconfiguring the service still
requires an administrator. The Agent starts when the Engine first needs a
privileged operation. Without a managed device, it exits after 10 clean idle
seconds with no clients or recovery jobs. After the first TUN use, Engine holds
an independent device lease until the application fully exits. Normal
disconnect/reconnect reuses that device; it does not keep network configuration
or a packet session active while disconnected.

The service temporarily changes itself to automatic start before Usque records
or applies privileged network state. This lets the next boot recover an
interrupted VPN or system-proxy transaction. At startup it verifies the exact
adapter identity and the network resources needed for reattachment. A surviving
tunnel, or a lost Engine lease, gets a 30-second reattachment window. Missing
resources are recovered instead of being treated as a live tunnel. If no Engine
returns, the Agent restores Usque network state, changes back to demand start,
and exits.

On confirmed shutdown/restart the Agent stops admitting operations, stops packet
forwarding, and restores network state within a 30-second service preshutdown
budget. Ordinary service stops retain the existing maintenance/reattachment
behavior. Interrupted or failed cleanup keeps its journal for the next start.

Unrestored connection effects keep the Agent available and automatic. When
only final device retirement remains, one bounded attempt can save a pending
device record and stop normally; the next Agent startup must recover it before
creating another device. Failed network cleanup or journal persistence cannot
use this exit exception. Starting a connection
first makes at most one authenticated, operation- and generation-checked recovery
attempt, before DNS or VPN startup. It never recovers an active session or another
user's transaction. Failed or timed-out recovery does not start a new tunnel;
the journal is retained and the app displays a recovery-specific error. Older
Agents without device-reuse capability require a matching application/Agent
update; new TUN requests cannot fall back to the old per-connection device path. Do not delete the
recovery journal to bypass an error.

Established CONNECT-IP reconnects reuse the physical-network observer described in
[connection recovery](h3-client-reliability.md#established-connect-ip-recovery).
The observer distinguishes confirmed `AGENT_PHYSICAL_NETWORK_OFFLINE` from failed
queries and older Agents' generic errors. This does not relax the Agent's startup,
exact-egress, cleanup or automatic-recovery checks.

## System-proxy ownership and recovery

The system-proxy output points at an explicit loopback HTTP listener. Turning it
off restores the captured user settings: an originally disabled manual proxy
returns to disabled, while an originally enabled proxy returns to its previous
configuration. When the current server differs from both the captured and
applied addresses, cleanup preserves that replacement and its related settings,
including its PAC configuration. Captured/applied intermediate values remain
eligible for interrupted-operation recovery.

The Agent checks the resolved identity of the opened registry object before
reading or changing proxy values and uses that same handle throughout the
operation. A valid caller SID alone does not authorize a different resolved
object. Recovery retains its ownership marker until settings are flushed and
the change notification succeeds. Retrying after marker removal still flushes
and notifies, so an interrupted final step cannot become a false success.

The Engine observes the proxy lease independently of transport health. A lost
lease invalidates the reported system-proxy runtime state. Reattaching an active
tunnel closes the previous lease and restores any retained proxy receipt before
applying the replacement, without discarding the tunnel's persistent protection.
Failed frontend or tunnel-attachment changes restore the previous runtime when
possible; if that compensation fails, the connection stops and reports failure.

An unsuccessful proxy shutdown retains its recovery owner and operation ID even
after the lease pipe closes. A later explicit Disconnect or exit cleanup retries
that same operation over a new authenticated pipe, within one 30-second budget.
It never restores a successor transaction. The owner is released only after a
successful Restore reply confirms cleanup or a state query confirms fully Clean
platform state. An Active tunnel with an inactive proxy flag alone is not proof:
an unfinished proxy receipt may still need restoration. Fully Clean state also
confirms cleanup when the entire tunnel was rolled back after a sidecar failure.
Timeouts, cancellations and repeated failures retain the error and owner; they
do not permit a new connection or schedule an automatic Engine retry.

These ownership and failure paths have deterministic memory and named-pipe
tests. Those tests do not prove Windows Settings UI synchronization, connection
flags, WinINet notification across user/service contexts, or actual installed
Agent crash recovery. Those behaviors require snapshot-VM validation, including
the case where the Windows proxy was disabled before Usque enabled it.

### Review validation, 2026-09-30

The tested executable source is
`cfe17f53aa671f285fa465f9904e06ddc30c3585`, based on
`f29077a4d45d1c4dd6bc167eb644f4bcf32b5451`. The subsequent documentation-only
commit does not change that executable source. The four code batches are
`9799b8a` (frontend classification), `afeb24b` (Agent ownership/recovery),
`4628bca` (Engine leases/compensation), and `cfe17f5` (settings application).

| Review finding | Recheck and evidence |
| --- | --- |
| HTTP shutdown rebuilds unrelated outputs | Confirmed in the planner; fixed with hot classification and dependency tests. The real GUI request also exposed an earlier validation rejection, now covered by wire-input tests. |
| A saved proxy-off switch is never applied behind a busy executor | Confirmed with the fake Engine. Coalescing, runtime/state contention, HTTP-dependent shutdown, timeout, cancellation and failure tests cover the fix. |
| Expired proxy lease remains Active | Confirmed from ownership/health flow; fake pipe EOF and drop tests cover invalidation and pipe release. Existing-session Connect refreshes runtime health. |
| Reattachment duplicates a retained proxy step | Confirmed for retained applied and intended receipts. Capability/protocol, request-order and journal tests cover idempotent cleanup before replacement. |
| Failed hot changes lose previous listeners/proxy | Confirmed with injected runtime failures; compensation-success and compensation-failure tests cover restore or explicit stop. |
| Cleanup disables an externally selected replacement proxy | Confirmed with production recovery flow on memory settings; replacement configuration is preserved. |
| Failed final flush/notification is retried as success | Confirmed with production recovery fault injection; unfinished durability/notification remains retryable. |
| Caller SID validation does not constrain the resolved registry object | Confirmed from the registry API contract. Resolved-path rejection tests and a read-only real key-name query verify the new check; an attack was not executed. |

The workstation checks below completed successfully from the repository root
using Rust 1.97.1 and PowerShell 7. `python` denotes the verified Python 3.12.14
runtime executable used for the policy check.

```powershell
cargo fmt --all --check
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2
& .\tool\build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy
python tool/check_repository_policy.py
git diff --check
```

The x64-v2 Rust release compile-only check also completed successfully. No
package is part of this validation. The native resolved-key query was read-only and did
not read proxy values, create links, or change registry state.

Actual Windows proxy apply/restore and Settings UI readback, installed Agent
restart/crash recovery, VPN/platform restoration, registry-link attack
execution, and external leak observation are `not_run`. The specific reported
black-box UI failure has not been reproduced in a snapshot VM. Connection-level
flags and user/service WinINet notification remain unconfirmed causes; no raw
connection-blob or notification-architecture change was made on that assumption.

## Upgrade ordering and payload replacement

The newer-Agent-first ordering below was introduced in v0.2.5. It is retained
in v0.2.7; the original v0.2.4 package does not have these fixes. Compile-only
and MSI table checks do not prove a successful installed upgrade or recovery.

A running Usque process is asked to disconnect and exit through Windows Restart
Manager before any installed files are replaced. Usque treats that maintenance
request differently from an ordinary window close, so the close-to-tray setting
does not keep the process alive. If an older or unresponsive build cannot honor
the request, Windows Installer uses its bounded force-shutdown fallback; it does
not restart that process during the upgrade.

A major upgrade stops the Agent, installs the newer recovery-compatible Agent
inside the Windows Installer transaction, and then removes the older product.
The older package's recovery action therefore runs the fixed Agent from the
stable installation path while it restores Usque-owned WFP, route, DNS,
system-proxy, and Wintun state. Component reference counting keeps the new
files and service registration in place when the older product is removed. The
upgrade keeps user profiles, settings, logs, caches, Credential Manager
identities, and the recovery journal the new version needs.

Same-version replacements use an explicit payload overwrite policy
(`REINSTALLMODE=amus`) before file costing. This replaces equal-version and
unversioned application files together, instead of leaving an older Agent or
GUI beside a new Engine. All installed files must remain under the private
Usque installation directory; user data is not part of that payload. Product
downgrades are still rejected, and repair/modify/patch operations remain
unsupported. The setting is not a request to run an MSI repair.

This ordering is also the supported bridge from `v0.2.4`, whose Agent could
mistake asynchronous Wintun device removal for a permanent cleanup failure. A
user whose `v0.2.4` uninstall failed should use a verified official Windows
package from `v0.2.5` or later containing this bridge, then uninstall the newer version if
removal was the original goal. If recovery still fails, stop and report the
failure with sanitized diagnostics; development artifacts are not substitutes
for the official package.
Do not work around the failure by deleting the Agent, its recovery journal, or
Windows network objects manually.

If privileged network state cannot be restored, the upgrade stops with an error. It must not continue with leftover routes, filters, DNS, proxies, or adapters.

## Uninstall and administrator automation

Confirming Uninstall keeps the original-user Rust/Win32 window open. A worker
thread installs an external MSI record callback and uses
`MsiConfigureProductExW` with `INSTALLSTATE_ABSENT`. Internal MSI UI uses
`INSTALLUILEVEL_NONE | INSTALLUILEVEL_UACONLY | INSTALLUILEVEL_SOURCERESONLY`,
preserving system UAC and source-location dialogs while the outer window handles
other transaction prompts. MSI can request a matching original package when its
cached source is unavailable. Rejecting this UI configuration stops before the
uninstall transaction; quiet automation retains its silent launcher. It then:

1. asks the GUI and Engine to disconnect and exit, with a bounded force fallback for an unresponsive older build;
2. stops the Agent;
3. removes Usque WFP Kill Switch objects;
4. restores journaled routes, DNS, and system-proxy state;
5. removes the Usque-owned Wintun adapter;
6. removes the service, program files, shortcut, and clean machine journal.

The shared Wintun driver package stays, because another application may use it. A successful uninstall must not leave an Usque Wintun adapter.

The callback copies only known action names, numeric progress and error codes,
and transient files-in-use display names. It does not retain MSI record handles
or export arbitrary MSI records. Cancellation is enabled only when MSI allows
it, before user-data deletion, and outside rollback or Burn cleanup. A request
returns through the MSI callback and waits for the transaction to finish; it
never kills the worker or implies that deleted data has been restored.

MSI removal and hidden Burn registration cleanup are separate results. Both
must finish before the window reports success; reboot-required status is
retained across both phases. Only failed Burn cleanup can be retried directly
in the current window. MSI failures recheck installed state and reset the data
deletion choice before a new confirmation. Once deletion may have started,
failure copy states that some data may already be gone. Save-details exports
only the version, stage and result codes, without paths, accounts,
network addresses, raw logs or automatic upload.

A registration retry clears the previous failure from the running page but
retains the completed MSI result, user-data choice and trusted bundle path.
Successful retry preserves any required restart and the data-removal result.
If the cleanup worker ends without a result, the saved context still permits
only registration cleanup; it cannot return to an MSI or data-deletion retry.
The inert preview follows the same result-combination rules.

Restart-required results offer later or immediate restart. Immediate restart
first displays a save-work confirmation; only its explicit confirmation
temporarily enables the current token's existing shutdown privilege and calls
the non-forcing Windows restart API. Preview builds never call that API. A
failed or cancelled restart request remains visible and does not change the
uninstall transaction result.

The data-deletion option cannot be undone and does not affect other Windows
users. Leave it unchecked to keep local data for a later reinstall. The
registered `QuietUninstallString` uses a hidden system PowerShell host to stage
and verify the helper, then runs its temporary copy with `--quiet`. It keeps
user data and waits for both Windows Installer and hidden Burn cleanup before
returning the final failure or reboot-required exit code. Use this registered
command for automation, not `usque-uninstall.exe --quiet` in the install folder.
Because Windows Installer and Burn retain separate per-machine trust
boundaries, Windows may request administrator approval for each phase.
Administrators of an MSI-only deployment may instead use
`msiexec /x {ProductCode} /qn /norestart`; do not use that direct command for
an EXE-bundle installation because it cannot remove Burn's cached registration.
Upgrades never show the confirmation dialog and never purge user data.
Re-running the installer EXE while Usque is installed still offers the same
default-off deletion checkbox on the maintenance remove path.

MSI Repair, Modify, and Patch are not supported, and the Start Menu shortcut is
non-advertised so launching it cannot trigger MSI self-repair. Repair could stop
the Agent or overwrite the crash-recovery start mode while privileged network
state is active. The native installer has no Repair entry. The direct MSI
interface explains this if Repair is selected, and command-line maintenance is
rejected before `StopServices`. Use the supported
major-upgrade path, or uninstall and reinstall while leaving the data-deletion
checkbox off.

If recovery fails, uninstall stops rather than leaving privileged network
residue behind. Windows install, recovery, upgrade, connected-uninstall, and
platform-state restoration tests belong on a snapshot VM. Externally observed
IPv4, IPv6, DNS, Kill Switch, and route leak tests belong on the independent
controlled-network observer. Neither belongs on a daily-driver machine.
Development-machine limits are in [CONTRIBUTING.md](../CONTRIBUTING.md).
