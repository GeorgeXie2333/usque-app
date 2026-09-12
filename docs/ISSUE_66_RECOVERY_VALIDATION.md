# Issue 66 recovery handoff follow-up

Local source validation on 2026-09-13 (Asia/Singapore), on
`codex/vpngate-exit`, starting at `11aca08`. The existing VPN Gate and offline
flag changes were retained. This record covers the three follow-up commits;
it is not a protected-runner report or proof of the original live failure's
complete cause.

## Changes

1. `6670d00`: cancellation covers selected event handlers and backpressured
   transport failure reporting. Native stop precedes transport joins, wakes
   pending input writers, and bounds the native join to five seconds. A timed
   out or cancelled join retains worker ownership instead of claiming exit.
2. `2bf8582`: cancellation of headless Prepared startup drops producer guards
   before releasing its Agent pipe. Explicit disconnect closes forwarding
   before releasing startup/active leases. Finalization observes cancellation;
   ordinary failures retain in-place retry. Agent ownership/epoch checks remain
   unchanged and reject stale recovery of a later transaction.
3. The commit containing this record adds ephemeral Engine log correlation,
   typed lifecycle stages and journal-generation correlation. It also bounds
   the desktop Tokio runtime shutdown wait after service cleanup and makes the
   UI process monitor cancellable. The native thread is never treated as
   forcibly cancelled or safe to free merely because a timeout elapsed.

## Validation

Commands ran from the repository root using Rust 1.97.1 and the checked-in
Windows native environment helper. Cargo build, test and Clippy actions used
`--locked`. Python commands used the verified bundled Python executable where
`python` was not on PATH.

| Gate | Exact command | Result |
| --- | --- | --- |
| Rust format | `cargo fmt --all --check` | Passed |
| Windows workspace Clippy | `./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed |
| Windows workspace tests | `./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | 1022 passed; 3 credential-dependent live tests ignored |
| Native interoperability Clippy | `cargo clippy -p usque-openvpn --all-targets --features interop-test --locked -- -D warnings` | Passed in the helper-initialized environment |
| Native interoperability tests | `cargo test -p usque-openvpn --features interop-test --locked` | 10 passed; memory TLS/CBC peer, no OS TUN |
| Android Rust Clippy | `./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed with pinned NDK/CMake; no JNI copying or APK execution |
| Native source lock | `python tool/check_openvpn_sources.py` | Passed; 5 source/notice locks |
| Repository policy | `python tool/check_repository_policy.py` | Passed |
| Whitespace | `git diff --check` | Passed |
| Final Engine test repeat | `./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test -Package usque-engine` | 172 passed; 3 live tests ignored |
| Final Engine Clippy repeat | `./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy -Package usque-engine` | Passed |
| Windows release compile | `./tool/build_windows_rust_release.ps1 -Variant x64-v2` | Passed; compile-only, no installation |

The full workspace gates preceded a final diagnostic tightening: an empty Agent
reply must not be logged as success. The affected Engine tests and Clippy, and
the release build, were repeated for that change. The native wrapper and
Android-shared transport code were unchanged after their successful checks.

The first Engine test run caught a Windows test-thread stack overflow caused
by retaining a large startup future inline. The existing boxed startup boundary
was restored; subsequent full and scoped suites passed. The existing quiche
linker emitted its informational import-library warning; the helper gates
completed without suppressing checks.

## Covered behavior and safety

- Cancellation closes a transport even while its failure notification cannot
  progress, and prevents a pending event from publishing a replacement.
- Native shutdown timeout/cancellation keeps the original worker joinable;
  runtime shutdown returns without freeing state still owned by a worker.
- Prepared cancellation releases an actual scripted Agent Named Pipe without
  promoting it; producer cancellation precedes observed EOF. Ordinary failure
  retains the same lease for retry. Already cancelled startup cannot poll new
  connection work.
- A stale startup watchdog cannot restore a later Prepared transaction.
  Existing mock Backend, Connect/Retry, cancellation, account-switch,
  reattachment, recovery-budget and in-place Gate retry tests remain green.
- Lifecycle logs retain typed stages, elapsed time and journal generation while
  excluding request/response plan contents. Read-only polling creates no
  lifecycle log traffic. Empty replies remain failures. Writer-owned run IDs
  and sequence numbers replace forged correlation fields and respect the event
  size limit.
- The Agent's read-only recovery sampling, bounded history, protected journal
  format and protobuf remain unchanged. Clean still requires the existing
  identity checks and both interface/PnP absence. Failed or unknown observations
  cannot authorize a new VPN transaction.

Only source, deterministic unit fixtures, loopback IPC/networking, and
compile-only gates were exercised. No generated installer, live VPN/TUN,
service recovery command, WFP/route/DNS/system-proxy mutation, or APK was run.
Flutter, Kotlin, protobuf, build tooling and workflow sources were unchanged;
their unrelated gates were not rerun for this patch.

## Remaining evidence

| Scenario | Status |
| --- | --- |
| Real Windows TUN exit/reconnect and Engine/Agent termination in a snapshot VM with independent management | `not_run` |
| High-load Wintun removal and coexistence with other Wintun users | `not_run` |
| Independent IPv4/IPv6/DNS/Kill Switch observation | `not_run` |
| Android device lifecycle and protected performance sampling | `not_run` |
| BBRv3 versus Cubic and edge-resolved live traffic comparison | `not_run` |
| Packaging, installation and publication | `not_run` |

These source fixes address cancellation and recovery handoff defects. They do
not establish why Windows previously retained an interface after its PnP
device disappeared, or prove BBRv3 caused the initial connection failure.
The added lifecycle correlation and existing recovery observations support
that follow-up without relaxing cleanup success criteria. Protected-runner
evidence remains optional and is not a publication prerequisite.

The lifecycle contract is described in [VPN Gate](VPN_GATE.md); the required
check matrix remains [CONTRIBUTING.md](../CONTRIBUTING.md).
