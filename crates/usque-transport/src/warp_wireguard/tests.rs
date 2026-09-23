use super::*;
use crate::RuntimePath;
use usque_core::{AddressFamily, Transport};
use zeroize::Zeroizing;

struct MetadataFixture;
impl store::ProfileCipher for MetadataFixture {
    fn seal(&self, _: uuid::Uuid, bytes: &[u8]) -> Result<Vec<u8>, ImportError> {
        Ok(bytes.to_vec())
    }
    fn open(&self, _: uuid::Uuid, bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>, ImportError> {
        Ok(Zeroizing::new(bytes.to_vec()))
    }
}
fn context(profile: Profile) -> Context {
    Context {
        profile,
        existing: None,
        identity: None,
        protector: Arc::new(crate::NoopSocketProtector),
        refresher: None,
        allow_physical: false,
    }
}
#[test]
fn restart_requires_manual_resume_and_terminal_jobs_cannot_resume() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("profiles-v2.json");
    let profile = Profile::default();
    let request = Request::parse(r#"{"action":"start"}"#).unwrap();
    let job = Job::new(
        &request,
        usque_core::warp_wireguard::context(&profile).unwrap(),
    )
    .unwrap();
    Store::new(directory.path(), &MetadataFixture)
        .insert(&job)
        .unwrap();
    let manager = Arc::new(Manager::new(path, Arc::new(MetadataFixture)));
    let recovered = manager
        .command(Request::parse(r#"{"action":"get"}"#).unwrap(), None)
        .unwrap();
    assert_eq!(recovered.job.unwrap().state, "paused");
    assert!(manager.running.lock().unwrap().is_none());
    let cancel =
        Request::parse(&format!(r#"{{"action":"cancel","job_id":"{}"}}"#, job.id)).unwrap();
    assert_eq!(
        manager.command(cancel, None).unwrap().job.unwrap().state,
        "cancelled"
    );
    let resume =
        Request::parse(&format!(r#"{{"action":"resume","job_id":"{}"}}"#, job.id)).unwrap();
    assert_eq!(
        manager
            .command(resume, Some(context(profile)))
            .err()
            .unwrap()
            .reason,
        "cannot_resume"
    );
}

#[test]
fn terminal_failure_never_commits_volatile_progress() {
    let directory = tempfile::tempdir().unwrap();
    let manager = Manager::new(
        directory.path().join("profiles-v2.json"),
        Arc::new(MetadataFixture),
    );
    let request = Request::parse(r#"{"action":"start"}"#).unwrap();
    let mut job = Job::new(&request, "context".into()).unwrap();
    manager.store().insert(&job).unwrap();
    job.next = 64;
    job.working = 64;
    manager.record_failure(&mut job, "failed", "secure_storage_failed");
    assert_eq!(job.next, 0);
    let saved = manager.store().job(job.id).unwrap();
    assert_eq!(saved.next, 0);
    assert_eq!(saved.state, "failed");
}
#[test]
fn unavailable_underlay_stops_the_worker_without_any_physical_fallback() {
    let directory = tempfile::tempdir().unwrap();
    let manager = Arc::new(Manager::new(
        directory.path().join("profiles-v2.json"),
        Arc::new(MetadataFixture),
    ));
    let response = manager
        .command(
            Request::parse(r#"{"action":"start"}"#).unwrap(),
            Some(context(Profile::default())),
        )
        .unwrap();
    assert!(response.job.is_some());
    assert!(manager.stop_blocking());
    let response = manager
        .command(Request::parse(r#"{"action":"get"}"#).unwrap(), None)
        .unwrap();
    assert_ne!(response.job.unwrap().state, "running");
    assert!(response.results.is_empty());
    let id = response.history[0].id;
    let cancelled = manager
        .command(
            Request::parse(&format!(r#"{{"action":"cancel","job_id":"{id}"}}"#)).unwrap(),
            None,
        )
        .unwrap();
    assert_eq!(cancelled.job.unwrap().state, "cancelled");
}
#[tokio::test(start_paused = true)]
async fn repeated_healthy_notifications_are_ignored_but_reconnections_invalidate_scan() {
    let baseline = RuntimeHealth::Connected {
        path: RuntimePath {
            transport: Transport::Http2,
            endpoint_family: AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: true,
        },
        reconnect_count: 0,
    };
    let (sender, mut receiver) = tokio::sync::watch::channel(baseline.clone());
    sender.send_replace(baseline.clone());
    assert!(
        tokio::time::timeout(
            Duration::from_millis(5),
            underlay_changed(&mut receiver, &baseline)
        )
        .await
        .is_err()
    );
    sender.send_modify(|health| {
        if let RuntimeHealth::Connected {
            reconnect_count, ..
        } = health
        {
            *reconnect_count += 1;
        }
    });
    tokio::time::timeout(
        Duration::from_secs(1),
        underlay_changed(&mut receiver, &baseline),
    )
    .await
    .unwrap();
}
