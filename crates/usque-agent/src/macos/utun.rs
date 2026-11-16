//! Root-only utun allocation. Creating the device does not install addresses,
//! routes, DNS or firewall policy. The Agent retains the original descriptor.

use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
};

pub struct Utun {
    descriptor: OwnedFd,
    name: String,
    index: u32,
}
impl Utun {
    pub fn create() -> io::Result<Self> {
        // SAFETY: geteuid has no preconditions and does not mutate system state.
        if unsafe { libc::geteuid() } != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "utun allocation requires the root Agent",
            ));
        }
        let exec_lock = usque_ipc::darwin_fd::DESCRIPTOR_EXEC_LOCK
            .lock()
            .map_err(|_| io::Error::other("descriptor lock poisoned"))?;
        // SAFETY: socket returns a newly owned descriptor or -1.
        let fd = unsafe { libc::socket(libc::AF_SYSTEM, libc::SOCK_DGRAM, libc::SYSPROTO_CONTROL) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: this function is the sole owner of the returned descriptor.
        let descriptor = unsafe { OwnedFd::from_raw_fd(fd) };
        // SAFETY: both calls set flags on the newly created, unpublished fd.
        if unsafe {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) < 0
                || libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) < 0
        } {
            return Err(io::Error::last_os_error());
        }
        drop(exec_lock);
        // SAFETY: ctl_info contains integers and a C character array.
        let mut info: libc::ctl_info = unsafe { std::mem::zeroed() };
        let name = b"com.apple.net.utun_control";
        for (out, byte) in info.ctl_name.iter_mut().zip(name) {
            *out = *byte as libc::c_char;
        }
        // SAFETY: the ioctl writes within a complete, initialized ctl_info.
        if unsafe { libc::ioctl(fd, libc::CTLIOCGINFO, &mut info) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let address = libc::sockaddr_ctl {
            sc_len: std::mem::size_of::<libc::sockaddr_ctl>() as u8,
            sc_family: libc::AF_SYSTEM as u8,
            ss_sysaddr: libc::AF_SYS_CONTROL as u16,
            sc_id: info.ctl_id,
            sc_unit: 0,
            sc_reserved: [0; 5],
        };
        // SAFETY: sockaddr_ctl has the size/family given to connect and remains live.
        if unsafe {
            libc::connect(
                fd,
                std::ptr::from_ref(&address).cast(),
                std::mem::size_of_val(&address) as _,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = [0_u8; libc::IFNAMSIZ];
        let mut size = buffer.len() as libc::socklen_t;
        // SAFETY: UTUN_OPT_IFNAME (2) returns a C string bounded by size.
        if unsafe {
            libc::getsockopt(
                fd,
                libc::SYSPROTO_CONTROL,
                2,
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        let name = parse_name(buffer.get(..size as usize).ok_or_else(invalid)?)?;
        let cname = std::ffi::CString::new(name.as_str()).map_err(|_| invalid())?;
        // SAFETY: cname is a valid, terminated interface name.
        let index = unsafe { libc::if_nametoindex(cname.as_ptr()) };
        if index == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            descriptor,
            name,
            index,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn index(&self) -> u32 {
        self.index
    }
    pub fn duplicate(&self) -> io::Result<OwnedFd> {
        self.descriptor.try_clone()
    }
}
impl AsRawFd for Utun {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.descriptor.as_raw_fd()
    }
}

fn parse_name(buffer: &[u8]) -> io::Result<String> {
    let name = buffer.strip_suffix(&[0]).ok_or_else(invalid)?;
    let name = std::str::from_utf8(name).map_err(|_| invalid())?;
    let unit = name.strip_prefix("utun").ok_or_else(invalid)?;
    let number = unit.parse::<u32>().map_err(|_| invalid())?;
    if unit != number.to_string() || name.len() >= libc::IFNAMSIZ {
        return Err(invalid());
    }
    Ok(name.into())
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid kernel utun identity")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_kernel_utun_names_with_terminator() {
        assert_eq!(parse_name(b"utun0\0").unwrap(), "utun0");
        assert_eq!(parse_name(b"utun123\0").unwrap(), "utun123");
        for name in [
            &b"utun\0"[..],
            b"en0\0",
            b"utun-1\0",
            b"utun0",
            b"utun0\0suffix",
            b"utun00\0",
            b"utun1/2\0",
        ] {
            assert!(parse_name(name).is_err());
        }
    }
    #[test]
    #[ignore = "disposable Mac only: creates one unconfigured system utun"]
    fn disposable_mac_real_utun_descriptor_handoff() {
        use std::{
            os::unix::net::UnixStream,
            time::{Duration, Instant},
        };
        use usque_ipc::darwin_fd::{peer_identity, receive_frame, send_frame};
        assert_eq!(std::env::var("USQUE_DISPOSABLE_MAC").as_deref(), Ok("1"));
        let (agent, engine) = UnixStream::pair().unwrap();
        engine
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let peer = peer_identity(&engine).unwrap();
        let device = Utun::create().unwrap();
        let name = std::ffi::CString::new(device.name()).unwrap();
        let index = device.index();
        send_frame(&agent, &[0, 0, 0, 1, 1], Some(&device)).unwrap();
        let received = receive_frame(&engine, peer).unwrap().descriptor.unwrap();
        drop(device);
        // SAFETY: the name is terminated and the received descriptor is live.
        assert_eq!(unsafe { libc::if_nametoindex(name.as_ptr()) }, index);
        let mut actual = [0u8; libc::IFNAMSIZ];
        let mut len = actual.len() as libc::socklen_t;
        assert_eq!(
            // SAFETY: UTUN_OPT_IFNAME writes at most len bytes to the provided buffer.
            unsafe {
                libc::getsockopt(
                    received.as_raw_fd(),
                    libc::SYSPROTO_CONTROL,
                    2,
                    actual.as_mut_ptr().cast(),
                    &mut len,
                )
            },
            0
        );
        assert_eq!(&actual[..len as usize], name.as_bytes_with_nul());
        drop(received);
        let deadline = Instant::now() + Duration::from_secs(5);
        // SAFETY: querying the recorded name never changes an interface.
        while unsafe { libc::if_nametoindex(name.as_ptr()) } == index {
            assert!(
                Instant::now() < deadline,
                "interface remained after final descriptor close"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    #[test]
    fn unprivileged_allocation_is_denied_without_opening_a_control_socket() {
        // SAFETY: geteuid only reads the current process UID.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        assert_eq!(
            Utun::create().err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
