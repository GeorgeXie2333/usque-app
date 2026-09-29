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
- Keep exports local and atomic. Do not collect traffic destinations, credentials,
  profile names, device identity, packet captures or user DNS queries.

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

Implementation and review results will be appended here after the corresponding
changes and checks actually complete. Planned validation is not a pass.
