# Network architecture prototype

This source builds four independent provider executables (Packet Tunnel,
Transparent Proxy, DNS Proxy and content filter) and a native control host.
The filter's socket-metadata provider forwards audit-token-bound flow metadata
to its packet guard over authenticated XPC; it does not assume shared process
memory. Shared Rust supplies every domain/IP routing verdict.

Production capability is always false. Ad-hoc builds intentionally omit
restricted Apple entitlements and cannot install/activate the providers.
They do not change routes, DNS, network preferences or the user Keychain when
built or queried for status. No PF implementation is present.

## Development builds

From the repository root, with the pinned toolchain available:

```sh
source .toolchains/env.sh
bash tool/build_macos_network_runtime.sh arm64
bash tool/build_macos_network_prototype.sh --arch arm64
bash tool/build_macos_network_runtime.sh x86_64
bash tool/build_macos_network_prototype.sh --arch x86_64
bash tool/test_macos_network_prototype.sh
```

The two app bundles are under `build/macos-network-prototype/<architecture>/`.
These are developer experiment hosts, not the final Flutter application or
DMG installers. Each app contains five Mach-O executables with the requested
architecture and macOS 13 deployment target. Fatal linker warnings prevent
silently linking C/C++ objects built for a newer minimum macOS version.

`--typecheck --arch arm64|x86_64` checks the native sources without building the
Rust archive or installing anything. The isolated native contracts can also be
run through `tool/test_macos_native_contracts.sh`.

## Authenticated control

The host accepts only `--stdio`. Messages and replies have a four-byte
big-endian length prefix and a 2 MiB limit. Secrets belong only in these frames
and authenticated XPC, never in arguments, environment, preferences or files.
Request acceptance and actual connection are separate states. Credential
callbacks can travel in the opposite direction while a request is in flight;
the Engine acknowledges the exact request digest after persistence completes.

Both the kernel's XPC peer signing requirement and each message's Security
framework audit identity constrain exact bundle identifiers to the same Team.
The required XPC API is available from macOS 12, below this project's minimum.
Ad-hoc identity has no Team ID and is rejected.

## Signed experiment, pending resources

An explicitly compiled `--signed-poc` uses separate `.poc` identifiers. A later
Developer ID build can use `--identity` and `USQUE_MACOS_TEAM_ID`, with matching
Apple provisioning profiles installed externally. This switch permits the
experiment to run without falsely declaring production acceptance. Nothing in
this task requests a certificate, creates profiles or enables system extension
developer mode.

Before starting the experiment, use the explicit framed activation action and
complete system approval. A Start/Protect request cannot bypass that approval.
The experiment currently requires **numeric Custom MASQUE endpoints**, and
encrypted DIRECT DNS requires explicit numeric bootstrap addresses. Automatic
physical endpoint discovery fails with `physical_endpoint_dns_unverified`.
These are explicit prototype limits, not final feature reductions.

The independent filter owns the authoritative physical generation. Its native
snapshot reports that value; helper-local generation before the filter starts
is labeled `uninitialized`. After a path change, callers must obtain a fresh
filter generation and perform a protected Replace. The experiment must verify
metadata ordering and `filterFlowIdentifier` correlation on the target OS.

Exact-tuple physical socket leases are bounded, owned by a persistent provider
XPC connection, renewed while that provider is alive and revoked on peer loss,
network change or protection reset. The control-plane allowlist is limited to
bounded DHCP/IPv6 neighbor discovery fixtures. IPv6 extension/fragment handling,
control-plane coverage and lease/source identity under crashes remain real
device acceptance cases. Kernel filter crash behavior has not been established.

Most critically, `IP_BOUND_IF`/`requiredInterface` is only an experiment under
`enforceRoutes`. An external capture must prove DIRECT actually exits on the
physical interface while PROXY uses the tunnel. Routing through the wrong path
is a failed experiment, not a successful fallback.

See the [milestone and acceptance record](../../../../docs/MACOS_NETWORK_PROTOTYPE.md)
for the remaining signed tests. Do not publish this as a full-feature VPN.
