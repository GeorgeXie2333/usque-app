# Network settings save and application

The network settings contract separates durable settings from the current
session. Saving is not evidence that platform operations have completed.

## Entry points and ownership

- Flutter owns form drafts. NetworkSettingsController serializes submissions,
  accepts confirmed saved values, and filters replies by process epoch and
  sequence. Account writes share this submission order.
- Rust owns field validation, normalization, the durable configuration, and
  application planning. The planner compares against the session profile.
- Desktop and Android hosts own execution, session identity, cancellation,
  runtime confirmation, and platform cleanup.

The [control protocol](../proto/usque/v1/control.proto) appends request fields
41 (save) and 42 (state), response field 22, event field 24, and capability
field 25. Old fields retain their wire numbers and shapes. An engine without
the new capability cannot accept saves from the new GUI.

Each save carries an operation UUID, account UUID, non-secret profile values,
and an explicit field mask. The account must be the current edit context.
The command cannot create or rename accounts, change identity, or write
credentials. Unknown fields and managed Zero Trust endpoint edits are rejected.
Rust disables system proxy when HTTP is disabled.

The result includes process epoch, sequence, operation ID, session ID, saved
profile, optional confirmed session profile, deferred fields, a sanitized error
code, and runtime status. The optional persisted flag means:

| persisted | Meaning |
| --- | --- |
| true | The save was acknowledged independently of runtime application. |
| absent | No durable acknowledgement is available for this operation. |

A structured rejection is a failed save. The GUI retains the draft. An
uncertain commit or lost reply is queried, never automatically replayed. If
the operation cannot be confirmed, the draft remains dirty. Applied, failed,
deferred, and unknown runtime states do not undo an acknowledged save.

The GUI tracks a failed status query separately from an unconfirmed save.
A valid query or event restores status visibility, including a query returning
the same epoch and sequence. Older sequences and retired epochs cannot clear
the warning or replace state. An uncertain save remains unknown until its
own operation ID receives a durable acknowledgement; an unrelated successful
save does not confirm it. A query failure arriving after a newer authoritative
reply cannot put the interface back into an unknown state.

## Application rules

| Situation | Result |
| --- | --- |
| Disconnected, connecting, reconnecting, disconnecting, error, or executor busy | Save; wait for a manual connection. |
| Stable; congestion control only | Save; keep the session algorithm. |
| Stable; auto-connect only | Save; no runtime action. |
| Stable; hot fields | Save; run the existing frontend/platform operation. |
| Stable; cold fields | Save; one controlled reconnect. |
| Mixed congestion control and runtime edits | Apply the requested runtime fields; keep congestion control deferred. |
| Previously deferred settings exist | Leave them deferred unless explicitly submitted or required by a dependency. |
| Partial platform failure | Retain saved settings; invalidate unconfirmed runtime configuration. |
| Platform restoration pending | Report applying; never infer applied from request acceptance. |

The [shared planner](../crates/usque-core/src/network_settings.rs) owns these
rules. The [desktop adapter](../crates/usque-engine/src/network_settings.rs)
reserves an immediately available lifecycle executor before the short save
transaction. A busy executor does not acquire an application queue entry.
Runtime execution rechecks session identity and disconnect intent.

Automatic recovery uses the captured session profile. Manual connect and Retry
read saved settings. A cold application failure can attempt one runtime
restoration using the previous session, without changing the durable file.
Android keeps the confirmed recovery profile separate from an in-progress
settings target and confirms the target after native and platform completion.

## Persistence and safety

[ConfigStore](../crates/usque-core/src/storage.rs) provides a short
read/modify/validate/atomic-save transaction under a stable sidecar file lock.
The lock is on the sidecar inode, not on the replaced JSON inode. Android's
legacy profile commands also use that lock; GEO downloads run outside it.
Desktop account commits preserve the latest network settings. Credential I/O,
network requests, runtime shutdown, and TUN operations stay outside the store
transaction.

The schema remains 14. Epochs, sequences, operation IDs, and application state
are in memory and do not create a durable operation log. Passwords are removed
from published profiles. This change does not relax Kill Switch, TUN retention,
Agent journal, privileged cleanup, or isolated-runner requirements.

## Verification

Shared policy cases are exercised in the core planner, desktop adapter, and
Android adapter tests. They cover transitional/busy saves, mixed deferred
fields, invalid masks, dependency normalization, concurrent persistence, and
partial runtime failure. Flutter and Binder tests cover acknowledgement
ordering, duplicate replies, lost replies, draft retention, and capability
gating. IPC tests pin the appended wire numbers and reject malformed framing.

Run the applicable matrix in [CONTRIBUTING](../CONTRIBUTING.md). The two
congestion-control visual fixtures use a confirmed saved/applied state pair;
their save bar now displays the unified deferred message in English and
Simplified Chinese, including the 200% TV text fixture. Existing failure
artifacts are not baseline inputs.

Real VPN lifecycle, WFP, route/DNS/system-proxy restoration, connected
uninstall, crash recovery, Android device lifecycle, and leak observation
require the isolated environments in [AGENTS](../AGENTS.md). Workstation
unit and compile checks do not establish those results. No MSI or release
APK is part of this change.

### Workstation validation, 2026-09-08

The following commands completed successfully on Windows using Rust 1.97.1,
Flutter 3.44.7 (84fc5cbb223bc12f83d65b647ff8a56caf779ffd), the pinned Android
NDK/CMake, and Buf 1.72.0. Commands shown as `flutter` and `dart` used the SDK
resolved from Android local.properties, not a global installation.

From the repository root:

```powershell
cargo fmt --all --check
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2
& .\tool\build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy
buf lint
buf format --exit-code --diff
buf breaking --against '.git#ref=5a03d1b91528c1556f5160fe74b976f9fcf0f0ca' --against-config buf.yaml
```

The Buf reference is the inspected origin/main baseline for this worktree;
CI must still check the eventual PR target SHA. An intermediate Rust run
failed when an existing DNS loopback test could not bind a TCP port (Windows
10013); the complete helper test rerun passed without changing that test.

From apps/usque_gui:

```powershell
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub --reporter expanded
flutter build apk --debug --config-only --no-pub
& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .
flutter build windows --release --no-pub
```

All 405 Flutter tests passed, including exact Windows bitmap comparisons.
The six review-fix regressions cover query/event recovery, stale replies,
out-of-order query failures, operation-specific durable confirmation, and a
confirmation event arriving before the save response times out.
The Windows GUI release build completed successfully without launching it.
The changed phone-light and TV-dark congestion fixtures were rendered and
visually reviewed in English and Chinese. The new save bar interaction test
covers both themes, both languages, 200% text, and keyboard focus/activation.

From apps/usque_gui/android:

```powershell
.\gradlew.bat --no-daemon :app:ktlintCheck :app:testDebugUnitTest :app:lintDebug
```

The Gradle checks also compiled debug JNI libraries for all three configured
ABIs. Those generated files remain outside version control. No release APK
was built or installed.

The frozen oracle passed `go mod verify` and `go test ./...` from oracle/go,
and `python tool/verify_oracle_archive.py` from the root (verified Python
3.12.14). The archived source was unchanged.
`python tool/check_repository_policy.py` and `git diff --check` also passed.

| Protected environment | Result |
| --- | --- |
| usque-snapshot-vm | not_run: no isolated snapshot/management channel supplied |
| usque-android-device | not_run: no dedicated device or isolated emulator supplied |
| usque-network-observer | not_run: no isolated observer supplied |
| usque-performance-lab | not_run: no controlled performance runner supplied |

These missing environments provide no lifecycle, cleanup, leak, or performance
pass. They do not block the compile-only workstation checks or change the
repository's optional protected-runner publication policy.
