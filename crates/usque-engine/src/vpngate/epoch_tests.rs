//! Owner fencing exercises only cache files and synthetic jobs, never a VPN.
use super::*;
use usque_core::storage::ConfigStore;

fn fixture() -> (tempfile::TempDir, ControlService) {
    let directory = tempfile::tempdir().unwrap();
    let service = ControlService::open_with_vault(
        ConfigStore::new(directory.path().join("config.json")),
        Arc::new(crate::tests::MemoryVault::default()),
    )
    .unwrap();
    (directory, service)
}

fn request(epoch: impl ToString, action: &str) -> v1::VpnGateNodeRequest {
    v1::VpnGateNodeRequest {
        operation_id: uuid::Uuid::new_v4().to_string(),
        action: action.into(),
        server_id: format!("v1:{}", "a".repeat(64)),
        config_sha256: "b".repeat(64),
        expected_source_epoch: epoch.to_string(),
        ..Default::default()
    }
}

fn prepared_reference(service: &ControlService) -> std::path::PathBuf {
    let path = service.cache_dir.join("vpngate/prepared").join(format!(
        "{}-{}.json",
        "a".repeat(64),
        "b".repeat(64)
    ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Release removes this path without parsing it. A sentinel proves the
    // preceding owner's cleanup cannot touch a newer same-selection reference.
    std::fs::write(&path, b"new-owner-reference").unwrap();
    path
}

#[tokio::test]
async fn stale_and_malformed_epoch_reject_before_cache_or_worker_mutation() {
    let (_directory, service) = fixture();
    let reference = prepared_reference(&service);
    let current = service.settings.lock().await.source_epoch;
    for stale in [uuid::Uuid::new_v4().to_string(), "malformed".into()] {
        for action in ["prepare", "release", "cancel", "remove_favorite"] {
            let result = service.vpn_gate_node(request(&stale, action)).await;
            assert!(result.is_err(), "{action}");
            if stale == "malformed" {
                assert_eq!(
                    result.unwrap_err().as_structured_error().code,
                    "VPN_GATE_SELECTION_STALE"
                );
            } else {
                assert_eq!(
                    result.unwrap_err().as_structured_error().code,
                    "VPN_GATE_OWNER_STALE"
                );
            }
            assert_eq!(std::fs::read(&reference).unwrap(), b"new-owner-reference");
            assert!(service.gate_fetch_task.lock().await.is_none());
            assert_eq!(
                service.gate_directory.node_progress(),
                NodeProgress::default()
            );
            assert_eq!(service.settings.lock().await.source_epoch, current);
        }
    }
    // Both the correlated and legacy empty-epoch API still release references.
    service
        .vpn_gate_node(request(current, "release"))
        .await
        .unwrap();
    assert!(!reference.exists());
    let reference = prepared_reference(&service);
    service.vpn_gate_node(request("", "release")).await.unwrap();
    assert!(!reference.exists());
}

#[tokio::test]
async fn queued_cleanup_rechecks_epoch_after_owner_reset_before_cancelling_work() {
    let (_directory, service) = fixture();
    let old = service.settings.lock().await.source_epoch;
    let submission = service.settings_submission.lock().await;
    let release_service = service.clone();
    let release =
        tokio::spawn(async move { release_service.vpn_gate_node(request(old, "release")).await });
    tokio::task::yield_now().await;
    *service.settings.lock().await = Default::default();
    let current = service.settings.lock().await.source_epoch;
    let reference = prepared_reference(&service);
    drop(submission);
    assert!(matches!(
        release.await.unwrap(),
        Err(ControlServiceError::VpnGateOwnerStale)
    ));
    assert!(reference.exists());

    // A mismatched prepare must not cancel a job admitted by the new owner.
    let cancellation = CancellationToken::new();
    let cancelled = cancellation.clone();
    *service.gate_fetch_task.lock().await = Some(FetchTask {
        cancellation: cancellation.clone(),
        task: tokio::spawn(async move { cancelled.cancelled().await }),
    });
    assert!(matches!(
        service.vpn_gate_node(request(old, "prepare")).await,
        Err(ControlServiceError::VpnGateOwnerStale)
    ));
    assert!(!cancellation.is_cancelled());
    service
        .vpn_gate_node(request(current, "release"))
        .await
        .unwrap();
    assert!(!reference.exists());
    service.cancel_gate_refresh().await;
    assert!(cancellation.is_cancelled());
}
