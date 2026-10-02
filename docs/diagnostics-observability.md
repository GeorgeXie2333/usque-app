# Diagnostics and observability

This reference describes current source behavior. It is not a validation report.
Use [Network Doctor](network-doctor.md) for the user workflow and
[Contributing](../CONTRIBUTING.md) for required checks and isolated-test boundaries.

## Public contract and evidence

[diagnostics-contract.json](../proto/usque/diagnostics-contract.json) defines
stable check IDs, failure/event codes, summary/remediation keys, evidence keys
and fact tokens. [The generator](../tool/generate_diagnostics_contract.py)
produces checked-in Rust, Kotlin and Dart projections; its freshness checks
detect drift. Update the contract first rather than editing generated files.
Protobuf field numbers and existing identifiers remain append-only.

A finding's outcome is separate from how its evidence was obtained:

| Field | Values and meaning |
| --- | --- |
| Source | `config`: configuration validation; `runtime`: runtime evidence; `platform`: platform observation; `active_probe`: an explicit Deep probe; `frontend`: client-side observation; `unknown`: no established source |
| Availability | `observed`: the source supplied the stated observation; `inferred`: derived from other state; `unavailable`: missing observation; `stale`: outdated or mismatched scope; `not_applicable`: the check does not apply |
| Correlation | Optional random connection-instance UUID and physical network generation; these are not account/device identities. Connection session generation and platform recovery journal generation have separate meanings. |
| Age | When supplied, age uses the quality/timeline timestamp or captured diagnostic context; frontend receipt time is not substituted for missing timestamps. |

Observed configuration or runtime state does not establish externally observed
traffic behavior. An inferred condition is presented as inferred. Unsupported,
missing and stale metrics cannot become successful zero measurements. Explicit
numeric zero remains a measurement; legacy protobuf scalar timeline counters
retain their existing zero semantics when a metrics message is present.

Typed evidence is either `{key: "fact", token: <allowlisted token>}` or
`{key: <allowlisted key>, number: <unsigned integer>}`. Ambiguous, private or
unknown values are rejected. Legacy string evidence remains compatible through
the same allowlists. Findings with no transport failure can still carry an
actionable remediation. The UI displays source/availability and omits raw
correlation IDs from ordinary presentation.

## Sessions and delivery

Standard reads configuration and existing snapshots without creating external
connections or applying platform network state. Deep requires confirmation.
The existing authenticated QUIC and encrypted-DNS probe safety/cleanup contracts
remain in [Network Doctor](network-doctor.md).

The desktop manager owns its worker, cancellation token and session. Clearing
retires and joins the old worker before another start can acquire that
lifecycle. Cancellation remains active while checks release their resources.
Data reset excludes new diagnostic captures through configuration and log
clearing, using a separate lifecycle guard from Deep's networking lease.
Android waits for the service's post-cleanup probe reply within the remaining
four-second check budget (at most 500 ms after cancellation). Missing confirmation
produces failed execution with unavailable cleanup evidence. The bridge retains
the pending-probe gate until an actual reply or client destruction; Binder loss
does not establish cleanup, and can leave probes unavailable for that client.
A completed session can contain failed findings: completion describes execution,
not a successful network verdict.

Full session snapshots contain all findings, parallel `active_checks` and a
monotonic session `revision`. The desktop event stream sends a full snapshot
on attachment, when the revision changes on its one-second tick, and after
broadcast lag. Existing start/check/completion events remain compatible.
Flutter ignores lower revisions and active updates after a terminal result for
the same session. Legacy check events request single-flight `GetDiagnostics`
recovery instead of fabricating revisions.
Conflicting session IDs trigger a fresh read at the recovery cadence; the reply
and intervening event are not assumed to have a universal delivery order.

| Bound | Current limit |
| --- | --- |
| Standard / Deep session budget | 2 seconds / 15 seconds |
| Desktop check concurrency | At most 4, with one check per resource group |
| Deep check ceiling / socket I/O | 4 seconds / 3.8 seconds, reserving cleanup time |
| Diagnostic event broadcast | 128 buffered events; lag recovers from a full snapshot |
| Flutter active-session recovery | Single-flight polling every 750 ms |
| Visible Diagnostics timeline refresh | Every 2 seconds; read-only, independent of session polling |
| Flutter timeline read | 2-second caller timeout, leaving delivery time around Android's 750 ms fallback budget; the underlying request stays owned until completion so another read cannot overlap |
| Flutter evidence / timeline display | At most 32 public evidence items per finding / latest 100 events; source drops and view omissions are shown separately |
| Android timeline bridge | At most 256 events and 192 KiB, with an optional JNI method and one pending request |

## Timeline and capture scope

Transport metrics are sampled independently of GUI refresh and diagnostic
polling. Existing source observations determine rates and freshness; repaint,
timeline reads and exports do not create replacement observations or additional
probes. See [Network-quality IPC](network-quality-ipc.md) for source rings and
sampling semantics.

The desktop Engine keeps one bounded terminal connection evidence snapshot
after the active runtime is removed. Shutdown can update that same session's
terminal timeline and cleanup result without overwriting a newer session.
The UI marks this as **Last connection**. The retained snapshot is process-local,
not a durable history database.

Android prefers its bounded native transport timeline when available, marked
`runtime / observed`. Older or unavailable JNI sources use the existing
platform phase timeline, marked `platform / inferred`. Missing fallback RTT,
fallback and queue measurements stay unavailable. Actual capture timestamps can
supply age; absent timestamps do not produce synthetic ages. Native timeline
mirroring runs at 1 Hz and shutdown; the full timeline is not added to ordinary
events.

Desktop exports capture the connection, timeline and one quality snapshot from
the same active or retained runtime scope. Saved configuration is a separate,
explicitly named scope. `capture.json` records the phases, session identity,
quality age, cleanup status, dropped timeline events and whether the connection
stayed stable during capture. A later platform observation is omitted if its
connection scope changed. Android's `export-capture.json` records available
connection/network generations and timeline correlation. Mismatched timeline
events/metrics are omitted and marked stale; mismatched scoped finding evidence
is also excluded. Missing correlation is reported as unavailable rather than
proof that independently collected sources match.
Scope stability checks identity rather than atomic sampling across every
source. A read-only runtime-health projection can be newer than the published
connection-state phase without changing that state machine.

## Log ownership, bounds and health

Desktop [logging.rs](../crates/usque-engine/src/logging.rs) gives a normalized
log directory one writer owner within the process. Producers format and filter
bounded records, then enqueue without filesystem I/O. The owner serializes
append, rotation, pruning, export capture and clear. Clear epochs prevent an
older formatter or queued record from restoring cleared data. A capture barrier
syncs preceding writes before the tail is read.
Directory ownership survives final-handle retirement until the old worker has
closed its file. Reopening during retirement yields an unavailable sink;
capture/clear report busy. Persisted-only operations also reserve the directory.
On reopen, an unfinished tail fragment is isolated before fresh records are
accepted; recovery is counted and the invalid fragment remains rejectable.

Android
[AndroidLogStore.kt](../apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/AndroidLogStore.kt)
uses a bounded serial owner and directory locks for append, rotation, capture
and clear. Service log snapshots include an ordered capture barrier; unavailable
service snapshots can fall back to explicitly marked persisted-only reads.
Clear pauses producers before its barrier, and failures are reported.

| Limit | Desktop Engine | Android |
| --- | --- | --- |
| Producer queue | 248 events and 2 MiB; command channel has 256 slots | 128 events; at most 8 control commands |
| Single record | 256 KiB | 2 KiB |
| Rotation / storage target | 4 MiB segments / 20 MiB total | 4 MiB segments / 20 MiB total |
| Retention | Archived files older than 7 days by file time; nonempty active segment rotates after 24 hours; periodic pruning every 60 seconds | Files older than 7 days by modification time, checked during write/capture |
| Export log tail | 2 MiB source-read and output bounds; 1,024 directory-entry scan limit | 128 KiB service/persisted bundle capture; the owner has a 2 MiB internal capture ceiling |
| Capture barrier wait | 5 seconds | 5 seconds |

Retention is a file/segment policy, not a strict seven-day expiry for each
record. Continuous writes can keep older records in an Android active segment.
Disk errors, queue overflow, shutdown timeouts and process crashes can lose
logs. Desktop periodically flushes and syncs at one-second intervals and tries
to drain/sync on orderly shutdown; these are not crash-durability guarantees.

Health reports distinguish accepted/queued/written events, drops, write
failures and availability. Desktop also tracks oversized and unconfirmed
events; export metadata records unreadable files, incomplete/invalid/oversized
records, rejected records, byte truncation and capture status. Android reports
barrier completion, persisted-only fallback, read failures and omitted lines.
An empty log payload is therefore not evidence that nothing happened.

## Local export and privacy

Export selects the newest complete records across active and rotated logs,
then restores the selected records to forward order. The active file takes
priority; archive ordering uses file time, so clock changes can limit ordering
between archives. Records outside the bounded tail or rejected by public projection are not silently
represented as a complete log. Desktop `log-export.json` and Android
`log-storage-health.json` explain omissions; manifests list content sizes and
SHA-256 hashes. Android limits the collected payloads to 8 MiB.

Public logs have a stronger boundary than local filtered debug text: only
fixed codes, enums, approved numeric context and permitted timestamps survive.
Arbitrary messages, nested error objects, paths and unknown fields are excluded.
Diagnostic summaries/evidence also use typed allowlists. Text scrubbing of
secrets, network identifiers and paths remains defense in depth. Bundles omit
credentials, keys, tokens, license material, endpoint pins, profile names,
traffic destinations, user DNS queries, SSIDs and packet payloads.

The desktop writes and syncs a temporary ZIP in the destination directory,
then replaces the destination. Android writes to the selected document
provider's stream; provider failures can leave partial output, and this path
does not promise atomic file replacement. Nothing is uploaded automatically.
Logging and diagnostic queues are independent of authoritative platform
recovery journals and cannot establish recovery or traffic protection.

Local checks, bounded counters and golden screenshots do not prove absence of
DNS/Kill Switch leaks, platform restoration or real-world performance. Those
claims require the matching isolated evidence described in
[Reliability testing](RELIABILITY_TESTING.md). Unavailable isolated validation
must remain `not_run`.
