use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn recorder_coalesces_requests_stops_polling_and_flushes_at_shutdown() {
    let directory = tempfile::tempdir().unwrap();
    let (sender, receiver) = mpsc::channel(1);
    let stop = CancellationToken::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let (samples, mut sampled) = mpsc::unbounded_channel();
    let task = tokio::spawn(run(
        directory.path().to_owned(),
        receiver,
        stop.clone(),
        move || {
            count.fetch_add(1, Ordering::Relaxed);
            samples.send(()).unwrap();
            async { PlatformState::default() }
        },
    ));
    for _ in 0..100 {
        let _ = sender.try_send(());
    }
    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(5), sampled.recv())
            .await
            .unwrap()
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        calls.load(Ordering::Relaxed),
        2,
        "idle must not keep an Agent alive"
    );
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 3);
    assert!(
        !directory
            .path()
            .join(super::super::cache::FILE_NAME)
            .exists()
    );
}

#[test]
fn blocked_evidence_io_does_not_pin_runtime_shutdown() {
    let (release, blocked) = std::sync::mpsc::channel();
    let (entered, started) = std::sync::mpsc::channel();
    let (finished, done) = std::sync::mpsc::channel();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        assert!(
            tokio::time::timeout(
                Duration::from_millis(30),
                write_evidence(move || {
                    entered.send(()).unwrap();
                    blocked.recv().unwrap();
                    finished.send(()).unwrap();
                    Ok(true)
                })
            )
            .await
            .is_err()
        );
    });
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let (stopped, stop_done) = std::sync::mpsc::channel();
    let shutdown = std::thread::spawn(move || {
        drop(runtime);
        stopped.send(()).unwrap();
    });
    let exited_without_writer = stop_done.recv_timeout(Duration::from_secs(1)).is_ok();
    // Always release/join the memory-only fixture, even if the assertion fails.
    release.send(()).unwrap();
    shutdown.join().unwrap();
    done.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        exited_without_writer,
        "diagnostic I/O must not hold the Engine runtime alive"
    );
}
