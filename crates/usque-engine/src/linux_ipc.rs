//! Authenticated, bounded current-user Linux control and event sockets.

use crate::{
    ControlService, event_stream::handle_event_stream, ipc_stream::handle_stream_with_timeouts,
};
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::{
        fd::AsRawFd,
        unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::Semaphore,
    task::JoinSet,
};
use usque_platform::linux_paths::{effective_uid, private_directory};

const MAX_CLIENTS: usize = 32;
const MAX_SOCKET_PATH_BYTES: usize = 107;

/// Own both endpoints and the process lease before loading mutable user state.
/// A live instance can never be displaced by deleting its socket or lock file.
pub struct LinuxIpcServer {
    control: UnixListener,
    events: UnixListener,
    _control_path: SocketLease,
    _event_path: SocketLease,
    _lease: File,
    _directory: File,
}

impl LinuxIpcServer {
    pub async fn bind(socket: PathBuf) -> io::Result<Self> {
        let events = event_socket_path(&socket);
        for path in [&socket, &events] {
            if path.as_os_str().as_encoded_bytes().len() > MAX_SOCKET_PATH_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Linux IPC socket path is too long",
                ));
            }
        }
        let parent = socket.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "IPC socket has no parent")
        })?;
        let directory = private_directory(parent)?;
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(parent.join("engine.lock"))?;
        let metadata = lease.metadata()?;
        if !metadata.is_file() || metadata.uid() != effective_uid() || metadata.mode() & 0o077 != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Engine lease is not a private current-user file",
            ));
        }
        // SAFETY: lease owns an open regular-file descriptor and LOCK_NB never waits.
        if unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "a current-user Engine already owns the runtime directory",
            ));
        }
        let (control, control_path) = bind_socket(socket).await?;
        let (events, event_path) = bind_socket(events).await?;
        Ok(Self {
            control,
            events,
            _control_path: control_path,
            _event_path: event_path,
            _lease: lease,
            _directory: directory,
        })
    }

    pub async fn serve(self, service: Arc<ControlService>) -> io::Result<()> {
        let permits = Arc::new(Semaphore::new(MAX_CLIENTS));
        // Dropping the server aborts every accepted client, including a stalled
        // frame or event reader, before removing its own socket inodes.
        let mut clients = JoinSet::new();
        let mut accept_failed = false;
        loop {
            tokio::select! {
                incoming = self.control.accept() => {
                    let Some(stream) = accepted(incoming, &mut accept_failed).await? else { continue; };
                    if authenticate_peer(&stream).is_err() { continue; }
                    let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else { continue; };
                    let service = Arc::clone(&service);
                    clients.spawn(async move {
                        let _permit = permit;
                        handle_stream_with_timeouts(stream, service, Some(Duration::from_secs(120))).await
                    });
                }
                incoming = self.events.accept() => {
                    let Some(stream) = accepted(incoming, &mut accept_failed).await? else { continue; };
                    if authenticate_peer(&stream).is_err() { continue; }
                    let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else { continue; };
                    let service = Arc::clone(&service);
                    clients.spawn(async move {
                        let _permit = permit;
                        handle_event_stream(stream, service).await
                    });
                }
                _ = clients.join_next(), if !clients.is_empty() => {}
            }
        }
    }
}

/// A failed accept affects only that client. Descriptor or memory pressure and
/// aborted handshakes are retried after a pause instead of ending the service.
async fn accepted(
    incoming: io::Result<(UnixStream, tokio::net::unix::SocketAddr)>,
    failed: &mut bool,
) -> io::Result<Option<UnixStream>> {
    match incoming {
        Ok((stream, _)) => {
            *failed = false;
            Ok(Some(stream))
        }
        Err(error) if transient_accept_error(&error) => {
            if !*failed {
                tracing::warn!(kind = ?error.kind(), "Linux IPC connections cannot be accepted; retrying");
            }
            *failed = true;
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn transient_accept_error(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(
            libc::ECONNABORTED
                | libc::EMFILE
                | libc::ENFILE
                | libc::ENOBUFS
                | libc::ENOMEM
                | libc::EPROTO
                | libc::EPERM
                | libc::EINTR
        )
    )
}

pub fn event_socket_path(control: &Path) -> PathBuf {
    let mut path = control.as_os_str().to_os_string();
    path.push(".events");
    PathBuf::from(path)
}

fn authenticate_peer(stream: &UnixStream) -> io::Result<()> {
    // Tokio obtains SO_PEERCRED on Linux. Checking the kernel's effective UID
    // is independent of filesystem permission checks and client payloads.
    let credentials = stream.peer_cred()?;
    if credentials.uid() != effective_uid() || credentials.pid().is_none_or(|pid| pid <= 0) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Linux IPC peer does not belong to the current user",
        ));
    }
    Ok(())
}

async fn bind_socket(path: PathBuf) -> io::Result<(UnixListener, SocketLease)> {
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.file_type().is_socket() || metadata.uid() != effective_uid() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "refusing to replace a foreign or non-socket IPC path",
                ));
            }
            match tokio::time::timeout(Duration::from_millis(500), UnixStream::connect(&path)).await
            {
                Ok(Err(error)) if error.kind() == io::ErrorKind::ConnectionRefused => {}
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "IPC socket still has a live listener",
                    ));
                }
            }
            // Keep ownership identity stable across the liveness probe.
            let current = fs::symlink_metadata(&path)?;
            if current.dev() != metadata.dev() || current.ino() != metadata.ino() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "IPC path changed during stale socket recovery",
                ));
            }
            fs::remove_file(&path)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(&path)?;
    let metadata = fs::symlink_metadata(&path)?;
    let lease = SocketLease {
        path,
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    fs::set_permissions(&lease.path, fs::Permissions::from_mode(0o600))?;
    Ok((listener, lease))
}

struct SocketLease {
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl Drop for SocketLease {
    fn drop(&mut self) {
        if let Ok(metadata) = fs::symlink_metadata(&self.path)
            && metadata.file_type().is_socket()
            && metadata.uid() == effective_uid()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// pidfd refers to one process lifetime; PID reuse cannot attach the sidecar
/// to a different user or a newly spawned process after the GUI exits.
pub async fn wait_for_parent_exit(parent_pid: Option<u32>) -> io::Result<()> {
    let Some(pid) = parent_pid else {
        return std::future::pending().await;
    };
    if pid == 0 || pid > i32::MAX as u32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid desktop parent PID",
        ));
    }
    // SAFETY: pidfd_open accepts a validated numeric PID and flags zero; success
    // returns an owned descriptor. It does not mutate the referenced process.
    let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
    if descriptor < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err(error);
    }
    use std::os::fd::FromRawFd;
    // SAFETY: the successful syscall returned a fresh descriptor owned here.
    let descriptor = unsafe { File::from_raw_fd(descriptor as i32) };
    let descriptor = tokio::io::unix::AsyncFd::new(descriptor)?;
    let _ready = descriptor.readable().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use usque_core::storage::ConfigStore;
    use usque_ipc::{
        decode_frame, encode_frame,
        v1::{ControlRequest, ControlResponse, GetStatusRequest, control_request},
    };

    #[tokio::test(start_paused = true)]
    async fn transient_accept_failures_skip_one_client_without_ending_the_service() {
        let mut failed = false;
        for code in [libc::EMFILE, libc::ENFILE, libc::ECONNABORTED, libc::ENOMEM] {
            assert!(
                accepted(Err(io::Error::from_raw_os_error(code)), &mut failed)
                    .await
                    .unwrap()
                    .is_none()
            );
            assert!(failed);
        }
        for code in [libc::EBADF, libc::EINVAL] {
            assert!(
                accepted(Err(io::Error::from_raw_os_error(code)), &mut failed)
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn owned_socket_round_trips_events_and_rejects_a_second_engine() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("ipc/engine.sock");
        let server = LinuxIpcServer::bind(path.clone()).await.unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            fs::metadata(path.parent().unwrap()).unwrap().mode() & 0o777,
            0o700
        );
        assert_eq!(
            LinuxIpcServer::bind(path.clone())
                .await
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::AddrInUse
        );
        let service = Arc::new(
            ControlService::open(ConfigStore::new(root.path().join("config.json"))).unwrap(),
        );
        let task = tokio::spawn(server.serve(service));
        let mut stream = UnixStream::connect(&path).await.unwrap();
        authenticate_peer(&stream).unwrap();
        stream
            .write_all(
                &encode_frame(&ControlRequest {
                    request_id: "linux-ipc".into(),
                    payload: Some(control_request::Payload::GetStatus(GetStatusRequest {})),
                })
                .unwrap(),
            )
            .await
            .unwrap();
        let mut header = [0; 4];
        stream.read_exact(&mut header).await.unwrap();
        let mut frame = header.to_vec();
        frame.resize(4 + u32::from_be_bytes(header) as usize, 0);
        stream.read_exact(&mut frame[4..]).await.unwrap();
        let response: ControlResponse = decode_frame(frame.into()).unwrap();
        assert_eq!(response.request_id, "linux-ipc");
        let mut events = UnixStream::connect(event_socket_path(&path)).await.unwrap();
        events.read_exact(&mut header).await.unwrap();
        assert!(u32::from_be_bytes(header) > 0);
        task.abort();
        let _ = task.await;
        assert!(!path.exists());
        assert!(!event_socket_path(&path).exists());
        LinuxIpcServer::bind(path).await.unwrap();
    }

    #[tokio::test]
    async fn stale_recovery_never_replaces_a_regular_file_or_live_listener() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("engine.sock");
        fs::write(&path, b"preserve").unwrap();
        assert!(bind_socket(path.clone()).await.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"preserve");
        fs::remove_file(&path).unwrap();
        let listener = UnixListener::bind(&path).unwrap();
        assert_eq!(
            bind_socket(path.clone()).await.err().unwrap().kind(),
            io::ErrorKind::AddrInUse
        );
        drop(listener);
        let (_listener, lease) = bind_socket(path.clone()).await.unwrap();
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"replacement").unwrap();
        drop(lease);
        assert_eq!(fs::read(&path).unwrap(), b"replacement");
    }
}
