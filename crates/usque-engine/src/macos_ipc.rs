//! Current-user Unix Domain Socket transport for the macOS engine.

use std::{
    fs, io,
    os::{
        fd::AsRawFd,
        unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::net::{UnixListener, UnixStream};
use tracing::warn;

use crate::{ControlService, ipc_stream::handle_stream};

const MAX_SOCKET_PATH_BYTES: usize = 100;

/// Serves protobuf control requests on a user-owned `0600` Unix socket.
///
/// The parent directory is forced to `0700`; every accepted peer is also
/// checked with `getpeereid`, rather than relying on path permissions alone.
pub async fn serve(service: Arc<ControlService>, socket_path: PathBuf) -> io::Result<()> {
    prepare_socket_parent(&socket_path)?;
    remove_owned_stale_socket(&socket_path)?;
    let listener = UnixListener::bind(&socket_path)?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
    let _cleanup = SocketCleanup(socket_path);
    let expected_uid = effective_user_id();

    loop {
        let (stream, _) = listener.accept().await?;
        match peer_user_id(&stream) {
            Ok(uid) if uid == expected_uid => {
                let service = Arc::clone(&service);
                tokio::spawn(async move {
                    if let Err(error) = handle_stream(stream, service).await {
                        warn!(%error, "macOS engine IPC client disconnected with an error");
                    }
                });
            }
            Ok(uid) => {
                warn!(
                    peer_uid = uid,
                    expected_uid, "rejected Unix Socket client owned by another user"
                );
            }
            Err(error) => warn!(%error, "rejected Unix Socket client without peer credentials"),
        }
    }
}

fn prepare_socket_parent(socket_path: &Path) -> io::Result<()> {
    if socket_path.as_os_str().as_encoded_bytes().len() > MAX_SOCKET_PATH_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unix Socket path is too long",
        ));
    }
    let parent = socket_path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Unix Socket has no parent"))?;
    fs::create_dir_all(parent)?;
    let metadata = fs::symlink_metadata(parent)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != effective_user_id()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Unix Socket parent is not a current-user-owned directory",
        ));
    }
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
}

fn remove_owned_stale_socket(socket_path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(socket_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_socket() || metadata.uid() != effective_user_id() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to replace a non-socket or foreign-owned IPC path",
        ));
    }
    fs::remove_file(socket_path)
}

fn peer_user_id(stream: &UnixStream) -> io::Result<u32> {
    let mut effective_uid = 0;
    let mut effective_gid = 0;
    // SAFETY: the stream owns a valid socket descriptor and both output
    // pointers remain writable for the duration of getpeereid.
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut effective_uid, &mut effective_gid) } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(effective_uid)
}

fn effective_user_id() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}

struct SocketCleanup(PathBuf);

impl Drop for SocketCleanup {
    fn drop(&mut self) {
        if let Ok(metadata) = fs::symlink_metadata(&self.0)
            && metadata.file_type().is_socket()
            && metadata.uid() == effective_user_id()
        {
            let _ = fs::remove_file(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use usque_core::storage::ConfigStore;
    use usque_ipc::{
        decode_frame, encode_frame,
        v1::{ControlRequest, ControlResponse, GetStatusRequest, control_request},
    };

    use super::*;

    #[tokio::test]
    async fn current_user_socket_round_trips_and_is_private() {
        let directory = tempfile::tempdir().expect("tempdir");
        let app_directory = directory.path().join("Usque");
        let socket_path = app_directory.join("engine.sock");
        prepare_socket_parent(&socket_path).expect("parent");
        let listener = UnixListener::bind(&socket_path).expect("listener");
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600)).expect("permissions");

        let service = Arc::new(
            ControlService::open(ConfigStore::new(app_directory.join("config.json")))
                .expect("service"),
        );
        let client = UnixStream::connect(&socket_path);
        let server = listener.accept();
        let (client, server) = tokio::join!(client, server);
        let mut client = client.expect("client");
        let (server, _) = server.expect("server");
        assert_eq!(peer_user_id(&server).expect("peer"), effective_user_id());
        assert_eq!(
            fs::metadata(&socket_path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let task = tokio::spawn(handle_stream(server, service));

        let request = ControlRequest {
            request_id: "socket-test".to_owned(),
            payload: Some(control_request::Payload::GetStatus(GetStatusRequest {})),
        };
        client
            .write_all(&encode_frame(&request).expect("encode"))
            .await
            .expect("write");
        let mut header = [0_u8; 4];
        client.read_exact(&mut header).await.expect("header");
        let mut payload = vec![0_u8; u32::from_be_bytes(header) as usize];
        client.read_exact(&mut payload).await.expect("payload");
        let mut frame = bytes::BytesMut::from(header.as_slice());
        frame.extend_from_slice(&payload);
        let response: ControlResponse = decode_frame(frame.freeze()).expect("decode");
        assert_eq!(response.request_id, "socket-test");
        assert!(response.error.is_none());

        client.shutdown().await.expect("shutdown");
        drop(client);
        task.await.expect("join").expect("connection");
        fs::remove_file(socket_path).expect("cleanup");
    }

    #[test]
    fn refuses_to_replace_a_regular_file() {
        let directory = tempfile::tempdir().expect("tempdir");
        let socket_path = directory.path().join("engine.sock");
        fs::write(&socket_path, b"do not delete").expect("fixture");
        assert_eq!(
            remove_owned_stale_socket(&socket_path)
                .expect_err("must reject")
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(fs::read(socket_path).expect("preserved"), b"do not delete");
    }
}
