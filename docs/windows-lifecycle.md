# Windows installer and service lifecycle

This is the maintainer reference for installation, Agent recovery, upgrades and
uninstall. User steps are in [Installation and removal](INSTALLATION.md).
Development-machine limits and isolated test requirements remain in
[Contributing](../CONTRIBUTING.md#development-machines).

## Installer language and contents

The EXE selects its MSI interface from the current Windows UI language. It
ships Arabic, German, Spanish, Persian, French, Indonesian, Italian, Japanese,
Korean, Dutch, Polish, Brazilian Portuguese, Russian, Thai, Turkish, Ukrainian,
Vietnamese, Simplified Chinese, Hong Kong Chinese, and Taiwan Chinese
transforms, with English as the base and fallback. This choice affects the
installer interface only; Usque's language remains independently selectable in
the app.

The interactive installer:

- asks for administrator approval to install the `usque-agent` service;
- lets you choose the install directory;
- installs the GUI, unprivileged engine, Agent, official Wintun DLL, and Start Menu shortcut;
- keeps that directory on a major upgrade;
- installs the Agent as a demand-start service and does not leave it running;
- does not start a VPN during install.

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
user whose `v0.2.4` uninstall failed should use a verified official `v0.2.7`
Windows package containing this bridge, then uninstall the newer version if
removal was the original goal. If recovery still fails, stop and report the
failure with sanitized diagnostics; development artifacts are not substitutes
for the official package.
Do not work around the failure by deleting the Agent, its recovery journal, or
Windows network objects manually.

If privileged network state cannot be restored, the upgrade stops with an error. It must not continue with leftover routes, filters, DNS, proxies, or adapters.

## Uninstall and administrator automation

Confirming Uninstall starts Windows Installer, which then:

1. asks the GUI and Engine to disconnect and exit, with a bounded force fallback for an unresponsive older build;
2. stops the Agent;
3. removes Usque WFP Kill Switch objects;
4. restores journaled routes, DNS, and system-proxy state;
5. removes the Usque-owned Wintun adapter;
6. removes the service, program files, shortcut, and clean machine journal.

The shared Wintun driver package stays, because another application may use it. A successful uninstall must not leave an Usque Wintun adapter.

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
state is active. The installer explains this if Repair is selected, and
command-line maintenance is rejected before `StopServices`. Use the supported
major-upgrade path, or uninstall and reinstall while leaving the data-deletion
checkbox off.

If recovery fails, uninstall stops rather than leaving privileged network
residue behind. Windows install, recovery, upgrade, connected-uninstall, and
platform-state restoration tests belong on a snapshot VM. Externally observed
IPv4, IPv6, DNS, Kill Switch, and route leak tests belong on the independent
controlled-network observer. Neither belongs on a daily-driver machine.
Development-machine limits are in [CONTRIBUTING.md](../CONTRIBUTING.md).
