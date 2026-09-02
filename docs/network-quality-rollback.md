# Network-quality emergency rollback

## Authority and defaults

`crates/usque-transport/src/feature_flags.rs` is the single internal build
decision point. All five production flags are true following deterministic
correctness/resource/compatibility checks; protected performance and leak
measurements remain `not_run`, not a claim of improvement. No ordinary user
configuration, environment variable or remote API can set these flags.
Instances receive an immutable copy; changing a build requires review,
rebuild and the ordinary gates in `CONTRIBUTING.md`.

| Flag / capability | False behavior | Required regression evidence |
| --- | --- | --- |
| `h2_tuned_flow_control` | Explicit Builder, stream and connection windows both 65,535; PING and cancel stay intact | H2 32-combination config and loopback capacity/cancel tests |
| `network_quality_metrics` | Stop quality events/payload/capability; existing business flow and safety counters remain active | Disabled sampler cancellation/counter test and missing-capability GUI tests |
| `udp_batch_io` | Force portable send/receive even if batch mode was requested | Portable/forced-mode/order/truncation/cancel tests on Windows and Linux |
| `automatic_pmtu` | Fixed 1350-byte outer send maximum; no discovery or migration-triggered probe; EMSGSIZE safely reconnects | Fixed-1350 across-path and fail-closed error tests; inner MTU/PTB tests |
| `quic_migration` | No migration handle dispatch; existing full reconnect | Disabled-handle no-candidate test and supervisor generation/reconnect tests |
| `ENCRYPTED_DIRECT_DNS_ENABLED` | Advertise unsupported and reject saved DoH/DoT before connection | Core/runtime capability rejection; GUI preserves custom settings read-only until explicit user change |

Do not rewrite saved DoH/DoT to System. Users may explicitly select System,
with its normal privacy meaning. Do not change protobuf field numbers,
remove appended messages or lower config schema 13. Old peers ignore unknown
fields; absence of a capability disables only its new UI/control.

## Procedure

1. Record affected platform/path, stable reason codes, exact candidate identity
   and available sanitized evidence. Never collect raw QNAME/endpoint/SSID/CID
   in ordinary logs or publish restricted packet captures.
2. Change the smallest internal flag/capability needed, keeping the default
   fail-closed contract and one data-bearing path. There is no live remote
   feature toggle. Do not weaken TLS, generation, lease, WFP or cleanup checks.
3. Run the complete change-scoped matrix from `CONTRIBUTING.md`, plus the
   feature's false-mode tests and [integration matrix](network-quality-acceptance.md).
   Use the Windows build helper, locked Flutter resolve and pinned Android
   helper. Fault-injection lab features must not enter release artifacts.
4. If protected runners are available, collect exact-candidate evidence in the
   correct environment. Missing/failed/`not_run` evidence remains explicit and
   supplemental; it is neither pass nor a new publication prerequisite.
5. Document the disabled behavior in release notes. Official packages still
   come only from the approved tag/staged-candidate workflow. A local build
   cannot replace a failed release job. This runbook authorizes no installation,
   signing-secret access, tag movement, publication or platform-state mutation.

Rollback does not dynamically raise TUN MTU, enable multipath, persist metric
history, upload diagnostics, or turn a local Doctor result into leak proof.
