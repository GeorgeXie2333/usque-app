//! GUI-supplied, bounded startup observations. Never platform health evidence.
use serde_json::{Value, json};
use usque_ipc::v1::{GuiStartupObservation, GuiStartupSource, GuiStartupStage, GuiStartupStatus};

pub(crate) fn summary(observation: Option<&GuiStartupObservation>) -> Value {
    let Some(value) = observation else {
        return json!({"schema_version":1,"availability":"unavailable","source":"gui_process"});
    };
    let mut seen = std::collections::BTreeSet::new();
    let valid = value.schema_version == 1
        && value.observation_window_ms <= 60_000
        && value.dart_max_gap_ms.is_none_or(|n| n <= 60_000)
        && value.gtk_max_gap_ms.is_none_or(|n| n <= 60_000)
        && value.stages.len() <= 16
        && value.stages.iter().all(|entry| {
            GuiStartupStage::try_from(entry.stage).is_ok_and(|s| s != GuiStartupStage::Unspecified)
                && GuiStartupSource::try_from(entry.source)
                    .is_ok_and(|s| s != GuiStartupSource::Unspecified)
                && GuiStartupStatus::try_from(entry.status)
                    .is_ok_and(|s| s != GuiStartupStatus::Unspecified)
                && entry.elapsed_ms.is_none_or(|n| n <= 60_000)
                && (entry.status == GuiStartupStatus::Pending as i32) == entry.elapsed_ms.is_none()
                && seen.insert((entry.source, entry.stage))
        });
    if !valid {
        return json!({"schema_version":1,"availability":"invalid","source":"gui_process"});
    }
    json!({
        "schema_version":1,"availability":"available","source":"gui_process",
        "observation_window_ms":value.observation_window_ms,
        "observation_complete":value.observation_complete,
        "dart_max_gap_ms":value.dart_max_gap_ms,"gtk_max_gap_ms":value.gtk_max_gap_ms,
        "stages":value.stages.iter().map(|entry| json!({
            "stage":GuiStartupStage::try_from(entry.stage).expect("validated").as_str_name(),
            "source":GuiStartupSource::try_from(entry.source).expect("validated").as_str_name(),
            "status":GuiStartupStatus::try_from(entry.status).expect("validated").as_str_name(),
            "elapsed_ms":entry.elapsed_ms,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use usque_ipc::v1::GuiStartupStageObservation;

    #[test]
    fn unavailable_and_invalid_input_cannot_claim_responsive_startup() {
        assert_eq!(summary(None)["availability"], "unavailable");
        let mut input = GuiStartupObservation {
            schema_version: 1,
            observation_window_ms: 1000,
            dart_max_gap_ms: Some(300),
            stages: vec![GuiStartupStageObservation {
                stage: GuiStartupStage::PreferencesRead as i32,
                source: GuiStartupSource::Dart as i32,
                status: GuiStartupStatus::Completed as i32,
                elapsed_ms: Some(500),
            }],
            ..Default::default()
        };
        assert_eq!(summary(Some(&input))["availability"], "available");
        assert!(summary(Some(&input))["gtk_max_gap_ms"].is_null());
        input.stages[0].stage = 999;
        assert_eq!(summary(Some(&input))["availability"], "invalid");
        input.stages[0].stage = GuiStartupStage::PreferencesRead as i32;
        input.stages.push(input.stages[0]);
        assert_eq!(summary(Some(&input))["availability"], "invalid");
        input.stages.pop();
        input.dart_max_gap_ms = Some(60_001);
        assert_eq!(summary(Some(&input))["availability"], "invalid");
    }
}
