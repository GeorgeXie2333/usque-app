use super::*;
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
fn manager(path: &std::path::Path) -> Arc<Manager> {
    Arc::new(Manager::new(
        path.join("profiles-v2.json"),
        Arc::new(MetadataFixture),
    ))
}

fn track(manager: &Manager, job: Job, done: bool) -> CancellationToken {
    let cancel = CancellationToken::new();
    *manager.running.lock().unwrap() = Some(Running {
        job: Arc::new(Mutex::new(job)),
        cancel: cancel.clone(),
        done: Arc::new(AtomicBool::new(done)),
    });
    cancel
}

#[test]
fn generation_status_is_process_local_and_never_reads_old_disk_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("warp-wireguard");
    std::fs::write(&path, b"opaque old data").unwrap();
    let first = manager(directory.path());
    let mut job = Job::generation();
    let id = job.id;
    job.state = "completed".into();
    track(&first, job, true);
    let response = first
        .command(Request::parse(r#"{"action":"get"}"#).unwrap(), None)
        .unwrap();
    assert_eq!(response.job.unwrap().id, id);
    let second = manager(directory.path());
    assert!(
        second
            .command(Request::parse(r#"{"action":"get"}"#).unwrap(), None)
            .unwrap()
            .job
            .is_none()
    );
    for action in ["get", "cancel"] {
        let request =
            Request::parse(&format!(r#"{{"action":"{action}","job_id":"{id}"}}"#)).unwrap();
        assert_eq!(
            second.command(request, None).err().unwrap().reason,
            "job_not_found"
        );
    }
    assert_eq!(std::fs::read(path).unwrap(), b"opaque old data");
}

#[test]
fn cancellation_targets_only_the_current_generation_and_duplicate_generation_is_blocked() {
    let directory = tempfile::tempdir().unwrap();
    let manager = manager(directory.path());
    let job = Job::generation();
    let id = job.id;
    let cancel = track(&manager, job, false);
    let wrong = Request::parse(&format!(
        r#"{{"action":"cancel","job_id":"{}"}}"#,
        uuid::Uuid::new_v4()
    ))
    .unwrap();
    assert_eq!(
        manager.command(wrong, None).err().unwrap().reason,
        "job_not_found"
    );
    assert!(!cancel.is_cancelled());
    let generate = Request::parse(r#"{"action":"generate"}"#).unwrap();
    assert_eq!(
        manager.command(generate, None).err().unwrap().reason,
        "generation_busy"
    );
    manager
        .command(
            Request::parse(&format!(r#"{{"action":"cancel","job_id":"{id}"}}"#)).unwrap(),
            None,
        )
        .unwrap();
    assert!(cancel.is_cancelled());
    assert!(directory.path().read_dir().unwrap().next().is_none());
}

#[test]
fn unavailable_underlay_stops_generation_without_any_physical_fallback() {
    let directory = tempfile::tempdir().unwrap();
    let manager = manager(directory.path());
    let response = manager
        .command(
            Request::parse(r#"{"action":"generate"}"#).unwrap(),
            Some(context(Profile::default())),
        )
        .unwrap();
    assert!(response.job.is_some());
    assert!(manager.stop_blocking());
    let job = manager
        .command(Request::parse(r#"{"action":"get"}"#).unwrap(), None)
        .unwrap()
        .job
        .unwrap();
    assert!(matches!(job.state.as_str(), "failed" | "cancelled"));
    assert!(job.profile_id.is_none());
    assert!(directory.path().read_dir().unwrap().next().is_none());
}
