//! A bounded join never detaches a still-live runtime or makes it reusable.
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub(super) fn wait_finished(thread: &JoinHandle<()>, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if thread.is_finished() {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        std::thread::sleep((deadline - now).min(Duration::from_millis(5)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_retains_the_worker_for_a_later_confirmed_stop() {
        let (release, blocked) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = blocked.recv();
        });
        assert!(!wait_finished(&worker, Duration::from_millis(10)));
        assert!(!worker.is_finished());
        release.send(()).unwrap();
        assert!(wait_finished(&worker, Duration::from_secs(2)));
        worker.join().unwrap();
    }
}
