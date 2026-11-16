# macOS TUN Agent

The ordinary-user Engine + root LaunchDaemon Agent + Darwin utun implementation
is integrated on `feat/macos`. It targets macOS 13+, arm64 and x86_64, using
ad-hoc signatures and manual user trust. It requires administrator authorization
for installation, replacement and removal, without an Apple Developer
subscription or disabling SIP. Account credentials remain in the user's Keychain.

**Production TUN is release-gated.** Manual hosted tests exercise selected
network failures on disposable Macs. Record the runner OS, architecture and
individual assertions when updating the acceptance matrix. To build the opt-in TUN test DMGs, run
`python3 tool/macos_package.py build --arch arm64 --test-tun` or use
`--arch x86_64`. Packages are written under `build/macos-tun-test/<arch>/`
with a `-tun-test.dmg` suffix. Install or update the bundled background service,
then quit and reopen Usque. A matching Engine and Agent, network preflight and
clean recovery state enable the TUN switch in Home and Proxy. Both helpers embed
the test flag at compile time. Ordinary builds omit the opt-in.

Both Engine and Agent retain `RELEASE_APPROVED = false`. Ordinary
proxy mode and service inspection remain available. The independent
[Network Extension experiment](MACOS_NETWORK_PROTOTYPE.md) is preserved and is
not built or installed by the default Agent package.

## Implemented components

| Component | Behavior |
| --- | --- |
| Agent authentication | Kernel audit token, UID/PID incarnation, live hardened code validity, exact root-installed Engine SHA-256/CDHash, version/protocol/architecture checks; revalidated per request |
| IPC | Append-only typed protobuf operations, bounded Unix frames, one owned SCM_RIGHTS descriptor, whole-frame timeout and sequence fencing; Darwin receive/child creation serialization |
| Native resources | Root-created utun, exact bound physical TCP/UDP sockets, generated PF anchor rules, SystemConfiguration DNS and owned capture routes |
| Durable recovery | Separate transaction/native receipts, root ownership/ACL checks, atomic full-sync writes, boot identity, compare-before-restore, initialization/replacement/deletion intents |
| Data plane | One shared `DataPlaneRuntime` for TUN and HTTP/SOCKS, CONNECT-IP/L4, H2/H3, 1–16 MASQUE connections, shared split rules, DNS/DoH/DoT and chain exits |
| Packet bridge | Validated four-byte Darwin family header, bounded upload/download, cancellation during backpressure, failure-triggered physical-egress revocation |
| Network epochs | Addresses, gateways, service UUIDs, address aliases and prefixes participate in the observed generation; old socket leases cannot enter a successor session |
| Service lifecycle | Root LaunchDaemon, authenticated readiness, administrator install/repair/update/recover/uninstall, staged hash validation, rollback and committed-cleanup retry |
| Interface | Settings service/version/recovery controls, retry after cancellation, common capability gating and recovery/error projection, independent macOS golden fixtures |
| Delivery | Separate native GUI/Engine/Agent/maintenance bundles and DMGs for Apple Silicon and Intel, deployment/architecture checks, hardened ad-hoc Engine signing, manifests and SHA-256 inventories |

## Connection and recovery contract

The Agent durably records each intent before changing the system and reads back
actual state before acknowledging it. Startup establishes protection before
physical egress, remote connection, utun configuration, DNS and routes. The
packet pump is attached before Commit and final frontend admission.

Physical sockets are created by root, bound to the selected interface/source,
and connected to the exact authorized peer before descriptor handoff. PF permits
only their root-origin tuples, with no PF states. The Agent retains originals
until permit removal has been persisted and read back. Successful shutdown is
idempotent; a first `ENOTCONN` never proves revocation. Cleanup still attempts
PF removal after shutdown errors and retains descriptors if removal is uncertain.
Connected Darwin UDP uses peer-preserving sends; TCP handoff completes the
already initiated connection instead of reconnecting it.

Kill Switch retains the guard after Engine/IPC loss. Reconnection by the same
user transfers the session through a persisted successor fence while keeping
protection. Another user cannot take over. GUI exit, Engine exit and explicit
Disconnect have distinct ownership paths. Explicit Disconnect also handles
failed startup with no published runtime, a successor committed during failed
Resume, and a lost successful Disconnect response. A restarted Engine projects
pending or unobservable recovery as an error, never a clean disconnected state.

Explicit recovery stops the data plane, revokes egress, restores owned routes
and DNS, closes utun, then removes Usque's PF anchor/reference last. Kill Switch
Active requires authenticated, actual protection readback; failure to verify
it produces an error. Corrupt evidence, mismatched ownership, unknown PF enable
references or cleanup failure remain “recovery incomplete.” No automatic timer
releases protection. No protection is promised before Agent readiness at boot.

## Supported PF/network preflight

The initial supported PF environment is intentionally strict: the standard
Apple `com.apple/*` hook (including its standard scrub hook), no competing
filter/NAT rules or nested foreign anchors, no skipped interfaces and no existing
PF states. A conflicting configuration is refused with a typed error. Usque
never flushes global rules/states or disables PF owned by other software.
Compatibility with the system firewall still requires external acceptance.

Fixed DHCP/IPv6 neighbor discovery and necessary ICMP infrastructure traffic
are distinct from user traffic. Unused address families remain blocked. Default
capture routes and more-specific physical LAN prefix routes keep domain/IP/
country/ad decisions in shared rules; `allow_lan` never installs a broad bypass.
The snapshot includes GUA, ULA and link-local aliases on the selected interface,
and scoped direct sockets select an on-link source. Multiple physical services
with different primary IPv4/IPv6 interfaces are explicitly unsupported by
preflight. Additional non-primary interfaces, scoped/cloned routes and unusual
router configurations require real acceptance before widening the supported set.

DNS uses SystemConfiguration preferences plus applied-state verification,
without editing `/etc/resolv.conf`. Original physical resolver evidence is kept
for controlled bootstrap; a retained session uses persisted numeric control
candidates rather than recursively invoking system DNS.

## System proxy and mihomo comparison

| macOS operation | FlClash / mihomo source | Usque implementation |
| --- | --- | --- |
| System proxy | FlClash enumerates enabled services and calls Apple's `networksetup` for HTTP, HTTPS, SOCKS and bypass domains. Stop disables proxies and clears the bypass list. [Proxy commands](https://github.com/chen08209/FlClash/blob/be84736605527034689c95bf27828ff27d912c5e/plugins/proxy/lib/src/macos_proxy.dart) | The Engine sends a loopback IP, port and lease ID to the installed Agent. The Agent changes HTTP/HTTPS on eligible physical services and temporarily disables PAC/WPAD. The SOCKS configuration and bypass list retain their native values. |
| Authorization | The proxy command runner starts the system executable directly. FlClash's separate TUN setup authorizes a root-owned core executable, restricted to the admin group. [Command runner](https://github.com/chen08209/FlClash/blob/be84736605527034689c95bf27828ff27d912c5e/plugins/proxy/lib/src/proxy_command.dart) · [Core authorization](https://github.com/chen08209/FlClash/blob/be84736605527034689c95bf27828ff27d912c5e/lib/common/system.dart) | Installation and updates authorize a root LaunchDaemon. Its per-request audit token and code identity checks also cover proxy leases. Normal connections use SystemConfiguration without creating an Authorization Services handle. |
| TUN and egress | Upstream mihomo's sing-tun listener manages the device, routing and interface monitoring. Its Darwin dialer binds outbound sockets with `IP_BOUND_IF` and `IPV6_BOUND_IF`. [Listener](https://github.com/MetaCubeX/mihomo/blob/184857ce21a3e4ef48c6829265c00aedfd87792b/listener/sing_tun/server.go) · [Darwin dialer](https://github.com/MetaCubeX/mihomo/blob/184857ce21a3e4ef48c6829265c00aedfd87792b/component/dialer/bind_darwin.go) | The root Agent owns utun, generation-scoped physical sockets, DNS, routes and PF. An ordinary-user Engine receives descriptors and runs the shared data plane. Public TUN remains gated on observed network behavior. |

A proxy lease survives a quiet IPC stream. Engine exit closes the stream and
the Agent attempts restoration. The Agent reloads its receipt after restart;
an Engine observing a closed stream reports the lost proxy. Each receipt is
stored in a root-only file with mode 0600 before preference writes. It records
owned scalar values and PAC fingerprints. The Engine's old encrypted journal
stays in its user's profile. No account secret is exchanged. An interrupted
write, unreadable receipt or unsuccessful restoration prevents a clean
acknowledgement. Successful uninstallation publishes a root-owned cleanup receipt
for an Engine whose proxy stream was lost. The next Agent invalidates that
receipt before accepting connections. A competing HTTP/HTTPS endpoint is preserved, including when
restoring PAC/WPAD would supersede that endpoint.

**Testing on a Mac with limited storage.** Tests exercise the production packet
pump over a Unix datagram pair and use real IPv4/IPv6 TCP and UDP peers on
`lo0` for Darwin interface binding. The optional host probe below only reads
SystemConfiguration services and constructs a proxy plan in memory. No VM or
system TUN is needed for these checks. The [local validation record](MACOS_PROXY_AGENT_VALIDATION.json) lists the exercised cases, remaining device tests and package identities.

```sh
cargo test --offline --locked -p usque-agent macos::
cargo test --offline --locked -p usque-engine macos_
cargo test --offline --locked -p usque-ipc darwin_fd
cargo test --offline --locked -p usque-platform reads_current_services_and_constructs -- --ignored
```

The next real-network run can use a contributor's disposable Mac or a remote
Mac with console recovery. The ignored test
`macos::utun::tests::disposable_mac_real_utun_descriptor_handoff` creates
one unconfigured utun, passes its descriptor using the production IPC code and
checks its lifetime after the last close. To run this test on separate GitHub-hosted
Apple Silicon and Intel runners, dispatch the Build workflow:

```sh
gh workflow run build.yml --ref feat/macos -f macos_utun=true
```

Both **Real utun** jobs must show the named test passing. The socket pair used
for this test lives in one process. To run the endpoint-failure and recovery
scenarios on both architectures with a separate Linux observer:

```sh
gh workflow run build.yml --ref feat/macos -f macos_tun_e2e=true
```

The first scenario installs the Agent, provisions synthetic credentials in a
temporary keychain and connects a separate ordinary-user Engine to an
closed test endpoint. It checks one-shot failure, continuous retries, the
disconnect response used by the GUI, the installed Agent's cleanup status and
normal Engine exit. A second Engine scenario uses a pinned test HTTP/2
CONNECT-IP server. A UDP datagram carrying a run-specific nonce must make a
round trip through utun before the server is stopped to exercise connection
loss and explicit disconnect. The native Agent scenario drives its transaction
past configuration and reads back the effective DNS,
per-service resolver preferences, IPv4 and IPv6 routes and utun lifetime.
The HTTPS receiver runs on a separate Ubuntu runner. Its report must contain
positive controls before protection and after disconnect for each Mac, with no
protected marker arriving while the guard is active or retained. The probes
bind to the original physical interface and reuse the pre-resolved destination.
After cleanup, a separate request resolves the receiver by name and uses the
default route. On each Mac a paired Ethernet interface also captures synthetic
IPv4 and IPv6 TCP, UDP and DNS probes on the receiving link. Each protocol
must be visible before and after protection, with zero matching frames during
both protected phases. The workflow saves the filtered captures and counts.
The public tunnel forwards only synthetic marker requests to the temporary
receiver. The jobs use no WARP registration or personal account credentials.

For a disposable Mac with console access, compile
`cargo test --locked --offline -p usque-agent --lib --no-run` as an ordinary
user. Run the resulting trusted test binary as administrator with
`USQUE_DISPOSABLE_MAC=1` and `--ignored --exact` plus the full test name.
This stage requires no change to release gates. For end-to-end acceptance,
capture on its test gateway as well as at a
controlled destination. Compare direct and proxied IPv4/IPv6 TCP/UDP probes,
then interrupt the Engine and Agent, check for unexpected physical packets and
verify DNS, routes, PF and proxy restoration. Record the OS, architecture and
package hash for each run. A loopback test cannot observe physical egress.

## Installation and administrator recovery

Build the matching architecture using [packaging instructions](MACOS_DEVELOPMENT_PACKAGING.md),
copy the manually trusted app to Applications, then use **Settings → macOS
background service**. Install/update invokes system administrator authorization.
Installing the service from Settings automatically reopens Usque with the pinned Engine after cleanup. After a repair or replacement, reopen Usque to use the updated service.
Daily connections use the installed service without another authorization prompt.
Service mutations are disabled while the GUI owns an active connection.

If a previous development build cannot quit after installing the service,
the administrator command may have created the installation directory and
Engine with mode `0700`. The GUI then cannot confirm the service's cleanup.
Use the corrected package's **Repair / update** operation. If the old window
is still open and prevents replacing the app, an administrator can uninstall
the old service with the existing installed app's maintenance tool:

```sh
sudo /Applications/Usque.app/Contents/Helpers/usque-agent-maintenance uninstall
```

Wait for a successful result, retry **Disconnect and Exit**, replace the app,
and reinstall the service from Settings. Uninstall first recovers Agent-owned
network settings and preserves user accounts. If recovery fails, keep the
service and recovery records for diagnosis. An inaccessible pre-existing
`/Library/PrivilegedHelperTools` parent requires administrator inspection;
repair does not change permissions on existing system directories. Production
TUN stays disabled in these development packages after service installation.

Root-owned locations:

- `/Library/PrivilegedHelperTools/com.usque.agent/`: Agent, pinned Engine, install manifest and control socket.
- `/Library/Application Support/Usque/Agent/`: recovery and maintenance records, locks.
- `/Library/LaunchDaemons/com.usque.agent.plist`: LaunchDaemon definition.

A GUI-independent recovery entry point is included in the installed app:

```sh
sudo /Applications/Usque.app/Contents/Helpers/usque-agent-maintenance recover
```

The maintenance tool stops the daemon, runs the Agent's root-only
`--recover-state`, verifies cleanup and restarts the service. If the installed
Agent cannot recover its journal, open the updated app and choose **Settings →
macOS background service → Repair / update**. Repair can run the verified
staged Agent against the existing records before it activates the update. The same fixed
entry point accepts `install`, `repair` and `uninstall`; all require explicit
administrator execution. It accepts no arbitrary command, network rules or
recovery path. Uninstall first requires successful network recovery and retains
user profiles/Keychain and clean recovery evidence. Never delete recovery records
or globally flush PF to make an error disappear; preserve evidence for diagnosis.
The root-only `usque-agent --validate-only` and `--readiness` commands provide
non-mutating installation checks.

## Validation and release gate

The [source-bound validation record](MACOS_TUN_AGENT_VALIDATION.json) records
exact checks and artifact hashes. Isolated coverage includes FD ownership and
identity forgery, partial frames, packet validation/backpressure/cancellation,
connected socket revocation, stale owners and epochs, write failures, interrupted
handoff/initialization, PF ownership/reference reuse, boot changes, DNS value
preservation, installer ordering/rollback and service UI/goldens. Generated IPv4/
IPv6 PF rules are checked with the installed parser's `-n` mode only.

Run safe checks after loading the pinned toolchain:

```sh
source .toolchains/env.sh
cargo test --workspace --all-targets --locked --offline -- --test-threads=4
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
MACOSX_DEPLOYMENT_TARGET=13.0 cargo check --workspace --all-targets --target x86_64-apple-darwin --locked --offline
python3 tool/test_macos_package.py
```

Real network cases remain `not_run` for arm64/x86_64 × macOS 13/current:
IPv4/IPv6 TCP/UDP, actual utun/FD handoff, bound egress without loops, same-IP
domain decisions, DNS/DoH/DoT, automatic endpoints, chains, TUN plus proxies,
old physical states, firewall coexistence, GUI/Engine/Agent kills, sleep/wake,
network and same-address gateway changes, multi-user ownership, live installation,
upgrade rollback, uninstall, and damaged journal/admin recovery. Source tests,
parser checks, ad-hoc signing and cross compilation do not replace these tests.
Windows/Linux exact golden comparison remains for those hosts; only the native
arm64 macOS baseline is recorded here. Intel runtime/golden acceptance is `not_run`.

Release review must verify the matrix using an independent observer and
reversible test systems before opening both release constants. There is no
ordinary-user runtime flag to bypass the production gate and no unattended root
Agent auto-update in this version.

Primary implementation references: [Apple utun](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/net/if_utun.c),
[Darwin socket shutdown](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/uipc_socket.c),
[UDP behavior](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/netinet/udp_usrreq.c).
