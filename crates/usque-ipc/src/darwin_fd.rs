//! Bounded Darwin Unix descriptor transport using kernel audit tokens.
//!
//! Connection credentials are not code identity. The Agent must additionally
//! verify the installed, hardened Engine and revalidate it for each request.
//! Reading *all* bytes with recvmsg prevents hidden ancillary descriptors in
//! payload fragments. This module never accepts Linux SCM_CREDENTIALS.

use std::{
    io::{self, Write},
    mem,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::net::UnixStream,
    },
    ptr,
};

/// Serialize only descriptor installation with child creation. Polling and
/// waiting for peers never hold this lock.
pub static DESCRIPTOR_EXEC_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub const MAX_FRAME_BYTES: usize = 64 * 1024;
// XNU UIPC_MAX_CMSG_FD is 512. A smaller buffer is unsafe on Darwin:
// truncated SCM_RIGHTS can install descriptors it does not report to userspace.
const CONTROL_WORDS: usize = (512 * mem::size_of::<RawFd>() + 64) / mem::size_of::<usize>();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerIdentity {
    /// Native audit_token_t words, including pidversion (word 7).
    pub audit_token: [u32; 8],
}

pub fn peer_identity(socket: &impl AsRawFd) -> io::Result<PeerIdentity> {
    identity_for_fd(socket.as_raw_fd())
}

fn identity_for_fd(socket: RawFd) -> io::Result<PeerIdentity> {
    let mut token = [0_u32; 8];
    let mut length = mem::size_of_val(&token) as libc::socklen_t;
    // SAFETY: token and length are writable buffers of the declared size.
    let result = unsafe {
        libc::getsockopt(
            socket,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            token.as_mut_ptr().cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != mem::size_of_val(&token) || token[5] == 0 || token[7] == 0 {
        return Err(invalid("invalid Darwin peer audit token"));
    }
    Ok(PeerIdentity { audit_token: token })
}

fn verify_peer(socket: RawFd, expected: PeerIdentity) -> io::Result<()> {
    if identity_for_fd(socket)? != expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Darwin peer identity changed",
        ));
    }
    Ok(())
}

fn set_cloexec(fd: RawFd) -> io::Result<()> {
    // SAFETY: fd is newly received and owned; these calls only inspect/set flags.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the live descriptor is owned for the duration of this call.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[derive(Debug)]
pub struct ReceivedFrame {
    /// Includes the four-byte big-endian protobuf length prefix.
    pub bytes: Vec<u8>,
    pub descriptor: Option<OwnedFd>,
}

pub fn receive_frame(socket: &UnixStream, expected: PeerIdentity) -> io::Result<ReceivedFrame> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut header = [0_u8; 4];
    let mut descriptor = None;
    receive_exact(
        socket.as_raw_fd(),
        &mut header,
        expected,
        &mut descriptor,
        deadline,
    )?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(invalid("Agent frame length is outside the allowed range"));
    }
    let mut bytes = vec![0_u8; length + 4];
    bytes[..4].copy_from_slice(&header);
    receive_exact(
        socket.as_raw_fd(),
        &mut bytes[4..],
        expected,
        &mut descriptor,
        deadline,
    )?;
    Ok(ReceivedFrame { bytes, descriptor })
}

fn receive_exact(
    socket: RawFd,
    mut output: &mut [u8],
    expected: PeerIdentity,
    descriptor: &mut Option<OwnedFd>,
    deadline: std::time::Instant,
) -> io::Result<()> {
    while !output.is_empty() {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut poll = libc::pollfd {
            fd: socket,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd; bounded wait outside the descriptor lock.
        let ready = unsafe {
            libc::poll(
                &mut poll,
                1,
                remaining.as_millis().min(i32::MAX as u128) as i32,
            )
        };
        if ready == 0 {
            return Err(io::ErrorKind::TimedOut.into());
        }
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        let _serial = DESCRIPTOR_EXEC_LOCK
            .lock()
            .map_err(|_| io::Error::other("descriptor lock poisoned"))?;
        verify_peer(socket, expected)?;
        // cmsghdr alignment and room for unexpected descriptors.
        let mut control = [0_usize; CONTROL_WORDS];
        let mut iov = libc::iovec {
            iov_base: output.as_mut_ptr().cast(),
            iov_len: output.len(),
        };
        // SAFETY: zero initializes all optional pointer/count fields.
        let mut message: libc::msghdr = unsafe { mem::zeroed() };
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = mem::size_of_val(&control) as _;
        // SAFETY: message references live, writable iov and control buffers.
        let received = unsafe { libc::recvmsg(socket, &mut message, libc::MSG_DONTWAIT) };
        if received < 0 {
            let error = io::Error::last_os_error();
            if matches!(
                error.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) {
                continue;
            }
            return Err(error);
        }
        let mut fds = Vec::new();
        let mut unsupported = false;
        let available = (message.msg_controllen as usize).min(mem::size_of_val(&control));
        message.msg_controllen = available as _;
        // SAFETY: every header and payload is bounded below against the actual
        // returned buffer. Darwin may leave cmsg_len larger than msg_controllen
        // on truncation, so CMSG_NXTHDR alone is not a payload bounds check.
        unsafe {
            let mut header = libc::CMSG_FIRSTHDR(&message);
            while !header.is_null() {
                let offset = header as usize - control.as_ptr() as usize;
                let remaining = available.saturating_sub(offset);
                let header_length = libc::CMSG_LEN(0) as usize;
                if remaining < header_length {
                    unsupported = true;
                    break;
                }
                let declared = (*header).cmsg_len as usize;
                if declared < header_length {
                    unsupported = true;
                    break;
                }
                let truncated = declared > remaining;
                unsupported |= truncated;
                let data_length = declared.min(remaining) - header_length;
                match ((*header).cmsg_level, (*header).cmsg_type) {
                    (libc::SOL_SOCKET, libc::SCM_RIGHTS) => {
                        if !data_length.is_multiple_of(mem::size_of::<RawFd>()) {
                            unsupported = true;
                        }
                        for offset in 0..(data_length / mem::size_of::<RawFd>()) {
                            let fd = ptr::read_unaligned(
                                libc::CMSG_DATA(header).cast::<RawFd>().add(offset),
                            );
                            // Each in-bounds descriptor was installed by recvmsg.
                            if fd < 0 {
                                unsupported = true;
                            } else {
                                fds.push(OwnedFd::from_raw_fd(fd));
                            }
                        }
                    }
                    _ => unsupported = true,
                }
                if truncated {
                    break;
                }
                header = libc::CMSG_NXTHDR(&message, header);
            }
        }
        if message.msg_flags & (libc::MSG_CTRUNC | libc::MSG_TRUNC) != 0 || unsupported {
            return Err(invalid("truncated or unsupported Unix ancillary data"));
        }
        // A zero-byte recvmsg can still install SCM_RIGHTS on Darwin. Dropping
        // fds here closes them before treating the message as EOF.
        if received == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        // Darwin has no MSG_CMSG_CLOEXEC. The privileged service must never
        // fork/exec concurrently with this receiver; close-on-exec is set before
        // exposing any received descriptor to other code.
        for fd in &fds {
            set_cloexec(fd.as_raw_fd())?;
        }
        verify_peer(socket, expected)?;
        if fds.len() + usize::from(descriptor.is_some()) > 1 {
            return Err(invalid("multiple descriptors in one Agent frame"));
        }
        if let Some(fd) = fds.pop() {
            *descriptor = Some(fd);
        }
        output = &mut output[received as usize..];
    }
    Ok(())
}

pub fn send_frame(
    mut socket: &UnixStream,
    bytes: &[u8],
    descriptor: Option<&impl AsRawFd>,
) -> io::Result<()> {
    if bytes.len() < 5
        || bytes.len() > MAX_FRAME_BYTES + 4
        || u32::from_be_bytes(bytes[..4].try_into().expect("checked header")) as usize
            != bytes.len() - 4
    {
        return Err(invalid("invalid outbound Agent frame"));
    }
    let mut control = [0_usize; 4];
    let mut iov = libc::iovec {
        iov_base: bytes.as_ptr().cast_mut().cast(),
        iov_len: bytes.len(),
    };
    // SAFETY: zero initializes all optional pointer/count fields.
    let mut message: libc::msghdr = unsafe { mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    if let Some(fd) = descriptor {
        message.msg_control = control.as_mut_ptr().cast();
        // SAFETY: buffer is cmsghdr-aligned and has room for one RawFd.
        unsafe {
            message.msg_controllen = libc::CMSG_SPACE(mem::size_of::<RawFd>() as _) as _;
            let header = libc::CMSG_FIRSTHDR(&message);
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len = libc::CMSG_LEN(mem::size_of::<RawFd>() as _) as _;
            ptr::write_unaligned(libc::CMSG_DATA(header).cast::<RawFd>(), fd.as_raw_fd());
        }
    }
    let written = loop {
        // SAFETY: message only borrows live buffers for the duration of sendmsg.
        let result = unsafe { libc::sendmsg(socket.as_raw_fd(), &message, libc::MSG_NOSIGNAL) };
        if result >= 0 {
            break result as usize;
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    };
    if written == 0 {
        return Err(io::Error::from(io::ErrorKind::WriteZero));
    }
    socket.write_all(&bytes[written..])
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests;
