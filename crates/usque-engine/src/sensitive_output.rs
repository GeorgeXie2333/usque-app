use std::io::{self, Write};
use std::path::Path;

use zeroize::Zeroizing;

pub(crate) fn export_secret_noclobber(
    destination: &Path,
    secret: &Zeroizing<String>,
) -> Result<(), io::Error> {
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "export destination has no parent",
        )
    })?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".usque-secret-")
        .tempfile_in(parent)?;
    temporary.write_all(secret.as_bytes())?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(destination)
        .map_err(|error| error.error)?;
    Ok(())
}

#[cfg(windows)]
pub(crate) fn copy_sensitive_text(value: &[u8]) -> Result<(), io::Error> {
    use std::ptr;
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };

    const CF_UNICODETEXT: u32 = 13;
    let text = std::str::from_utf8(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "secret is not UTF-8"))?;
    let wide = Zeroizing::new(text.encode_utf16().chain([0]).collect::<Vec<u16>>());
    let byte_length = wide.len() * size_of::<u16>();

    // SAFETY: this follows the Win32 clipboard ownership contract. On a
    // successful SetClipboardData call the system owns `allocation`; on every
    // earlier failure Usque releases it before returning.
    unsafe {
        if OpenClipboard(ptr::null_mut()) == 0 {
            return Err(io::Error::last_os_error());
        }
        if EmptyClipboard() == 0 {
            let error = io::Error::last_os_error();
            let _ = CloseClipboard();
            return Err(error);
        }
        let allocation = GlobalAlloc(GMEM_MOVEABLE, byte_length);
        if allocation.is_null() {
            let error = io::Error::last_os_error();
            let _ = CloseClipboard();
            return Err(error);
        }
        let target = GlobalLock(allocation);
        if target.is_null() {
            let error = io::Error::last_os_error();
            let _ = GlobalFree(allocation);
            let _ = CloseClipboard();
            return Err(error);
        }
        ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), target.cast::<u8>(), byte_length);
        let _ = GlobalUnlock(allocation);
        if SetClipboardData(CF_UNICODETEXT, allocation).is_null() {
            let error = io::Error::last_os_error();
            let _ = GlobalFree(allocation);
            let _ = CloseClipboard();
            return Err(error);
        }
        if CloseClipboard() == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn copy_sensitive_text(_value: &[u8]) -> Result<(), io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native clipboard integration is unavailable on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_is_atomic_and_refuses_to_overwrite() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("warp-secret.json");
        let secret = Zeroizing::new("secret".to_owned());
        export_secret_noclobber(&destination, &secret).unwrap();
        assert_eq!(std::fs::read_to_string(&destination).unwrap(), "secret");
        assert_eq!(
            export_secret_noclobber(&destination, &secret)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
    }
}
