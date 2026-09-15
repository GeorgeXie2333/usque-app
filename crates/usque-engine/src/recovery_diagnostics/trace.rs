//! Independent export allowlist: never serialize the Agent's trace wholesale.
use super::{Value, agent_v1, json, observation};
use agent_v1::{RecoveryHistoryStatus as Status, RecoveryTraceEvent, RecoveryTraceStage as Stage};

const MAX_EVENTS: usize = 128;
// Reserve room for the separately bounded 32-step history and current sample.
// The complete pretty-printed windows-recovery.json remains below 256 KiB.
const MAX_EVENTS_JSON_BYTES: usize = 128 * 1024;

pub(super) fn summary(trace: Option<&agent_v1::RecoveryTrace>, availability: &str) -> Value {
    let Some(trace) = trace else {
        return json!({
            "schema_version": 1,
            "availability": if availability == "available" { "extension_unavailable" } else { availability },
            "status": "unavailable",
            "events": [],
            "dropped_events": null,
            "write_failures": null,
            "truncated": false,
        });
    };
    let readable = matches!(
        Status::try_from(trace.status),
        Ok(Status::Complete | Status::Partial)
    );
    let mut rows = Vec::new();
    let mut size = 0usize;
    let mut invalid = false;
    let mut truncated = false;
    if readable {
        for event in trace.events.iter().rev() {
            let Some(row) = event_summary(event) else {
                invalid = true;
                continue;
            };
            // Account for indentation and separators added by the parent JSON.
            let bytes = serde_json::to_vec_pretty(&row).expect("allowlisted JSON values");
            let indented_size =
                bytes.len() + 6 * bytes.iter().filter(|b| **b == b'\n').count() + 16;
            if rows.len() == MAX_EVENTS
                || size.saturating_add(indented_size) > MAX_EVENTS_JSON_BYTES
            {
                truncated = true;
                break;
            }
            size += indented_size;
            rows.push(row);
        }
    }
    rows.reverse();
    json!({
        "schema_version": 1,
        "availability": if readable { "available" } else { "unavailable" },
        "status": if invalid || truncated { "partial".to_owned() } else {
            label!(RecoveryHistoryStatus, trace.status, "RECOVERY_HISTORY_STATUS_")
        },
        "events": rows,
        "dropped_events": trace.dropped_events,
        "write_failures": trace.write_failures,
        "truncated": truncated,
    })
}

fn event_summary(event: &RecoveryTraceEvent) -> Option<Value> {
    let stage = Stage::try_from(event.stage).ok()?;
    if stage == Stage::Unspecified
        || event.schema_version != 1
        || event.agent_run_id == 0
        || event.event_sequence == 0
        || event.occurred_at_unix_ms == 0
    {
        return None;
    }
    let native = matches!(
        stage,
        Stage::NativeRemoveFailed
            | Stage::NativeOrphanRemoveFailed
            | Stage::NativeOrphanRemoved
            | Stage::NativeOther
    );
    if !native && (event.journal_generation == 0 || event.resource_id == 0) {
        return None;
    }
    let sample = if matches!(stage, Stage::ObservationChanged | Stage::ObservationAbsent) {
        let sample = observation(event.observation.as_ref()?, event.journal_generation);
        if stage == Stage::ObservationAbsent
            && (sample["status"] != "complete"
                || sample["interface"]["presence"] != "absent"
                || sample["pnp_device"]["presence"] != "absent")
        {
            return None;
        }
        Some(sample)
    } else {
        None
    };
    Some(json!({
        "schema_version": 1,
        "agent_run_id": event.agent_run_id,
        "event_sequence": event.event_sequence,
        "occurred_at_unix_ms": event.occurred_at_unix_ms,
        "monotonic_ms": event.monotonic_ms,
        "journal_generation": event.journal_generation,
        "resource_id": event.resource_id,
        "stage": label!(RecoveryTraceStage, event.stage, "RECOVERY_TRACE_STAGE_"),
        "elapsed_ms": event.elapsed_ms.filter(|_| matches!(stage, Stage::PumpJoinReturned | Stage::EndSessionReturned | Stage::CloseAdapterReturned)),
        "reference_count": event.reference_count.filter(|n| stage == Stage::AdapterReleaseRequested && (1..=1_000_000).contains(n)),
        "succeeded": if stage == Stage::PumpJoinReturned { event.succeeded } else { None },
        "observation": sample,
        "native_level": event.native_level.filter(|n| native && (1..=3).contains(n)),
        "dropped_events": event.dropped_events,
        "write_failures": event.write_failures,
    }))
}
