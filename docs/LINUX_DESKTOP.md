# Linux desktop / Linux 桌面版

The Linux desktop app uses the real Rust Engine; the separate **Usque UI
preview** is a debug-only build with simulated data. Linux packages have not
been included in a stable release; v0.3.3-beta.1 introduced the first Linux
pre-release packages, and beta.2 improves startup and recovery diagnostics.
Back up before manual testing. The validation table below shows
what has been tested.

Linux 桌面版使用真实的 Rust Engine；单独的 **Usque UI preview** 是仅供调试、使用
模拟数据的版本。Linux 安装包尚未纳入稳定版；v0.3.3-beta.1 首次提供 Linux 包，beta.2 改进启动与恢复诊断。
手动测试前请备份，已测试的范围见下方验证表。

## Choose and verify a package

The target baseline is Ubuntu 22.04+, Debian 12+, and Fedora releases still
supported when the package is published, with systemd and a GNOME or KDE
session on X11 or Wayland. Stable v0.3.2 predates Linux packages; this Beta
does not change the stable Latest or add a Beta update channel. Filenames use the release version in place of
`vX.Y.Z`:

| Host | AppImage | Debian package | RPM package |
| --- | --- | --- | --- |
| Generic x86-64 | `usque-vX.Y.Z-linux-amd64.AppImage` | `usque-vX.Y.Z-linux-amd64.deb` (`amd64`) | `usque-vX.Y.Z-linux-amd64.rpm` (`x86_64`) |
| ARM64 | `usque-vX.Y.Z-linux-arm64.AppImage` | `usque-vX.Y.Z-linux-arm64.deb` (`arm64`) | `usque-vX.Y.Z-linux-arm64.rpm` (`aarch64`) |

Use only files attached to the matching official GitHub Release. Compare the
package hash with `SHA256SUMS` and GitHub's asset digest. Linux also has a signed
manifest for each architecture, for example
`usque-vX.Y.Z-linux-amd64.manifest.json` and its raw 64-byte `.sig` file.
The stable Ed25519 public key is checked in at
[release-public-key.hex](../packaging/linux/release-public-key.hex). Obtain
that public key through the trusted source repository; never accept a different
key supplied beside an untrusted download.

With the public key, signed manifest and signature in the current directory,
these commands verify the signature using system tools without running the
AppImage or installing anything:

```shell
python3 -c 'from pathlib import Path; key=Path("release-public-key.hex").read_text().strip(); assert len(key)==64; Path("usque-public.der").write_bytes(bytes.fromhex("302a300506032b6570032100"+key))'
openssl pkeyutl -verify -pubin -keyform DER -inkey usque-public.der -rawin -in usque-vX.Y.Z-linux-amd64.manifest.json -sigfile usque-vX.Y.Z-linux-amd64.manifest.json.sig
sha256sum usque-vX.Y.Z-linux-amd64.AppImage
```

Require `Signature Verified Successfully`, then compare the complete package
digest and size with that exact manifest entry. Use `arm64` for ARM64. A valid
signature does not authorize a downgrade or a different installation format.
Each update independently checks the same constraints inside the privileged
helper.

Canonical Beta versions use `X.Y.Z-beta.N` in filenames, signed manifests
and root-owned installation records. Only deb/rpm metadata maps this to
`X.Y.Z~beta.N` (RPM Release `1`). Numeric Beta ordinals sort before the
corresponding stable version; malformed, older and mixed-format candidates
are rejected. Do not edit the version record to bypass these checks.

## Install and start

For Debian/Ubuntu, install the verified `.deb` with the system package manager;
on Fedora, install the verified `.rpm`. The packages install a root-owned
service and a normal user desktop application. Installing or starting the
service does not connect a VPN. Launch **Usque**, finish the terms/account
steps, and connect explicitly from Home.

For AppImage, make the verified file executable and launch it. The application
can perform account and local-proxy work without copying a service into the
system. Use **Background service → Install service** in setup or
Settings to authorize the privileged service installation. Cancelled
authorization leaves the action available to retry. The installed service runs
from `/usr/lib/usque`, never the temporary AppImage mount. Restart Usque after
installing or repairing the service so the desktop selects the matching,
trusted Engine from that fixed directory.

AppImage service setup requires the host nftables utility, systemd D-Bus tools
and libsecret runtime library. Install the distribution packages `nftables`,
`systemd` and `libsecret-1-0` (Debian/Ubuntu) or `libsecret` (Fedora), plus a
compatible session keyring. The installer checks dependencies after signature
verification and before modifying the service; missing dependencies can be fixed
and retried. It does not install dependencies automatically.

Initial AppImage installation runs an installer from the user-selected
AppImage under a normal administrator prompt. Its self-verifier cannot prove
its own authenticity before that first execution: complete the independent
signature check above before granting authorization. Later service maintenance
uses the already installed root-owned helper and fixed public key.

Without FUSE, extract the verified AppImage as the normal user and start
`squashfs-root/AppRun`. It exports the same application directory used for the
service payload. Automatic replacement of an extracted directory is not an
AppImage update: download and verify the new image, extract it to a new
directory and use the service repair/install flow when a version mismatch is
reported.

Do not mix AppImage service installations, deb and rpm. The protected
`/etc/usque/install.json` record identifies the format, architecture, protocol,
version and installed Engine digest. Change formats only after removing the
existing system installation; personal data is kept separately.

## Credentials and the desktop session

Credentials and the encryption key for imported chain profiles use the session
Secret Service, such as GNOME Keyring or a compatible KWallet service. A
missing, locked or cancelled keyring stays retryable. Usque does not fall back
to plaintext credentials. Unlock the keyring in the desktop session and retry
the operation; installing the root Agent does not unlock a user's keyring.

At startup the window displays non-secret account settings before credential
validation finishes. Connection and account actions wait for that validation;
the initial summary never establishes that an identity is ready. Window size
and maximization changes are coalesced and saved off the GTK main thread, with
the latest pending state drained before a normal exit.

Configuration, cache and logs use the user's XDG directories. The GUI and
Engine communicate in a private `0700` directory below `XDG_RUNTIME_DIR`, with
`0600` sockets and authenticated peer credentials. Another user cannot use that
control channel. Login autostart is a current-user desktop entry; auto-connect
occurs only after the application starts and credentials are available.

The tray uses StatusNotifierItem. If no compatible tray host is available, or
the host disappears, the window remains visible and close-to-tray is disabled.
Window size and maximization are saved; Wayland placement belongs to the
compositor. GNOME/KDE system proxy settings apply in the user's session and
require the HTTP proxy output. Restoration preserves later external changes.

## TUN mode and host networking

The Agent selects each authorized socket's physical exit with a kernel route
lookup for the exact destination, Engine UID, protocol, ports, source address,
traffic class, IPv6 scope and lease mark. It binds the chosen device and checks
that route again. Main-table default interfaces are candidate/DNS ownership
hints; their absence does not establish that a family is offline. Unresolved
or unsupported routing results fail without falling back to the first NIC.
Physical DNS retains its existing ownership and scope requirements; policy-only
DNS discovery and rules requiring a not-yet-bound TCP source port are not
guaranteed by this implementation.

Foreign route and rule changes revoke socket permits and advance the durable
network generation, including changes of gateway or metric on the same NIC.
Failed snapshots retain last-good hints and suspend new permits. An Agent
restart requires verified Resume and revocation of old permits before admitting
new ones. Release and recovery clear both socket marks and Agent device bindings;
uncertain cleanup retains the descriptor and retires the mark until recovery.

While a Linux TUN session holds its nftables guard, the host's output policy
allows only loopback, the Usque interface, Agent-authorized Engine sockets,
link-local neighbor discovery, configured direct exclusions and DHCP renewal.
DHCP renewal (UDP 68 to 67 and 546 to 547) is allowed for root and for the
`systemd-network`, `dhcpcd` and `_dhcpcd` system users that exist when the
session starts. Other users cannot use this exception.

The guard also drops all forwarded IPv4 and IPv6 traffic for the whole
session, independent of the Kill Switch setting, so routed or bridged guests
cannot bypass it. Docker, Podman, libvirt and similar container or VM networks
lose connectivity while TUN mode is connected, including traffic between
containers on a bridge with `br_netfilter`. Use the local proxy outputs for
those workloads, or disconnect TUN mode.

The Agent checks the Engine's executable mappings for every request. If a
system upgrade replaces a library the Engine has already loaded, the running
session continues, because the Agent verified that file before the upgrade.
If the Agent restarts after such an upgrade, it cannot verify the replaced
file any more. Restart Usque before connecting TUN mode again.

## Recovery, updates and removal

If service startup fails, open **Background service** and use **Repair
service**. Authorization or package-manager lock failures can be retried. A
missing or incompatible deb/rpm installation requires reinstalling the same
verified package format with its package manager.

Linux network recovery has its own protected journal under `/var/lib/usque`.
It records only Linux resources and ownership, independent of Windows
HANDLE/SID/WFP state. Recovery attempts only owned changes. A failed restore
retains the journal and necessary protection; do not delete the journal,
flush nftables, reset all routes, or overwrite `/etc/resolv.conf` to clear an
error. An administrator should retain the error and install a compatible fix.

Linux changes between TUN policies, chain types, and split-DNS modes rebuild
the transport under the Agent's persistent replacement guard. These changes
can briefly reconnect; the existing guard remains until the new policy is
committed or an explicit disconnect completes recovery. Enabling TUN from a
local proxy also establishes a new transport whose physical sockets are
authorized by the Agent.

Updates match architecture and installation format. After confirmation, deb
and rpm are passed to apt/dnf only after signed-byte verification in root-owned
staging. AppImage replacement uses staging in the same directory and an atomic
rename, coordinating the matching service payload. Unwritable directories,
busy package managers and cancelled authorization preserve retryable input.
Version/protocol mismatches refuse to start a mismatched privileged session.
A signed release may advance the Agent protocol, so the installed helper still
verifies and installs it. A manifest with an older protocol is refused.

Remove deb/rpm through the package manager. AppImage service removal uses the
installed maintenance helper's `--remove-service` operation after administrator
authorization. Removal must complete network recovery before deleting service
files. Packages preserve personal data; **Clear all data** remains an explicit
current-user action. Never use the service maintenance commands merely to test
a workstation build.

## Builds and validation

[Contributing](../CONTRIBUTING.md#linux-desktop-and-packages) gives exact safe
checks. The [Linux package workflow](../.github/workflows/linux.yml) builds
natively on Ubuntu 22.04 for each architecture. The
[package helper](../tool/linux_package.py) rejects incorrect ELF architecture,
newer GLIBC/GLIBCXX requirements, nonbaseline x86-64 ISA requirements, missing
payloads and preview/debug content. AppImage tooling and its runtime are
[version and hash pinned](../tool/linux_build_dependencies.json); FUSE is not
needed for packaging. This follows AppImage's
[oldest-system build guidance](https://docs.appimage.org/reference/best-practices.html).

AppImage includes GTK's own schema database, static resources and fallback
icons, while retaining the desktop's MIME and theme search paths. Copied
distribution libraries and resources retain their copyright texts and referenced
licenses. Their source package versions and file mapping are recorded inside
the image at `usr/share/doc/usque/bundled-distribution-files.json`.

On a matching baseline build node, after the exact Rust and Flutter release
builds, the compile-only packaging command is:

```shell
python3 tool/linux_package.py --bundle apps/usque_gui/build/linux/x64/release/bundle --rust target/x86_64-unknown-linux-gnu/release --output target/linux-amd64 --architecture amd64 --version 0.3.2 --signing-key /absolute/path/to/ephemeral-test-key.pem --test-signing
```

Use the corresponding ARM64 paths on a native ARM64 node. Packages signed with
a local test key are never official; only the protected release workflow signs
with the official key.

| Validation | Evidence requirement |
| --- | --- |
| IPC peer/FD ownership, frame limits, stale operations and multi-user conflicts | Deterministic tests with temporary sockets and injected backends |
| Recovery ordering, corrupt journals, DNS/proxy external changes | Deterministic failure injection; native restoration still needs an isolated VM |
| Keyring cancellation, tray loss, service repair UX and signed updates | Unit/widget/native socket fixtures; desktop-session integration is separate |
| amd64 and arm64 × AppImage/deb/rpm | Native baseline builds and package inspections for both architectures |
| Real installation, upgrade, uninstall, crash, reboot, sleep and network changes | `usque-linux-snapshot-vm`, with snapshot and independent management channel |
| IPv4/IPv6/DNS/Kill Switch/DIRECT leak claims | Independent network observer and the exact candidate |
| Windows and Android regressions | Existing platform gates; Linux results cannot replace them |

Protected lab reports are optional and do not block publication. Public
workflows skip the private lab jobs and never publish raw captures.

## Windows feature mapping

The following map shows how each Windows feature is implemented on Linux and
what still needs native testing beyond the shared unit and widget tests.

| Windows user feature | Linux implementation | Additional native acceptance |
| --- | --- | --- |
| Accounts, registration and experimental Zero Trust | Shared Engine account flow; Secret Service credentials; desktop browser callback | Locked/missing keyring, browser activation, login autostart |
| SOCKS5, HTTP, H3/H2 and experimental L4 | Shared transport, routing, DNS and local listeners | Reachable IPv4/IPv6 endpoints and network transitions |
| TUN and Kill Switch | Agent-owned native TUN, descriptor transfer, policy routes and separate nftables table | Independent dual-stack/DNS/DIRECT observation; crash and reboot |
| DNS, Ads and split routing | Shared rule engine; resolved or NetworkManager system resolver ownership | Both DNS backends, external changes and restoration |
| OpenVPN, WireGuard, WARP WireGuard and VPN Gate | Shared negotiated chain runtimes and encrypted imported profiles | All final exit types, retry, MTU, failure protection |
| HTTP/SOCKS5 chain exits | Shared CONNECT and final DNS behavior | TCP/DNS behavior and denied/failed exit handling |
| System proxy | Current-user GNOME GSettings or KDE configuration and notifications | Both desktops, later user changes, crash restore |
| Diagnostics and network quality | Shared Engine diagnostics and event stream | Native source availability and sanitized exports |
| Tray, notifications and shortcuts | StatusNotifierItem and native GTK/GApplication bridge | Tray unavailable/disappearing; GNOME/KDE on X11/Wayland |
| Settings, files, clipboard and window state | Shared Flutter controls and native desktop bridge; XDG paths | File cancellation, secret clipboard expiry, Wayland placement |
| Startup and automatic connection | Current-user login entry; Engine IPC starts immediately; auto-connect needs usable credentials | Login with locked keyring and duplicate activation |
| Updates and uninstall | Fixed-key manifests, privileged maintenance, apt/dnf or atomic AppImage replacement | All six packages, cancellation, locks, interrupted upgrade and recovery |

The v0.3.2 readiness review predates Linux support and does not cover it.
