//! Current-user Windows Named Pipe transport for the engine control service.

use std::{ffi::c_void, io, mem, ptr, sync::Arc};

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tracing::{debug, warn};
use windows_sys::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, LocalFree},
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SDDL_REVISION_1,
            },
            GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
            TOKEN_USER, TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    },
    core::PWSTR,
};

use crate::{ControlService, event_stream::handle_event_stream, ipc_stream::handle_stream};

const PIPE_PREFIX: &str = r"\\.\pipe\io.github.georgexie2333.usque.engine.v1";

pub fn current_user_pipe_name() -> io::Result<String> {
    Ok(format!("{PIPE_PREFIX}-{}", current_user_sid()?))
}

pub fn event_pipe_name(control_pipe_name: &str) -> io::Result<String> {
    if !control_pipe_name.starts_with(PIPE_PREFIX)
        || control_pipe_name.len() > 240
        || !control_pipe_name[PIPE_PREFIX.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "control pipe name is outside the Usque namespace",
        ));
    }
    Ok(format!("{control_pipe_name}.events"))
}

/// Serves authenticated protobuf requests until the future is cancelled.
///
/// Each pipe instance denies remote clients and has a protected DACL granting
/// full access only to the current user and Local System.
pub async fn serve(service: Arc<ControlService>, pipe_name: String) -> io::Result<()> {
    let mut next = create_user_pipe(&pipe_name, true)?;
    loop {
        next.connect().await?;
        let connected = next;
        next = create_user_pipe(&pipe_name, false)?;
        let service = Arc::clone(&service);
        tokio::spawn(async move {
            if let Err(error) = handle_stream(connected, service).await {
                warn!(%error, "Windows engine IPC client disconnected with an error");
            }
        });
    }
}

/// Serves the independent, read-only engine event stream. It uses the same
/// current-user/System DACL as control IPC, but a separate pipe so event
/// backpressure cannot block control requests.
pub async fn serve_events(service: Arc<ControlService>, pipe_name: String) -> io::Result<()> {
    let mut next = create_user_event_pipe(&pipe_name, true)?;
    loop {
        next.connect().await?;
        let connected = next;
        next = create_user_event_pipe(&pipe_name, false)?;
        let service = Arc::clone(&service);
        tokio::spawn(async move {
            if let Err(error) = handle_event_stream(connected, service).await {
                if matches!(
                    error.kind(),
                    io::ErrorKind::BrokenPipe
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::UnexpectedEof
                ) {
                    debug!(%error, "Windows engine event subscriber disconnected");
                } else {
                    warn!(%error, "Windows engine event subscriber disconnected with an error");
                }
            }
        });
    }
}

fn create_user_pipe(pipe_name: &str, first_instance: bool) -> io::Result<NamedPipeServer> {
    create_user_pipe_with_access(pipe_name, first_instance, true)
}

fn create_user_event_pipe(pipe_name: &str, first_instance: bool) -> io::Result<NamedPipeServer> {
    create_user_pipe_with_access(pipe_name, first_instance, false)
}

fn create_user_pipe_with_access(
    pipe_name: &str,
    first_instance: bool,
    inbound: bool,
) -> io::Result<NamedPipeServer> {
    let sid = current_user_sid()?;
    let descriptor = SecurityDescriptor::for_user(&sid)?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first_instance)
        .reject_remote_clients(true);
    if !inbound {
        // Event IPC is intentionally server-to-client only. A read-only
        // client handle must not look like an EOF on a server read half.
        options.access_inbound(false).access_outbound(true);
    }

    // SAFETY: `attributes` and the owned self-relative security descriptor
    // remain valid for the complete CreateNamedPipeW call. Windows copies the
    // descriptor into the created kernel object before this function returns.
    unsafe {
        options.create_with_security_attributes_raw(
            pipe_name,
            (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
        )
    }
}

fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: token points to writable storage and GetCurrentProcess returns a
    // valid pseudo-handle for the calling process.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedHandle(token);

    let mut required = 0_u32;
    // The first call determines the variable-sized TOKEN_USER buffer.
    unsafe {
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut required);
    }
    if required == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut buffer = vec![0_u8; required as usize];
    // SAFETY: the buffer has exactly the size requested by Windows and remains
    // alive while TOKEN_USER and its embedded SID are inspected.
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful TokenUser query starts with a valid TOKEN_USER.
    let token_user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };

    let mut sid_text: PWSTR = ptr::null_mut();
    // SAFETY: the SID belongs to the live token buffer and sid_text is a valid
    // output pointer. The returned LocalAlloc buffer is owned below.
    if unsafe { ConvertSidToStringSidW(token_user.User.Sid, &mut sid_text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let sid_text = LocalAllocation(sid_text.cast());
    wide_null_to_string(sid_text.0.cast())
}

fn wide_null_to_string(value: *const u16) -> io::Result<String> {
    if value.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows returned a null SID string",
        ));
    }
    let mut length = 0_usize;
    // A textual Windows SID is far shorter than this defensive bound.
    while length < 256 {
        // SAFETY: the source was returned as a null-terminated PWSTR and the
        // defensive bound prevents unbounded reads if Windows violates it.
        if unsafe { *value.add(length) } == 0 {
            // SAFETY: all `length` UTF-16 units precede the terminator.
            let units = unsafe { std::slice::from_raw_parts(value, length) };
            return String::from_utf16(units)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
        }
        length += 1;
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Windows returned an overlong SID string",
    ))
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    fn for_user(sid: &str) -> io::Result<Self> {
        if !is_sid_text(sid) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "current-user SID contains unexpected characters",
            ));
        }
        let sddl = format!("O:{sid}D:P(A;;GA;;;SY)(A;;GA;;;{sid})");
        let sddl: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: sddl is null-terminated and descriptor points to writable
        // storage. The resulting LocalAlloc allocation is owned by Self.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(descriptor))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: ConvertStringSecurityDescriptor... allocates with
            // LocalAlloc and ownership has not been transferred.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}

fn is_sid_text(value: &str) -> bool {
    value.starts_with("S-")
        && value.len() <= 256
        && value[2..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
}

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this pointer was allocated by a Windows function whose
            // contract requires LocalFree.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle was returned by OpenProcessToken and is closed
            // exactly once here.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;
    use tempfile::tempdir;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::windows::named_pipe::ClientOptions,
    };
    use usque_core::storage::ConfigStore;
    use usque_ipc::{
        decode_frame, encode_frame,
        v1::{ControlRequest, ControlResponse, GetStatusRequest, control_request},
    };
    use uuid::Uuid;

    use super::*;

    #[test]
    fn current_sid_builds_a_protected_descriptor_and_user_scoped_name() {
        let sid = current_user_sid().expect("current SID");
        assert!(is_sid_text(&sid));
        let _descriptor = SecurityDescriptor::for_user(&sid).expect("security descriptor");
        assert_eq!(
            current_user_pipe_name().expect("pipe name"),
            format!("{PIPE_PREFIX}-{sid}")
        );
    }

    #[tokio::test]
    async fn current_user_can_round_trip_a_framed_request() {
        let directory = tempdir().expect("tempdir");
        let service = Arc::new(
            ControlService::open(ConfigStore::new(directory.path().join("config.json")))
                .expect("service"),
        );
        let pipe_name = format!("{PIPE_PREFIX}.test-{}", Uuid::new_v4());
        let server = create_user_pipe(&pipe_name, true).expect("server");
        let mut client = ClientOptions::new().open(&pipe_name).expect("client");
        server.connect().await.expect("connect");
        let task = tokio::spawn(handle_stream(server, service));

        let request = ControlRequest {
            request_id: "pipe-test".to_owned(),
            payload: Some(control_request::Payload::GetStatus(GetStatusRequest {})),
        };
        client
            .write_all(&encode_frame(&request).expect("encode"))
            .await
            .expect("write");

        let mut header = [0_u8; 4];
        client.read_exact(&mut header).await.expect("header");
        let payload_len = u32::from_be_bytes(header) as usize;
        let mut payload = vec![0_u8; payload_len];
        client.read_exact(&mut payload).await.expect("payload");
        let mut frame = BytesMut::from(header.as_slice());
        frame.extend_from_slice(&payload);
        let response: ControlResponse = decode_frame(frame.freeze()).expect("decode");
        assert_eq!(response.request_id, "pipe-test");
        assert!(response.error.is_none(), "{:?}", response.error);

        client.shutdown().await.expect("shutdown");
        drop(client);
        task.await.expect("join").expect("connection");
    }

    #[test]
    fn sid_validation_rejects_sddl_injection() {
        assert!(is_sid_text("S-1-5-21-1001"));
        assert!(!is_sid_text("S-1-5-21-1001)(A;;GA;;;WD"));
        assert!(!is_sid_text("not-a-sid"));
    }
}
