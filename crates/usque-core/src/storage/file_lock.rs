use std::fs::File;
use std::io;

// Rust 1.97.1's File::lock returns Unsupported on Android. Call flock directly
// on Unix so Android and the Unix host tests use the same implementation. The
// lock belongs to the open file description and is released when it is closed,
// including on process exit; never delete or atomically replace the sidecar.
#[cfg(unix)]
pub(super) fn lock_exclusive(file: &File) -> io::Result<()> {
    flock(file, libc::LOCK_EX)
}

#[cfg(not(unix))]
pub(super) fn lock_exclusive(file: &File) -> io::Result<()> {
    file.lock()
}

#[cfg(unix)]
fn flock(file: &File, operation: libc::c_int) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    retry_interrupted(|| {
        // SAFETY: file keeps its descriptor open for the entire call. flock
        // does not retain pointers or take ownership of the descriptor.
        if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    })
}

#[cfg(any(unix, test))]
fn retry_interrupted(mut operation: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    loop {
        match operation() {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

#[cfg(test)]
pub(super) fn try_lock_exclusive(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        flock(file, libc::LOCK_EX | libc::LOCK_NB)
    }
    #[cfg(not(unix))]
    {
        file.try_lock().map_err(io::Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_lock_is_retried() {
        let mut calls = 0;
        retry_interrupted(|| {
            calls += 1;
            if calls < 3 {
                Err(io::ErrorKind::Interrupted.into())
            } else {
                Ok(())
            }
        })
        .unwrap();
        assert_eq!(calls, 3);
    }

    #[test]
    fn lock_failures_are_not_ignored_or_retried() {
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::Unsupported,
            io::ErrorKind::WouldBlock,
        ] {
            let mut calls = 0;
            let error = retry_interrupted(|| {
                calls += 1;
                Err(kind.into())
            })
            .unwrap_err();
            assert_eq!(error.kind(), kind);
            assert_eq!(calls, 1);
        }
    }
}
