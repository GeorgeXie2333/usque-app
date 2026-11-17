//! Kernel resource identifiers are scoped to one boot, including PF tokens.
use std::io;
use uuid::Uuid;
pub fn identity() -> io::Result<Uuid> {
    let mut bytes = [0u8; 64];
    let mut length = bytes.len();
    // SAFETY: constant NUL-terminated sysctl name and bounded output buffer.
    if unsafe {
        libc::sysctlbyname(
            c"kern.bootsessionuuid".as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if length == 0 || length > bytes.len() {
        return Err(io::Error::other("boot identity unavailable"));
    }
    let value = std::str::from_utf8(&bytes[..length])
        .map_err(io::Error::other)?
        .trim_end_matches('\0');
    Uuid::parse_str(value).map_err(io::Error::other)
}
