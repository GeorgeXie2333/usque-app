# macOS Network Extension prototype

This is the first milestone of the macOS port, based on clean `dev`
`948de61c25800d6c5753f6c4bd2b2db35cfffa2e`. The previous implementation at
`dcf332a142524f8d7ba58b95655f2b9eccbb1131` remains a review-only backup; it was
not restored. The public macOS VPN capability stays disabled.

## Ownership and boundaries

| Component | Responsibility |
| --- | --- |
| Ordinary Engine | Profiles, accounts, user Keychain, durable credential updates, HTTP/SOCKS listeners |
| Packet Tunnel process | Sole `DataPlaneRuntime`, MASQUE, chained exit, reconnect, remote VPN packets |
| Transparent Proxy process | Per-flow DIRECT selection and the explicitly unvalidated physical-egress experiment |
| DNS Proxy process | DNS policy and upstream resolution; encrypted-upstream errors never request plaintext fallback |
| Independent filter process | Block/allow protection and authenticated exact-flow metadata |
| Shared Rust policy | Existing rule order and version snapshot, reused through the C ABI |

In VPN mode, the Engine uses `SystemProxyRuntime`, which only creates ordinary
OS TCP/UDP sockets. It has no API for constructing a MASQUE or chained session.
The socket frontend preserves the requested hostname and registers the local
and remote tuple before sending a SYN/datagram. A missing native authorizer
denies the flow. Hostnames are never inferred from a global IP-to-domain cache.
The provider's confirmed health and a local activation gate are both required;
an epoch change terminates established streams and invalidates UDP associations.

Configuration snapshots hash the profile and the country/advertising artifacts.
Every provider verifies the digest before and after loading. A cache update
requires an explicit replacement; providers retain parsed GEO/advertising rules
without independently following mutable files. The combined hostname/IP FFI
uses the existing Rust precedence, including CIDR rejection of a named flow.

In proxy-only mode, the existing Engine runtime remains the owner. A transition
to VPN first confirms independent protection, then stops that old runtime.
Admission of a Start request does not report a successful connection.

Packet Tunnel, proxy, DNS and filtering must remain separate provider processes.
The packet tunnel must never implement local DIRECT fallback or filtering.
These boundaries follow [Apple TN3120](https://developer.apple.com/documentation/technotes/tn3120-expected-use-cases-for-network-extension-packet-tunnel-providers).

## Control and secrets

The additive, versioned control envelope carries a session UUID, configuration
revision, physical-network generation and operation UUID. Protect, Start,
Replace, Stop, Snapshot and Cancel have bounded admission, stale-request
validation and an operation replay journal. Existing Engine IPC message numbers
are unchanged. Reusing an operation UUID with another payload is an error.

Native control verifies a fixed signing identifier and the app's Team ID from
kernel-supplied XPC audit identity. Ad-hoc builds cannot satisfy that production
identity requirement. The bundled control helper uses bounded, four-byte
big-endian length-prefixed frames; secret data never appears in arguments,
environment variables, provider preferences, logs, or temporary files.

Dedicated credential DTOs preserve the full WARP/Zero Trust identity, imported
chain credentials and Proton material. Persistence callbacks are reconstructed
on the provider side and call back into the Engine. A failed, stale or missing
durable-write acknowledgement fails the credential handoff. Providers never
read the user's Keychain.

Start order is protection, DNS/flow policy, tunnel, then allow. Explicit Stop
blocks first and removes protection last. Loss of the Engine/control peer is
not an implicit Stop. Provider restart begins blocked without credentials and
waits for a new authenticated handoff.

## Running isolated validation

With the repository's Rust toolchain available:

```sh
source .toolchains/env.sh
bash tool/test_macos_network_prototype.sh
```

These tests use synthetic provider adapters and OS-assigned loopback sockets.
They do not install or activate providers, change system routes/DNS, contact
external VPN servers, or use real account credentials. Restricted runners must
allow local loopback sockets. The native component build and its contract checks
are documented alongside the providers under
`apps/usque_gui/macos/NetworkExtensions`.

## Acceptance record

| Area | Implemented | Isolated/build evidence | Signed real-device evidence |
| --- | --- | --- | --- |
| Versioned lifecycle and replay/cancellation/backpressure | Prototype | See validation log below | Pending |
| Sole owner and Engine-to-provider adapter | Prototype | See validation log below | Pending |
| HTTP/SOCKS TCP and UDP, health/epoch gating | Prototype | Loopback tests | Pending kernel-route capture |
| Shared rule semantics | Prototype | Shared Rust policy tests | Pending same-IP domain split |
| Independent protection and DNS/provider orchestration | Prototype | Native compile/contract checks | Pending activation and fault injection |
| Credential refresh persistence acknowledgement | Prototype | Broker contract tests | Pending signed XPC and live renewal |
| Apple Silicon, current macOS | Development host | Local build/test only | Pending |
| Intel native runtime and macOS 13 | Target support | No hardware claim | Pending |
| Desktop integration, CEF, dual DMG, update/recovery | Later milestone | Not part of this prototype | Pending |

Build success, isolated correctness and signed real-network correctness are
separate results. No result here establishes leak prevention on the host.

### Local validation log

Development host: Apple Silicon (`arm64`), macOS 27.0, Xcode 27.1. Deployment
target: macOS 13.0. Validation date: 2026-10-29.
Full-workspace code candidate: `14b17a8c5e6617134a084353e4120c5e25699357`.
Test-only diagnostic suppression: `0789dafacd912bc5049addf8708d333092eb3754`,
followed by a successful rerun of the five Engine provider tests. The subsequent
validation-record commit changes documentation only.
The [machine-readable record](MACOS_NETWORK_VALIDATION.json) separates these
checks from hardware and real-network acceptance.

- Rust lifecycle, credential RPC, actual shared-runtime refusal boundary and
  immutable policy tests: 15 passed.
- OS-socket frontend tests: 5 passed, including IPv6 TCP/UDP loopback.
- Engine-to-actual-controller integration: 5 passed. HTTP/SOCKS share one
  synthetic backend owner; accepted/pending state, replacement, detach and
  durable-write failure are covered.
- Native isolated contracts: 3 groups passed (lifecycle fencing; bounded
  framing; control-plane parsing).
- Full macOS workspace regression: 1,910 passed, 3 ignored, no failures.
- `cargo fmt --all --check`, strict workspace Clippy and the complete
  `tool/check_source.ps1` checks passed (Rust, Dart/Flutter analysis, Kotlin,
  Python, PowerShell and protobuf). Loopback permission was required for
  Gradle's local lock service.
- arm64 and x86_64 Rust static libraries and five native executables per app
  cross-built locally; architecture, Mach-O minimum OS and development signatures
  checked. Intel binaries were not run on Intel hardware.

Reproduce with `tool/test_macos_network_prototype.sh`, then
`tool/build_macos_network_runtime.sh <architecture>` and
`tool/build_macos_network_prototype.sh --arch <architecture>`.

The first aggregate run exposed a pending/Stopped snapshot race and an active
Protect rejection during replacement; both were fixed and the Engine tests
re-run. Initial linking also exposed cached C/C++ objects built for macOS 27;
the architecture-specific runtime build now fixes their target to 13 and treats
native linker warnings as errors. Sandbox loopback denial is environmental;
the actual loopback runs used permission to bind local sockets only.
The full regression also found an IPC error-code compatibility change and two
Darwin-specific test fixture assumptions. The original error code was restored;
fixtures now use real IPv4/IPv6 loopback addresses and a sufficiently sized test
UDP socket, without changing interface aliases or host network settings. Engine
unit tests use an in-memory vault on macOS. Provider contract tests also suppress
the ordinary exit-location diagnostic, so its hostname resolution cannot escape
the isolated loopback boundary.

Repository policy passed on the tracked-source archive. Running the validator
directly over this development checkout also scans ignored local SDK docs and
reports their missing links; those SDK files are not part of the candidate.

No extensions were installed/activated. No system VPN, external tunnel, DNS
leak, physical-egress crash test, sleep/network transition or Intel/macOS 13
hardware test has run. Numeric Custom endpoint bootstrap is a prototype limit;
authenticated physical endpoint discovery is still required for automatic mode.

### Control and recovery follow-up

The prototype was squash-merged into `main` by
[PR #1](https://github.com/JustinL330/usque-app/pull/1), commit
`39f31f2d7e49526b7e29b756cdfef27930b16441`. The following implementation and
isolated checks use code candidate `8c4191499dd8d76df111377f819ce2688a6e682a`.
They supplement the historical record above.

- Snapshot remains available when the 4,096-entry Rust mutation journal is
  full. A separate bounded Stop lane cancels pending work and retains one
  retryable terminal operation; queued starts cannot reopen a stopped session.
  Retired session UUIDs cannot start again. The journal fails closed instead
  of evicting accepted mutation identities; Stop and observations still work.
- Engine reserves a session before protected preflight, including generation
  zero while initialization is incomplete. A failed or cancelled preflight
  retains this identity for Stop or retry. Only a satisfied observation from
  the independent filter supplies the forwarding generation; helper-local
  counters never substitute for it. Permission/version failures keep stable
  error codes.
- Native teardown handles protection-only startup and roles still holding an
  older revision of the same session. It refuses newer or foreign sessions,
  waits for packet/transparent manager disconnection, and removes filtering
  last. Its 60-second budget fits inside the Engine's 90-second Stop deadline.
  A stopped packet runtime continues reporting independent protection until
  coordinator cleanup completes. Stopped DNS/flow providers reject new flows.
- Synchronous native configuration and operation acknowledgement are serialized.
  Failed work can actually retry with the same operation UUID; successful
  duplicates do not reapply the configuration.
- Physical observations normalize interface/address order and track used
  physical interfaces and their numeric IPv4/IPv6 addresses. Aggregate VPN IP
  capability flags do not change the physical fence. Missing usable observations
  deny permits. Guard commands and outgoing packet decisions check the current
  generation, including the interval before the change callback clears leases.

Validation on the same arm64 development host:

- Full Rust workspace: **1,919 passed, 3 ignored, 0 failed**. The prototype's
  Rust coverage is now 34 tests (19 runtime, 10 Engine, 5 OS-socket frontends).
- Native isolated contracts: **7 groups passed**, including configuration
  retry, partial-stop fencing and deterministic network observations.
- Format, strict workspace Clippy and aggregate source checks passed.
- Both architecture-specific Rust libraries and development apps rebuilt;
  each app has five matching Mach-O executables targeting macOS 13, with
  ad-hoc signatures verified. The arm64 helper's framed status and unsigned
  preflight rejection passed. Intel execution remains untested.

These native lifecycle results are contract/build evidence, not tests of real
system-manager teardown. A missing DNS/control peer still leaves protection
in place rather than declaring recovery complete. Filter-authority restart
incarnation fencing and same-address gateway/SSID changes need further work;
address observations alone do not identify every physical-network transition.
Signed activation, XPC acceptance, kernel routing and physical packet captures
remain `not_run`. Production capability remains disabled.

## Required signed experiment

The prototype has a separate signed experiment entry point. Production remains
closed while the experiment is unvalidated; validation is not required merely
to run the signed experiment. It must record external packet capture, provider
statuses, network generations, approval/cancellation and crash outcomes.

Apple documents that `enforceRoutes` supersedes app interface scoping. Binding
a socket to a physical interface therefore cannot establish DIRECT bypass.
The trial must actually demonstrate the resulting egress path; failure blocks
the milestone and requires an architecture change. There is no PF fallback.
See [Apple VPN routing](https://developer.apple.com/documentation/networkextension/routing-your-vpn-network-traffic).

Required cases, on IPv4 and IPv6, include:

- Same remote IP with separate domain names routed DIRECT, PROXY and REJECT.
- System DNS versus tunnel DNS, no recursive resolution, and encrypted DNS
  failure with no plaintext fallback.
- HTTP/SOCKS and TUN traffic sharing the one provider-owned remote session.
- Independent termination of Engine, Packet Tunnel, Transparent Proxy, DNS
  Proxy and filter; provider restart with absent credentials must stay blocked.
- Sleep/wake, interface changes and reconnect, with no unauthorized physical
  egress during transitions.
- Permission cancellation, extension-version mismatch and recovery failure,
  each identifiable and retryable.

Only after this experiment passes should the remaining desktop, CEF and
packaging stages proceed. The label **macOS full-feature version** additionally
requires complete Apple Silicon/Intel and macOS 13/current-system acceptance.
