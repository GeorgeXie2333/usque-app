# Windows Flutter timer regression candidate

Status, 2026-11-02: the repository retains a reproducible dependency patch and
standalone regression tests for [#142](https://github.com/GeorgeXie2333/usque-app/issues/142).
The application still uses the original prebuilt Flutter engine. This material
does not replace an installed SDK, rebuild `flutter_windows.dll`, or change a
shipping application's behavior.

## Reproduced scheduling defect

The pinned engine's `TimerThread` retains an expired deadline while invoking its
callback without the mutex. If another thread calls `ScheduleAt()` with a future
deadline during that callback, the older expired value wins the comparison, but
the schedule counter still increases. When the callback returns, the changed
counter prevents the timer from parking. It can invoke the callback immediately
again and lose the requested future wake-up.

The retained [candidate patch](../tool/fixtures/flutter_windows_timer/candidate.patch)
consumes an expired deadline under the mutex before invoking the callback. A new
request made during the callback then competes for a fresh pending deadline.
The invocation holds its own callback copy while `Stop()` clears the registered
callback and waits for completion. The obsolete schedule counter is removed.

The pinned version also predates upstream
[PR #193181](https://github.com/flutter/flutter/pull/193181), which corrects
notifications that move a future deadline earlier. The candidate includes that
wait-loop correction. It is a separate defect from callback-time rearming.

## Source identity and regression tests

The [manifest](../tool/fixtures/flutter_windows_timer/manifest.json) records the
historical source identity: Flutter 3.44.7, framework commit
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`, and engine commit
`69c8c61792f04cc809dfef0c910414fb9afc06cd`. The original Windows source bytes matched
the compiled source checksum in the matching Flutter PDB. Normalizing CRLF to LF
also matched the official pinned Git blob.

The small upstream source fixture retains its
[Flutter BSD license](../tool/fixtures/flutter_windows_timer/LICENSE.flutter).
The preparation helper checks the frozen inputs, applies the actual retained
patch in an isolated copy, and extracts the original and patched timer methods
for compilation. The candidate is generated from the patch rather than maintained
as a second handwritten implementation.

The harness uses ordinary threads, condition variables, and bounded barriers:

- Three controlled runs reproduce the original callback-time rearming defect.
- Two separate controls reproduce the older notification defect.
- Ten candidate checks cover rearming, earlier notifications, earliest-deadline
  preservation, self-rearming, and stopping both while waiting and during a callback.
- Wait-entry witnesses delegate to the same standard condition-variable operation;
  they do not replace its clock or predicate.

The original defects are expected negative controls. A successful harness run
requires both their reproduction and all candidate checks to pass. These checks
are not the full Flutter engine GoogleTest suite.

Use the exact command in
[Contributing](../CONTRIBUTING.md#flutter-windows-timer-regression-fixture).
It creates a new output directory under `target/flutter-windows-timer`, leaves
diagnostics there, and does not access an installed Flutter SDK. It requires Git,
Python 3.10 or newer, and the Visual Studio x64 C++ tools. It starts no GUI,
Engine, Agent, VPN, or service and makes no network or system-policy changes.

## Investigation evidence

The application investigation used Usque commit
`57d7624864d940c322e16c855476fa235d182346`. A dedicated Windows VM had one virtual
CPU, 6 GiB RAM, VirtualBox 7.2.22, and 3D acceleration disabled. The later controls
ran on OS build 26300.9711 after Windows updates; earlier stack captures used
26300.9457. Absolute performance and system symbols are not interchangeable
between those builds.

| Same-VM control | Initial hidden | Hidden after showing | Minimized |
| --- | --- | --- | --- |
| Actual GUI with a simulated Engine | 81.52% | 79.44% | 85.42% |
| Plain Win32 message-loop window | No measurable CPU-time increment | No measurable CPU-time increment | No measurable CPU-time increment |

Percentages use one-core process CPU time. The GUI emitted 4, 7, and 3 frames in
the respective observation windows, so it was not a zero-frame test. The control
created no native Engine or VPN. Both controls exited normally.

Read-only observation of the matching Flutter DLL then collected 400 stable
timer-field samples over 80 seconds. During a fully hidden interval, the same
expired deadline remained fixed for 29.6 seconds while `ScheduleAt`'s counter
increased by 393,210; UI CPU was 31.5% of one core. The counter counts scheduling
calls, not messages. This live state is consistent with the defect reproduced by
the controlled harness.

Matching symbols identified `TimerThreadMain`, `TaskRunnerWindow::WakeUp`,
`PostMessageW`, and the application's `GetMessageW` loop. Kernel interrupt/DPC
samples are not by themselves proof that application instructions consumed all
reported kernel time. An exact native-UTC-to-ETL time alignment was not established
for the earlier traces; only their internal timestamp/stack association and
whole-trace module direction were used. Raw traces and process-memory observations
remain private; this document contains only the sanitized summary.

## Integration boundary

The candidate's standalone checks do not establish an integrated application fix.
The full Flutter engine GN/GoogleTest build, a rebuilt Flutter DLL, and application
validation with that DLL have not run. The shared SDK, dependency pins, application
connection logic, authentication, leases, and network-generation checks are
unchanged.

Before closing #142, integrate an engine version containing the repair or a
reviewed, reproducible engine build, record its source and binary identities, and
run the applicable complete Windows checks from Contributing plus the actual
visibility/connection comparison. No custom engine distribution or upstream PR
is created by this repository fixture.
