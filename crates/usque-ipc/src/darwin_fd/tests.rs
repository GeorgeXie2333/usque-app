use super::*;
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::AsRawFd,
};

fn pair() -> (UnixStream, UnixStream, PeerIdentity) {
    let (a, b) = UnixStream::pair().unwrap();
    let timeout = Some(std::time::Duration::from_secs(2));
    a.set_write_timeout(timeout).unwrap();
    b.set_read_timeout(timeout).unwrap();
    let peer = peer_identity(&b).unwrap();
    (a, b, peer)
}

#[test]
fn kernel_audit_identity_includes_pid_uid_and_incarnation() {
    let (_, _, peer) = pair();
    assert_eq!(peer.audit_token[5], std::process::id());
    // SAFETY: geteuid only reads this process's effective UID.
    assert_eq!(peer.audit_token[1], unsafe { libc::geteuid() });
    assert_ne!(peer.audit_token[7], 0);
}

#[test]
fn transfers_owned_descriptor_and_sets_close_on_exec() {
    let (a, b, peer) = pair();
    let file = File::open("/dev/null").unwrap();
    send_frame(&a, &[0, 0, 0, 1, 42], Some(&file)).unwrap();
    drop(file);
    let frame = receive_frame(&b, peer).unwrap();
    assert_eq!(frame.bytes, [0, 0, 0, 1, 42]);
    let fd = frame.descriptor.unwrap();
    // SAFETY: reading flags does not consume the live descriptor.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
    assert_ne!(flags & libc::FD_CLOEXEC, 0);
    assert_eq!(File::from(fd).read(&mut [0]).unwrap(), 0);
}

#[test]
fn rejects_a_different_process_incarnation_even_when_uid_and_pid_match() {
    let (a, b, mut peer) = pair();
    send_frame(&a, &[0, 0, 0, 1, 42], None::<&File>).unwrap();
    peer.audit_token[7] ^= 1;
    assert_eq!(
        receive_frame(&b, peer).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn rejects_oversized_and_empty_frames_before_reading_payload() {
    for size in [0, MAX_FRAME_BYTES as u32 + 1] {
        let (mut a, b, peer) = pair();
        a.write_all(&size.to_be_bytes()).unwrap();
        assert_eq!(
            receive_frame(&b, peer).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[test]
fn reads_fragmented_frames_without_consuming_next_frame() {
    let (mut a, b, peer) = pair();
    a.write_all(&[0, 0]).unwrap();
    a.write_all(&[0, 2, 7, 8, 0, 0, 0, 1, 9]).unwrap();
    assert_eq!(receive_frame(&b, peer).unwrap().bytes, [0, 0, 0, 2, 7, 8]);
    assert_eq!(receive_frame(&b, peer).unwrap().bytes, [0, 0, 0, 1, 9]);
}

#[test]
fn rejects_malformed_outbound_frames() {
    let (a, _, _) = pair();
    for bytes in [&[][..], &[0, 0, 0, 0], &[0, 0, 0, 2, 1]] {
        assert_eq!(
            send_frame(&a, bytes, None::<&File>).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}

fn raw_fragment(socket: &UnixStream, bytes: &[u8], descriptors: &[&File]) {
    use std::{mem, ptr};
    let mut control = [0_usize; 260];
    let mut iov = libc::iovec {
        iov_base: bytes.as_ptr().cast_mut().cast(),
        iov_len: bytes.len(),
    };
    // SAFETY: zero initializes optional fields, and every buffer remains live
    // until sendmsg returns. This deliberately bypasses frame validation.
    unsafe {
        let mut message: libc::msghdr = mem::zeroed();
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen =
            libc::CMSG_SPACE((descriptors.len() * mem::size_of::<libc::c_int>()) as _);
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len =
            libc::CMSG_LEN((descriptors.len() * mem::size_of::<libc::c_int>()) as _);
        for (index, file) in descriptors.iter().enumerate() {
            ptr::write_unaligned(
                libc::CMSG_DATA(header).cast::<libc::c_int>().add(index),
                file.as_raw_fd(),
            );
        }
        assert_eq!(
            libc::sendmsg(socket.as_raw_fd(), &message, libc::MSG_NOSIGNAL),
            bytes.len() as isize
        );
    }
}

#[test]
fn refuses_multiple_descriptors_even_when_second_is_hidden_in_payload() {
    let file = File::open("/dev/null").unwrap();
    for fragmented in [false, true] {
        let (a, b, peer) = pair();
        if fragmented {
            raw_fragment(&a, &[0, 0, 0, 1], &[&file]);
            raw_fragment(&a, &[42], &[&file]);
        } else {
            raw_fragment(&a, &[0, 0, 0, 1, 42], &[&file, &file]);
        }
        assert_eq!(
            receive_frame(&b, peer).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[test]
fn permits_one_descriptor_in_payload_fragment_but_rejects_truncated_payload() {
    let file = File::open("/dev/null").unwrap();
    let (mut a, b, peer) = pair();
    a.write_all(&[0, 0, 0, 1]).unwrap();
    raw_fragment(&a, &[42], &[&file]);
    assert!(receive_frame(&b, peer).unwrap().descriptor.is_some());
    raw_fragment(&a, &[0, 0, 0, 8], &[&file]);
    drop(a);
    assert!(matches!(
        receive_frame(&b, peer).unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof | io::ErrorKind::NotConnected
    ));
}

#[test]
fn control_buffer_covers_xnu_maximum_rights_without_truncation() {
    // SAFETY: CMSG_SPACE only computes the required size for 512 int FDs.
    let needed = unsafe { libc::CMSG_SPACE(512 * 4) } as usize;
    assert!(CONTROL_WORDS * std::mem::size_of::<usize>() >= needed);
}

#[test]
fn zero_payload_rights_are_closed_before_reporting_eof() {
    let (a, b, peer) = pair();
    let (mut witness, handle) = UnixStream::pair().unwrap();
    witness.set_nonblocking(true).unwrap();
    let file = File::from(std::os::fd::OwnedFd::from(handle));
    raw_fragment(&a, &[], &[&file]);
    drop(file);
    assert_eq!(
        receive_frame(&b, peer).unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
    assert_eq!(
        witness.read(&mut [0]).unwrap(),
        0,
        "all transferred references must close"
    );
}

#[test]
fn excessive_rights_are_rejected_without_leaking_any_duplicate() {
    const {
        assert!(CONTROL_WORDS >= 260);
    }
    for count in [62, 100, 200, 254] {
        let (a, b, peer) = pair();
        let (mut witness, handle) = UnixStream::pair().unwrap();
        witness.set_nonblocking(true).unwrap();
        let file = File::from(std::os::fd::OwnedFd::from(handle));
        raw_fragment(&a, &[0, 0, 0, 1, 42], &vec![&file; count]);
        drop(file);
        assert_eq!(
            receive_frame(&b, peer).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            witness.read(&mut [0]).unwrap(),
            0,
            "leaked FD with count {count}"
        );
    }
}

#[test]
fn idle_and_partial_frame_waits_do_not_hold_descriptor_spawn_lock() {
    for partial in [false, true] {
        let (mut a, b, peer) = pair();
        if partial {
            a.write_all(&[0, 0]).unwrap();
        }
        let task = std::thread::spawn(move || receive_frame(&b, peer));
        std::thread::sleep(std::time::Duration::from_millis(20));
        let (send, recv) = std::sync::mpsc::channel();
        let lock = std::thread::spawn(move || {
            let _guard = DESCRIPTOR_EXEC_LOCK.lock().unwrap();
            send.send(()).unwrap();
        });
        let available = recv.recv_timeout(std::time::Duration::from_secs(1)).is_ok();
        drop(a);
        let _ = task.join().unwrap();
        lock.join().unwrap();
        assert!(
            available,
            "idle peer blocked all descriptor installation and child creation"
        );
    }
}
