//! Keep child creation separate from concurrent IPC lease and socket tests:
//! even CLOEXEC descriptors can be inherited between process creation and exec.

#![cfg(target_os = "linux")]

use std::{fs, time::Duration};
use usque_engine::linux_ipc::wait_for_parent_exit;

#[tokio::test]
async fn parent_wait_releases_its_pidfd_on_cancellation_and_exit() {
    fn pidfd_count(pid: u32) -> usize {
        let pid = pid.to_string();
        fs::read_dir("/proc/self/fdinfo")
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .filter(|info| {
                info.lines().any(|line| {
                    line.strip_prefix("Pid:")
                        .is_some_and(|value| value.trim() == pid)
                })
            })
            .count()
    }

    // Closing stdin makes this inert child exit; it opens no network socket
    // and requires neither a live Agent nor privileged platform changes.
    let mut child = std::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    assert_eq!(pidfd_count(pid), 0);
    for cancelled in [true, false] {
        let mut waiting = Box::pin(wait_for_parent_exit(Some(pid)));
        std::future::poll_fn(|context| {
            assert!(std::future::Future::poll(waiting.as_mut(), context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert_eq!(pidfd_count(pid), 1);
        if cancelled {
            drop(waiting);
        } else {
            drop(child.stdin.take());
            tokio::time::timeout(Duration::from_secs(3), waiting)
                .await
                .unwrap()
                .unwrap();
        }
        assert_eq!(pidfd_count(pid), 0);
    }
    child.wait().unwrap();
}
