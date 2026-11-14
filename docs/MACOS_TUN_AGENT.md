# macOS TUN Agent

The ordinary-user Engine + root LaunchDaemon Agent + Darwin utun implementation
is integrated on `feat/macos`. It targets macOS 13+, arm64 and x86_64, using
ad-hoc signatures and manual user trust. It requires administrator authorization
for installation, replacement and removal, without an Apple Developer
subscription or disabling SIP. Account credentials remain in the user's Keychain.

**Production TUN is release-gated.** The implementation and isolated checks do
not establish real network safety. The user has no disposable macOS environment
or external observer; all unavailable real-system acceptance is explicitly
`not_run`. Both Engine and Agent retain `RELEASE_APPROVED = false`. Ordinary
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

## Installation and administrator recovery

Build the matching architecture using [packaging instructions](MACOS_DEVELOPMENT_PACKAGING.md),
copy the manually trusted app to Applications, then use **Settings → macOS
background service**. Install/update invokes system administrator authorization.
Restart the GUI after installation/replacement so it launches the pinned Engine.
Daily connections use the installed service without another authorization prompt.
Service mutations are disabled while the GUI owns an active connection.

Root-owned locations:

- `/Library/PrivilegedHelperTools/com.usque.agent/`: Agent, pinned Engine, install manifest and control socket.
- `/Library/Application Support/Usque/Agent/`: recovery and maintenance records, locks.
- `/Library/LaunchDaemons/com.usque.agent.plist`: LaunchDaemon definition.

A GUI-independent recovery entry point is included in the installed app:

```sh
sudo /Applications/Usque.app/Contents/Helpers/usque-agent-maintenance recover
```

The maintenance tool stops the daemon, runs the Agent's root-only
`--recover-state`, verifies cleanup and restarts the service. The same fixed
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
