use std::io::{self, Write};
use std::path::Path;

use zeroize::Zeroizing;

#[cfg(windows)]
mod windows_clipboard;

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
    windows_clipboard::copy(value)
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
