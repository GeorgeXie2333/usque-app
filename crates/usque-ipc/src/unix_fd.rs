//! Bounded Unix frames with kernel credentials and close-on-exec descriptors.
//!
//! All bytes are read through recvmsg: using read() for the payload would
//! silently discard ancillary data and could hide unexpected descriptors.

use std::{
    io::{self, Write},
    mem,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::net::UnixStream,
    },
    ptr,
};

pub const MAX_FRAME_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    pub process_id: u32,
    pub uid: u32,
    pub gid: u32,
}

pub fn peer_credentials(socket: &impl AsRawFd) -> io::Result<PeerCredentials> {
    let mut credentials = mem::MaybeUninit::<libc::ucred>::uninit();
    let mut length = mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: output points to enough writable storage and length describes it.
    let result = unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            credentials.as_mut_ptr().cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != mem::size_of::<libc::ucred>() {
        return Err(invalid("invalid peer credential size"));
    }
    // SAFETY: successful getsockopt initialized the complete ucred above.
    let credentials = unsafe { credentials.assume_init() };
    credentials_from_native(credentials)
}

fn credentials_from_native(value: libc::ucred) -> io::Result<PeerCredentials> {
    let process_id = u32::try_from(value.pid)
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or_else(|| invalid("invalid peer process"))?;
    Ok(PeerCredentials {
        process_id,
        uid: value.uid,
        gid: value.gid,
    })
}

pub fn enable_credentials(socket: &impl AsRawFd) -> io::Result<()> {
    let enabled = 1_i32;
    // SAFETY: socket is borrowed and enabled has the size supplied to setsockopt.
    let result = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PASSCRED,
            ptr::from_ref(&enabled).cast(),
            mem::size_of_val(&enabled) as _,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[derive(Debug)]
pub struct ReceivedFrame {
    /// Includes the four-byte, big-endian length prefix used by usque-ipc.
    pub bytes: Vec<u8>,
    pub descriptor: Option<OwnedFd>,
}

pub fn receive_frame(socket: &UnixStream, expected: PeerCredentials) -> io::Result<ReceivedFrame> {
    let mut header = [0_u8; 4];
    let mut descriptor = None;
    receive_exact(socket.as_raw_fd(), &mut header, expected, &mut descriptor)?;
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
    )?;
    Ok(ReceivedFrame { bytes, descriptor })
}

fn receive_exact(
    socket: RawFd,
    mut output: &mut [u8],
    expected: PeerCredentials,
    descriptor: &mut Option<OwnedFd>,
) -> io::Result<()> {
    while !output.is_empty() {
        // cmsghdr alignment, room for credentials and up to 16 unexpected FDs.
        let mut control = [0_usize; 32];
        let mut iov = libc::iovec {
            iov_base: output.as_mut_ptr().cast(),
            iov_len: output.len(),
        };
        // SAFETY: zero initializes all optional pointer/count fields.
        let mut message: libc::msghdr = unsafe { mem::zeroed() };
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = mem::size_of_val(&control);
        // SAFETY: message references live, writable iov and control buffers.
        let received = unsafe { libc::recvmsg(socket, &mut message, libc::MSG_CMSG_CLOEXEC) };
        if received < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if received == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        let mut credentials = None;
        let mut fds = Vec::new();
        let mut unsupported = false;
        // SAFETY: recvmsg produced this bounded ancillary buffer; CMSG helpers
        // validate header lengths against msg_controllen before advancing.
        unsafe {
            let mut header = libc::CMSG_FIRSTHDR(&message);
            while !header.is_null() {
                let data_length = (*header)
                    .cmsg_len
                    .saturating_sub(libc::CMSG_LEN(0) as usize);
                match ((*header).cmsg_level, (*header).cmsg_type) {
                    (libc::SOL_SOCKET, libc::SCM_RIGHTS) => {
                        if !data_length.is_multiple_of(mem::size_of::<RawFd>()) {
                            unsupported = true;
                        }
                        for offset in 0..(data_length / mem::size_of::<RawFd>()) {
                            let fd = ptr::read_unaligned(
                                libc::CMSG_DATA(header).cast::<RawFd>().add(offset),
                            );
                            // Each received descriptor is newly owned by this process.
                            fds.push(OwnedFd::from_raw_fd(fd));
                        }
                    }
                    (libc::SOL_SOCKET, libc::SCM_CREDENTIALS)
                        if data_length == mem::size_of::<libc::ucred>() =>
                    {
                        if credentials.is_some() {
                            unsupported = true;
                        }
                        credentials = Some(credentials_from_native(ptr::read_unaligned(
                            libc::CMSG_DATA(header).cast(),
                        ))?);
                    }
                    _ => unsupported = true,
                }
                header = libc::CMSG_NXTHDR(&message, header);
            }
        }
        if message.msg_flags & (libc::MSG_CTRUNC | libc::MSG_TRUNC) != 0 || unsupported {
            return Err(invalid("truncated or unsupported Unix ancillary data"));
        }
        if credentials != Some(expected) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Agent message sender changed",
            ));
        }
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
            message.msg_controllen = libc::CMSG_SPACE(mem::size_of::<RawFd>() as _) as usize;
            let header = libc::CMSG_FIRSTHDR(&message);
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len = libc::CMSG_LEN(mem::size_of::<RawFd>() as _) as usize;
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
mod tests {
    use super::*;
    use std::{fs::File, io::Read};

    #[test]
    fn credentials_and_descriptor_are_kernel_verified() {
        let (left, right) = UnixStream::pair().unwrap();
        enable_credentials(&right).unwrap();
        let peer = peer_credentials(&right).unwrap();
        let source = File::open("/dev/null").unwrap();
        send_frame(&left, &[0, 0, 0, 1, 42], Some(&source)).unwrap();
        let frame = receive_frame(&right, peer).unwrap();
        assert_eq!(frame.bytes, [0, 0, 0, 1, 42]);
        let fd = frame.descriptor.unwrap();
        assert_ne!(
            // SAFETY: fcntl reads descriptor flags without transferring ownership.
            unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        assert_eq!(File::from(fd).read(&mut [0_u8]).unwrap(), 0);
    }

    #[test]
    fn oversized_frame_rejected_before_allocating_body() {
        let (mut left, right) = UnixStream::pair().unwrap();
        enable_credentials(&right).unwrap();
        left.write_all(&((MAX_FRAME_BYTES + 1) as u32).to_be_bytes())
            .unwrap();
        assert!(receive_frame(&right, peer_credentials(&right).unwrap()).is_err());
    }

    #[test]
    fn changed_process_identity_is_rejected() {
        let (left, right) = UnixStream::pair().unwrap();
        enable_credentials(&right).unwrap();
        send_frame(&left, &[0, 0, 0, 1, 42], None::<&File>).unwrap();
        let mut peer = peer_credentials(&right).unwrap();
        peer.process_id += 1;
        assert_eq!(
            receive_frame(&right, peer).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
