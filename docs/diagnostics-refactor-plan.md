# Diagnostics and logging refactor

This plan was agreed for implementation on 2026-09-30. Each independently
reviewable change receives its own commit. The starting source is `b55fd04`.

## Design decisions

- Preserve Standard's read-only behavior, Deep confirmation and socket/cleanup
  limits, append-only protobuf numbers and stable failure/check identifiers.
- Keep authoritative platform recovery journals independent of diagnostic
  queues. Logging failure must never bypass protection or block cleanup.
- Use a shared, checked-in diagnostic contract for public check/event names and
  allowlisted evidence. Generate language projections and test their freshness.
- Represent evidence provenance and availability explicitly. Configuration,
  runtime inference, actual platform observation and active probes are distinct.
  Missing observations are never zero measurements or successful checks.
- Retain a bounded terminal connection timeline independently of the runtime.
  Capture exports with connection/generation identity and report missing,
  truncated or inconsistent evidence rather than silently combining it.
- Give each log store one owner for writes, rotation, retention and clearing.
  Producers use bounded queues; dropped events and write failures are observable.
  Public exports use typed allowlists; text scrubbing is defense in depth.
- Keep desktop exports local and atomic. Android selected-document-provider
  failures are reported without promising atomic replacement across providers.
  Do not collect traffic destinations, credentials, profile names, device
  identity, packet captures or user DNS queries.

## Implementation batches

1. **Plan and contract:** record this plan, introduce shared diagnostic metadata,
   generated language allowlists and compatible wire additions.
2. **Engine log export correctness:** prioritize the newest complete JSONL
   records and expose size, truncation and omission information.
3. **Engine log storage:** bounded asynchronous writer, coordinated lifecycle,
   health counters and stricter privacy filtering with safe numeric context.
4. **Diagnostic semantics and supervision:** correct unsupported passes and
   misleading failures; attach provenance and typed evidence; preserve parallel
   progress, cancellation cleanup and snapshot recovery after stream lag.
5. **Connection evidence:** retain terminal timelines, capture quality once,
   correlate exported sources and reject/mask observations from another session.
6. **Android observability:** shared contract, accurate DNS events and unknown
   metrics, provenance, bounded asynchronous logs and export completeness.
7. **Flutter presentation:** compatible metadata decoding, accurate event names,
   evidence availability and omissions, actionable findings, and independent
   timeline refresh behavior using existing components and theme.
8. **Documentation and validation:** describe the final contracts, record exact
   checks and unavailable isolated validation, then review all changes with a
   subagent, perform a primary-agent review, fix findings in separate commits and
   rerun affected checks.

## Acceptance

Regression tests must cover newest-tail selection across rotation, disk and
queue failures, secret-bearing hostile log input, clearing/rotation ordering,
unknown observations, contract drift, terminal timeline retention, stale
connection evidence, stream resynchronization and old-client compatibility.
UI tests cover precise labels, unavailable counts, actionable warnings, reset
and cancellation races; golden changes require Windows-pinned visual review.

Run every applicable exact check in [Contributing](../CONTRIBUTING.md): Markdown
policy and diff checks; Windows-helper Rust Clippy, tests and release compilation;
Android-target Clippy, Kotlin format/unit/lint; pinned Flutter resolution,
format/analyze/tests and Windows compilation; Python checks/tests and Buf checks
when their sources change; initialized aggregate source checks.

No MSI or release APK is requested. Do not install packages, start VPN/TUN,
mutate platform networking or run protected reliability tests on this machine.
Windows snapshot, dedicated Android device, external leak observation and lab
performance validation remain `not_run` unless appropriate infrastructure exists.

## Execution record

Completed on 2026-09-30 on `codex/diagnostics-observability-refactor`, starting
from `b55fd04`. The final implementation commit is `43f7448`; subsequent changes
are documentation only. Each independently reviewable change has its own local
commit. No package installation, release signing, publication or push was run.

All eight batches are complete. Current behavior and its limits are in
[Diagnostics and observability](diagnostics-observability.md). The shared contract
contains 39 check IDs, 54 stable transport failure codes, language projections
and checked evidence/log allowlists. Compatible optional IPC fields preserve old
clients. Engine and Android logging use bounded owners and ordered capture/clear
barriers; timelines and exports distinguish current, retained, unavailable and
mismatched evidence. Flutter preserves complete sessions, truthful event labels,
unknown counters and independent timeline refresh.

### Independent review and primary-agent repairs

After implementation was frozen, three subagents cross-reviewed areas they had
not authored: desktop logging/bootstrap/export/privacy; Engine sessions,
capture/IPC/contracts/native publishers; and Android/Flutter behavior. The
primary agent independently checked the findings, repaired them and reviewed
the complete resulting diff. Focused subagent follow-up reviews checked the
ownership, session-ordering and cancellation repairs. No actionable finding
remains from these reviews.

| Finding or scoped gap | Repair | Validation |
| --- | --- | --- |
| Retiring log worker could overlap a replacement and resurrect cleared records | `c0df574`: retain ownership through actual exit; reserve persisted capture/clear | Deterministic blocked-owner/reopen/clear and offline-capture regressions |
| Partial persisted tail could consume the next fresh record during recovery | `a3f8589`: isolate the fragment before accepting records; count repairs | Partial-tail fixture preserves the fresh parseable record and sync count |
| Concurrent diagnostic start could survive a data reset with old context | `0cc24b5`: exclude capture/start through the whole reset; cancel before Deep lease wait | Memory-vault test pauses reset and exercises a second IPC start |
| Non-ready H2/pin checks used H3 summaries | `6fbd4a2`: keep protocol/pin-specific explanations and actual typed failures | Disconnected/reconnecting H2 and failed-pin regressions |
| Cached early progress overrode a newer start reply | `54b1201`: compare revision and terminal state | Early Running/revision 2 versus Completed/revision 5 |
| Cross-session replies/events could overwrite or hide current state | `9836569`, `e7b7f92`: reject conflicting stale replies and reread at the guarded recovery cadence | Both event/reply orderings, reset and single-flight tests |
| Android replacement/cleanup logs mixed runtime identities | `8d4647c`: freeze old cleanup context and use unknown runtime identity for new requests | Delayed old stop completion keeps its old scope |
| Unavailable Android export observations became false/zero | `4118cb3`: omit missing/invalid fields and expose availability | Empty/disconnected source and actual false/zero fixtures |
| Clock changes let Android archives consume the newest-tail budget | `947265f`: active segment first | File-time fixture retains the latest terminal event |
| Probe cancellation could finish before cleanup acknowledgement | `43f7448`: bounded ACK wait, explicit unavailable failure and retained request gate | Delayed, missing, wrong and late replies; Binder loss; four-second budget; private marker exclusion |

The partial-tail and cancellation findings include pre-existing gaps that the
new recovery/completion semantics depended on. Their repair does not prove
crash durability or real-device cleanup. Provider output remains non-atomic;
file-time ordering between archives and per-record retention remain limited as
documented in the current reference.

### Completed workstation checks

Rust used `1.97.1` and locked dependencies through the Windows/Android helpers.
Flutter came from `android/local.properties`: `3.44.7`, full revision
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`. Python was the verified bundled
`3.12.14` executable, with Ruff `0.16.0`; PSScriptAnalyzer `1.25.0` and Buf
`1.72.0` were verified by the aggregate gate. Installed matching tools were
reused without changing dependency locks.

The commands below completed successfully against the final relevant source
files. Rust/Dart validation finished before the final Kotlin commit; its shared
fact token was already present during those runs. Full Android and aggregate
checks completed after `43f7448`. These are local development checks, not
release-candidate or protected-runner evidence.

| Exact command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed, full workspace/all targets |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | Passed, full workspace/all targets; Engine 247 lib tests passed, 4 live tests ignored, plus 2 main and 2 bootstrap tests passed |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | Passed, compile-only release |
| `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed; not substituted for three-ABI compilation |
| `flutter pub get --enforce-lockfile` | Passed |
| `dart format --output=none --set-exit-if-changed lib test` | Passed, 194 files, no changes |
| `flutter analyze --no-pub` | Passed |
| `flutter test --no-pub` | Passed, 717 tests including the Windows golden suite; baselines unchanged |
| `& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .` | Passed |
| `flutter build windows --release --no-pub --split-debug-info=build/symbols/windows` | Passed, compile-only |
| `flutter build apk --debug --config-only --no-pub` | Passed; no release APK or installation |
| `.\gradlew.bat --no-daemon :app:ktlintCheck` | Passed |
| `.\gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` | Passed, 273 JVM tests, zero failures/errors/skips; complete command without exclusions rebuilt all three debug JNI ABIs and its Flutter dependencies |
| `python -m ruff check tool` | Passed |
| `python -m ruff format --check tool` | Passed, 23 files |
| `python tool/generate_diagnostics_contract.py --check` | Passed |
| `python -m unittest discover -s tool -p "test_*.py" -v` | Passed, 100 tests |
| `buf lint` | Passed |
| `buf format --exit-code --diff` | Passed |
| `buf breaking --against '.git#ref=b55fd04'` | Passed, append-only wire additions |
| `pwsh -NoProfile -File tool/check_source.ps1` | Passed, initialized with the Windows helper in the same PowerShell session |

Flutter commands ran in `apps/usque_gui`; Gradle commands ran in its `android`
directory. The pinned SDK's `bin` was prepended to PATH. Python commands above
used `C:/Users/George/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe`
through the `$taskPython` PowerShell variable. The same Python directory and
Flutter `bin` were prepended to PATH for the aggregate command, after running
the Windows helper's Clippy command in that session.

An early workspace bootstrap test inherited `RUST_LOG=warn` and failed its INFO
shutdown-boundary assertion; `82057c0` isolated the harmless child environment.
The complete workspace test command subsequently passed. Intermediate test
fixture/format failures were corrected and affected checks rerun. Existing
vendored linker and Gradle deprecation messages did not become first-party
warning suppressions or weakened gates.

Markdown acceptance uses the verified Python executable for
`python tool/check_repository_policy.py` and `git diff --check`; both completed
successfully after the record was saved. Generated binaries, symbols, JNI libraries, caches,
logs and diagnostics remain outside the commits. Transport wire/oracle sources
were unchanged; Go interoperability fixtures were not part of this control-IPC
change.

### Not run

| Isolated validation | Status and reason |
| --- | --- |
| Windows install/upgrade/uninstall, real VPN/TUN/WFP/DNS/routes/proxy restoration and crash recovery | `not_run`: no approved snapshot VM with independent management channel |
| Android device/emulator VPN, Doze, Always-on/Lockdown, reboot and upgrade lifecycle | `not_run`: no dedicated isolated device/emulator validation |
| Externally observed IPv4/IPv6/DNS/Kill Switch leak behavior | `not_run`: no network-observer environment |
| Controlled repeated performance/resource measurements | `not_run`: no performance lab |
| Protected reliability runner and release evidence reports | `not_run`: no protected execution requested or fabricated |

Local fake-engine/JVM/mock-platform tests establish their tested contracts.
They do not establish native VPN restoration, leak freedom or measured
performance gains. No MSI, release APK, official signature, upload or published
release was produced.
