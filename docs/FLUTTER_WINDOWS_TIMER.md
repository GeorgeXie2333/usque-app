# Windows Flutter timer repair and source build

The repository retains the dependency repair, standalone regression tests, and
a Windows engine source-build recipe for
[#142](https://github.com/GeorgeXie2333/usque-app/issues/142). Windows build and
release workflows verify that the application contains this pinned engine's
exact DLL. The v0.3.4 candidate may reuse the specifically reviewed attested
source-build artifacts; their original build inputs and producer identity are
verified before extraction. The helper builds in a separate
directory and does not replace an installed Flutter SDK. A source patch or
successful compile alone is not evidence that the application regression is resolved.

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

### Rebuilt-engine comparison

On 2026-11-02, three alternating pairs used the same simulated-Engine GUI and
configuration in that VM, changing only `flutter_windows.dll`. Each run sampled
the three states below for approximately 30 seconds each. All six runs exited
normally; the source-built engine's DLL hash matched the build receipt.

| One-core process CPU, three runs | Original engine | Rebuilt engine |
| --- | --- | --- |
| Initially hidden, never shown | 62.46–85.98% | 1.45–1.56% |
| Hidden after showing | 65.02–84.59% | 0.47–0.73% |
| Minimized after showing | 84.96–85.61% | 0.10–0.21% |

This controlled comparison supports the dependency repair for the reproduced
CPU symptom. It uses the real Windows Flutter GUI with a simulated Engine and
does not establish native VPN behavior. The production application was also
compiled through the local-engine path and its staged DLL passed the exact hash
and PE architecture check. Full engine tests and connected lifecycle validation
remain separate from these results.

## Windows engine integration

The [engine recipe](../tool/flutter_windows_engine_manifest.json) pins the
framework, engine, and depot_tools revisions, each patch hash, and the exact
patched source contents. The build helper checks those identities before GN and
Ninja. Fresh dependency synchronization resolves the pinned DEPS and CIPD inputs;
a resumed local `Build` checks the preparation receipt and configuration, not
every dependency working tree recursively. Release workflows and the trusted
engine-package producer prepare fresh checkouts. The separate engine checkout
contains the timer repair, two engine GoogleTest regressions, and small
build-tool adjustments for Windows SDK 26100 and the Visual Studio 2026 DIA DLL
name. These adjustments do not change timer behavior.
The existing delayed-task test derives its deadline and elapsed measurement from
one start timestamp and checks the callback-entry timestamp against that exact
deadline. Its strict 50 ms lower bound is retained.

Use [the contributing commands](../CONTRIBUTING.md#windows-flutter-engine-source-build)
for a source build. Prerequisites include PowerShell 7, Python 3.10+, Git, Visual
Studio C++ tools with matching ATL and target CRT libraries, Windows SDK 26100
with debugging tools, long
path support, and permission to create the symlinks in official dependency
archives. The helper checks prerequisites without installing components or
changing machine policy. A prepared SDK or Visual Studio overlay can be passed
explicitly; its MSVC version must match the selected installation. Dependency
downloads and all build caches remain under the owned build directory.

CI uses the shared `create-build-directory` command to compare the volumes behind
`RUNNER_TEMP` and `LOCALAPPDATA`, then creates a fresh owned subdirectory on one
with at least 40 GiB free. Local regular-file sizes totaled about 15.2 GiB for
source and dependencies, 11.2 GiB for x64 release and test outputs, plus caches;
these figures do not deduplicate hardlinks or measure filesystem compression.
40 GiB provides space for these inputs and build temporaries. This is larger than
the standard runner's [documented 14 GB workspace](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
All subsequent engine source, receipt, and PDB input paths use the selected
`USQUE_FLUTTER_ENGINE_ROOT`. If neither volume has enough space, engine preparation
fails before downloading engine dependencies or compiling engine targets. It
does not delete global tools, select paid
runners, or substitute an unpatched DLL. Capacity selection does not establish
that a standard hosted runner or native ARM64 CI build has actually passed.

Both `--local-engine` and `--local-engine-host` must select `host_release` for
x64 or `host_release_arm64` for ARM64. Flutter resolves the Windows DLL and C++
wrapper through the host output. Selecting an x64 host output for an ARM64
application would select the wrong DLL. The recipe also builds the kernel
platform files, `sky_engine`, font tools, and tessellator needed by the framework.

`engine-build-<architecture>.json` records the recipe, source, toolchain files,
required output hashes, DLL architecture, and engine test status. Assembly checks
the actual staged DLL against that record before signing. The build record and
matching engine PDB join the existing separate Windows symbols artifact; they
are not added to the installable payload. The v0.3.4 release reuses only the
pinned, attested source-build producer recorded by the compatibility adapter.
Its restore receipt is independently attested and retained with the PDB. The
restore step receives no release PFX or password. Standard release signing
and approval rules continue to apply.

The full x64 engine test configuration uses `host_debug_test`; the helper invokes
the engine's own `flutter/testing/run_tests.py --type engine` after compilation.
ARM64 compilation is separate from native test execution. The workflow runs
native application fixtures on its ARM64 runner; this does not imply that the
x64-only full engine test configuration ran on ARM64.

### Engine reuse

The [engine workflow](../.github/workflows/flutter-engine.yml) builds separate
x64 and ARM64 local-engine packages after engine-input changes reach `main` or
`dev`. It can also be dispatched on either branch to refresh an expired package.
It never runs on PR source, receives no release signing material, and does not
publish an application or an official package. The x64 producer runs the full
native engine tests; the ARM64 receipt continues to record those tests as
`not_run`.

The [package helper](../tool/flutter_windows_engine_package.py) computes an exact
reuse key from the canonical recipe and the checked-in build, packaging,
regression and producer-workflow code. Patch hashes and pinned SDK/toolchain
inputs are part of the recipe. Checkout newline differences are normalized in
the build-code key. Ordinary application commits do not change it. The package
contains the local-engine headers, C++ wrapper, tools, shader includes, kernel
files, `sky_engine`, matching prebuilt Dart SDK, license notices, PDB and source-build receipt,
without dependency checkouts or compiler object trees. All package files are hashed.

[Build](../.github/workflows/build.yml) discovers an exact-name artifact from a
successful engine-workflow push or dispatch on this repository's `main` or `dev`.
It pins that immutable artifact ID and checks the download digest, then uses
`gh attestation verify` to require this repository, exact producer workflow,
branch, source and signer commit, and a GitHub-hosted runner. Verification
precedes unpacking or executing any packaged tool. Extraction rejects unsafe
paths, links, case collisions, excess expansion and incomplete or changed files.
The full engine receipt, required assets, test status, PDB and architecture are
checked again. Application/native fixtures and the final staged-DLL check still
run for each build. The restore receipt records the selected artifact and
producer identities separately from the current application commit.

If no successful matching package exists, or all matches have expired, Build
uses its existing fresh source compilation and applicable full engine tests.
An invalid download, failed provenance check or corrupt package fails the job;
it never counts as a cache miss. Lookup or download service failures also fail
explicitly. PRs cannot create trusted engine packages. A changed recipe or build
tool naturally misses until a trusted producer has built the new input set.
Artifacts are retained for 90 days; the first build before the new workflow is
available also follows the source path. For v0.3.4, Build and Release additionally
accept the two pinned artifacts from producer run `41924429831` through the
reviewed compatibility adapter. It checks the producer is an ancestor, proves
build inputs are unchanged apart from the old policy wording and reviewed test
updates, verifies the artifact digest and GitHub attestation before unpacking,
and fails closed on any mismatch. The x64 engine tests and ARM64 `not_run`
status remain distinct. The release workflow does not silently rebuild on a miss.

For local engine iteration, keep the owned directory for the same recipe and
use `-Phase Build` to retain Ninja's incremental outputs and skip dependency
preparation. A changed source-build recipe still requires a fresh directory.
The v0.3.4 release uses the reviewed restore path described above.

## Validation boundary

The candidate's standalone checks do not establish an integrated application fix.
Full engine tests, application build and tests, and the isolated VM comparison
are distinct results. Inspect their actual commands and output records before
claiming they passed. A local source-built DLL does not mean a repaired
application has been published. The shared SDK, application connection logic,
authentication, leases, and network-generation checks are unchanged by this
dependency repair.

Before closing #142, integrate an engine version containing the repair or a
reviewed, reproducible engine build, record its source and binary identities, and
run the applicable complete Windows checks from Contributing plus the actual
visibility/connection comparison. No Flutter upstream PR or separate engine
distribution is created by these helpers.
