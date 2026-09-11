# L4 high-load backpressure and stop follow-up

## Reproduced defects

The pinned stack wrapper's bounded `WakingPipeDev` checked TX readiness once,
but `transmit()` continued handing out tokens after the queue became full.
Its token then used a blocking channel send. An offline reproduction filled
63 of 64 packet slots and invoked actual smoltcp egress on two TCP sockets:
the second packet blocked the worker, and aborting the Tokio task could not
interrupt that synchronous send. Releasing the receiving endpoint unblocked it.

Android previously awaited that outbound enqueue inside a single packet-event
branch, preventing reverse traffic and sampling from running during the wait.
The native stop path subsequently joined the worker without a time bound.
These paths could amplify pressure into a stalled data plane and stop operation.

Separately, the legacy stack `request_nonblocking` helper returned success when
a full command queue discarded a Close request. The L4 cleanup fallback only
ran on error, so that fallback did not handle saturation.

Stress cancellation also exposed a mapping-lifetime gap: a task could be aborted
before its first poll, before its mapping guard was constructed. Dropping the
parent JoinSet requested child cancellation without awaiting every child's Drop.

## Implementation

- A first-party packet pipe uses existing Tokio bounded channels. Every smoltcp
  transmit token owns a reserved slot; full queues decline the token before TCP
  state advances. Sends and nonblocking receives never block a runtime thread.
  The common local-stack helper also applies this fix to CONNECT-IP proxy stacks
  and the existing direct TCP gateway, without changing routing policy.
- Android multiplexes receive, pending send, pending TUN write, and sampling.
  One packet per direction is retained. The owned send future is pinned in place
  across events, not replayed or heap-boxed for every wake. TUN attach/detach
  discards pending state from the retired attachment.
- The narrow core patch adds a truthful enqueue result distinguishing Full from
  Closed. L4 retries only Full asynchronously; closed-stack cleanup ends.
  One-shot Close releases already-closed allocations without requiring another
  packet to trigger reclamation. Unique socket ownership and cancelled-command
  guards remain intact.
- Mapping guards are created before task spawn. Shutdown cancels all top-level
  tasks before awaiting them, then waits for tracked flow/DNS children to drop.
- Android retains the old runtime admission slot until its worker is joined.
  A five-second unconfirmed stop does not detach the worker, start a replacement,
  force-kill the application or report success. A subsequent retry can confirm
  cleanup. Native duplicate TUN FDs are dropped before backend shutdown; Java
  retains its FD when necessary for fail-closed recovery.
- An additive JNI stop-confirmation method leaves the old native symbol present.
  Native stop requested/completed/unconfirmed events contain only allowlisted
  enum tokens. Pending cleanup is separate from the UI's disconnect notification;
  runtime state is unknown rather than falsely stopped while cleanup is pending.

No SNI, identity, pin, endpoint authorization, Auto fallback, UDP policy, DNS
resolver selection, WFP, routing, installer, or signing behavior is relaxed.
The receive/window/memory limits are not increased to hide saturation.

## Regression scope

- Exact 63/64 queue plus two-socket egress, including retention of the unsent SYN.
- Slot release when a token is discarded, wakeup after capacity returns, true
  nonblocking empty receives, and cancellation with a full queue.
- Full cleanup queues of one and 256 entries, plus a live one-shot listener
  whose asynchronous Close retry frees the budget without network progress.
- Two-worker TUN tests with eight simultaneous 512-KiB echo streams: pause the
  receiver, resume and verify all bytes, cancel a burst, then create a new bridge
  and repeat. Shutdown must finish within two seconds and reclaim mappings,
  half-opens and buffer accounting.
- Pending sends survive repeated receive/timer events without duplicate enqueue;
  blocked writes do not prevent reads or cancellation.
- A timed-out worker remains owned until a later confirmed join. Stop tracking
  keeps queued and unconfirmed requests visible and rejects stale confirmations.
- Android snapshot and diagnostic-log tests cover unconfirmed cleanup states.

## Validation status

Validation is development-only; no live VPN, system networking mutation,
personal-device installation, release packaging or publication was performed.
The required change-scoped checks are in [CONTRIBUTING](../CONTRIBUTING.md).
This follow-up is not a performance benchmark or externally observed leak test.

Checks were run on the uncommitted follow-up based on `b0bce7c`. These are
development results, not signed release evidence:

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` and scoped vendored-file `rustfmt --check` | Passed |
| Windows helper `-Variant x64-v2 -CargoAction clippy` | Passed, locked |
| Windows helper `-Variant x64-v2 -CargoAction test` | 901 passed; two existing credential-dependent live tests ignored |
| Compiled `tun_bursts_resume_after_slow_reader_and_stop_cleanly_before_reconnecting --exact` | Ten additional consecutive passes; functional stress, not a performance benchmark |
| Vendored core `cargo test --manifest-path third_party/ts_netstack_smoltcp_core/Cargo.toml --lib --locked` | Six passed |
| Windows helper `-Variant x64-v2` | Rust release-configuration compile passed |
| Android helper `-AbiFilter arm64-v8a -CargoAction clippy` | Passed, locked |
| Android helper `-AbiFilter all -CargoAction build` | Debug JNI compiled for arm64-v8a, armeabi-v7a and x86_64; old/new stop exports verified in all three |
| Flutter locked resolve, format, `analyze --no-pub`, `test --no-pub` | Passed; 410 tests with unchanged image goldens |
| Flutter `build apk --debug --config-only --no-pub` | Passed; no APK installation |
| Gradle `--no-daemon :app:ktlintCheck :app:testDebugUnitTest :app:lintDebug` | Passed; 167 JVM tests, no failures/errors/skips |
| Ruff checks and Python tool unit tests | Passed; 71 Python tests |
| Buf lint/format, repository policy, `git diff --check` | Passed |
| Frozen Go archive verification | 41 files verified; archived source unchanged |

The pinned Flutter SDK was 3.44.7 at
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`. No lockfile or image baseline was
changed. Debug JNI and diagnostic artifacts remain outside version control.

`tool/check_source.ps1` was attempted in the initialized Windows environment.
Rust, Dart/Flutter, Kotlin and Ruff passed. Windows software restriction policy
blocked importing PSScriptAnalyzer 1.25.0, so both PowerShell analyzer passes and
the aggregate command are **not passed**. No policy bypass was attempted; Buf
was run separately. No PowerShell script was modified.

Live Android device reproduction, Always-on/Lockdown lifecycle, independent leak
observation and controlled performance sampling remain not run and require
the corresponding isolated environments. They are supplemental evidence, not
new publication prerequisites.
