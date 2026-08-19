//! Bounded, privacy-filtered JSONL logging for the desktop engine.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use tracing_subscriber::fmt::MakeWriter;

const MAX_TOTAL_BYTES: u64 = 20 * 1024 * 1024;
const ROTATE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_EVENT_BYTES: usize = 256 * 1024;
const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const ACTIVE_LOG_NAME: &str = "engine.jsonl";

#[derive(Clone)]
pub struct LogWriterFactory {
    shared: Arc<Mutex<LogState>>,
}

struct LogState {
    directory: PathBuf,
    active_path: PathBuf,
    file: Option<File>,
    bytes_written: u64,
    rotation_counter: u32,
}

impl LogWriterFactory {
    pub fn open(config_path: &Path) -> io::Result<Self> {
        let directory = log_directory(config_path);
        fs::create_dir_all(&directory)?;
        prune_logs(&directory, MAX_TOTAL_BYTES)?;
        let active_path = directory.join(ACTIVE_LOG_NAME);
        let bytes_written = fs::metadata(&active_path)
            .map(|metadata| metadata.len())
            .unwrap_or_default();
        let file = open_active_log(&active_path)?;
        Ok(Self {
            shared: Arc::new(Mutex::new(LogState {
                directory,
                active_path,
                file: Some(file),
                bytes_written,
                rotation_counter: 0,
            })),
        })
    }
}

impl<'a> MakeWriter<'a> for LogWriterFactory {
    type Writer = BufferedLogEvent;

    fn make_writer(&'a self) -> Self::Writer {
        BufferedLogEvent {
            shared: Arc::clone(&self.shared),
            buffer: Vec::with_capacity(1024),
            overflowed: false,
        }
    }
}

pub struct BufferedLogEvent {
    shared: Arc<Mutex<LogState>>,
    buffer: Vec<u8>,
    overflowed: bool,
}

impl Write for BufferedLogEvent {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = MAX_EVENT_BYTES.saturating_sub(self.buffer.len());
        let copied = remaining.min(bytes.len());
        self.buffer.extend_from_slice(&bytes[..copied]);
        self.overflowed |= copied != bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for BufferedLogEvent {
    fn drop(&mut self) {
        let event = if self.overflowed {
            br#"{"level":"ERROR","target":"usque_engine","message":"oversized log event omitted"}"#
                .to_vec()
        } else {
            sanitize_log_bytes(&self.buffer)
        };
        if event.is_empty() {
            return;
        }
        if let Ok(mut state) = self.shared.lock() {
            let _ = state.write_event(&event);
        }
    }
}

impl LogState {
    fn write_event(&mut self, event: &[u8]) -> io::Result<()> {
        let event_length = u64::try_from(event.len()).unwrap_or(u64::MAX);
        if self.bytes_written > 0 && self.bytes_written.saturating_add(event_length) > ROTATE_BYTES
        {
            self.rotate()?;
        }
        prune_logs(
            &self.directory,
            MAX_TOTAL_BYTES.saturating_sub(event_length),
        )?;
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("active log file is closed"))?;
        file.write_all(event)?;
        if !event.ends_with(b"\n") {
            file.write_all(b"\n")?;
            self.bytes_written = self.bytes_written.saturating_add(1);
        }
        self.bytes_written = self.bytes_written.saturating_add(event_length);
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.take() {
            file.sync_data()?;
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let rotated_path = loop {
            let candidate = self.directory.join(format!(
                "engine-{timestamp}-{}.jsonl",
                self.rotation_counter
            ));
            self.rotation_counter = self.rotation_counter.wrapping_add(1);
            if !candidate.exists() {
                break candidate;
            }
        };
        if self.active_path.exists() {
            fs::rename(&self.active_path, rotated_path)?;
        }
        self.file = Some(open_active_log(&self.active_path)?);
        self.bytes_written = 0;
        prune_logs(&self.directory, MAX_TOTAL_BYTES)?;
        Ok(())
    }
}

fn open_active_log(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

pub fn log_directory(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("logs")
}

fn prune_logs(directory: &Path, byte_limit: u64) -> io::Result<()> {
    let now = SystemTime::now();
    let mut logs = Vec::new();
    for entry in match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    } {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !is_engine_log_name(&file_name) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_file() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        if file_name != ACTIVE_LOG_NAME
            && now.duration_since(modified).unwrap_or_default() > MAX_AGE
        {
            fs::remove_file(path)?;
            continue;
        }
        logs.push((path, file_name == ACTIVE_LOG_NAME, modified, metadata.len()));
    }
    logs.sort_by_key(|(_, active, modified, _)| (*active, *modified));
    let mut total = logs.iter().map(|(_, _, _, length)| *length).sum::<u64>();
    for (path, active, _, length) in logs {
        if total <= byte_limit {
            break;
        }
        if active {
            continue;
        }
        fs::remove_file(path)?;
        total = total.saturating_sub(length);
    }
    Ok(())
}

fn is_engine_log_name(name: &str) -> bool {
    name == ACTIVE_LOG_NAME || (name.starts_with("engine-") && name.ends_with(".jsonl"))
}

pub fn sanitize_log_bytes(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let Ok(mut value) = serde_json::from_str::<Value>(trimmed) else {
        return br#"{"level":"WARN","target":"usque_engine","message":"non-JSON log event omitted"}"#
            .to_vec();
    };
    redact_log_value(&mut value, None);
    serde_json::to_vec(&value).unwrap_or_else(|_| {
        br#"{"level":"ERROR","target":"usque_engine","message":"log serialization failed"}"#
            .to_vec()
    })
}

fn redact_log_value(value: &mut Value, key: Option<&str>) {
    if key.is_some_and(is_sensitive_log_key) {
        *value = Value::String("[REDACTED]".to_owned());
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                redact_log_value(value, Some(key));
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_log_value(item, None);
            }
        }
        Value::String(text) => *text = scrub_network_tokens(text),
        _ => {}
    }
}

fn is_sensitive_log_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    [
        "access_token",
        "assertion",
        "authorization",
        "callback_uri",
        "certificate",
        "cf-access-jwt-assertion",
        "cookie",
        "credential",
        "device_id",
        "endpoint",
        "endpoint_pin",
        "ip",
        "ipv4",
        "ipv6",
        "license",
        "listener",
        "jwt",
        "name",
        "passwd",
        "password",
        "peer",
        "private_key",
        "proxy-authorization",
        "proxy_authorization",
        "proxy_password",
        "remote",
        "secret",
        "sni",
        "source",
        "token",
        "warp_secret",
        "zero_trust_callback",
    ]
    .iter()
    .any(|candidate| {
        normalized == *candidate
            || normalized.ends_with(&format!("_{candidate}"))
            || normalized.starts_with(&format!("{candidate}_"))
    })
}

fn scrub_network_tokens(input: &str) -> String {
    input
        .split_inclusive(char::is_whitespace)
        .map(|part| {
            let (token, whitespace) = part
                .strip_suffix(char::is_whitespace)
                .map_or((part, ""), |token| (token, &part[token.len()..]));
            let trimmed = token.trim_matches(|character: char| {
                matches!(character, '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';')
            });
            if looks_like_network_identifier(trimmed) {
                format!("[NETWORK_REDACTED]{whitespace}")
            } else {
                part.to_owned()
            }
        })
        .collect()
}

fn looks_like_network_identifier(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if token.parse::<IpAddr>().is_ok()
        || token.parse::<SocketAddr>().is_ok()
        || token.contains("://")
        || looks_like_jwt(token)
    {
        return true;
    }
    let candidate = token.trim_end_matches(['.', ':']);
    candidate.contains('.')
        && !candidate.contains(['/', '\\'])
        && candidate.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
}

fn looks_like_jwt(token: &str) -> bool {
    let mut segments = token.split('.');
    let parts = [segments.next(), segments.next(), segments.next()];
    segments.next().is_none()
        && token.len() >= 32
        && parts.into_iter().all(|part| {
            part.is_some_and(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_log_redaction_removes_secrets_and_network_identifiers() {
        let sanitized = sanitize_log_bytes(
            br#"{"level":"WARN","peer":"192.0.2.1:443","warp_secret":"secret","message":"failed to reach example.com at 2001:db8::1"}"#,
        );
        let text = String::from_utf8(sanitized).unwrap();
        assert!(!text.contains("192.0.2.1"));
        assert!(!text.contains(r#""warp_secret":"secret""#));
        assert!(!text.contains("example.com"));
        assert!(!text.contains("2001:db8"));
        assert!(text.contains("[REDACTED]"));
        assert!(text.contains("[NETWORK_REDACTED]"));
    }

    #[test]
    fn proxy_passwords_are_redacted_from_logs() {
        let sanitized = sanitize_log_bytes(
            br#"{"password":"listener-secret","proxy_password":"vault-secret","Proxy-Authorization":"Basic dXNlcjpwYXNz"}"#,
        );
        let text = String::from_utf8(sanitized).unwrap();
        for secret in ["listener-secret", "vault-secret", "Basic dXNlcjpwYXNz"] {
            assert!(!text.contains(secret), "log retained {secret}");
        }
        assert!(text.contains("[REDACTED]"));
    }

    #[test]
    fn zero_trust_headers_callbacks_and_jwts_are_always_redacted() {
        let sanitized = sanitize_log_bytes(
            br#"{"CF-Access-Jwt-Assertion":"header-secret","jwt":"jwt-secret","assertion":"assertion-secret","zero_trust_callback":"callback-secret","message":"callback com.cloudflare.warp://example.cloudflareaccess.com/auth?token=secret JWT eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.signature"}"#,
        );
        let text = String::from_utf8(sanitized).unwrap();
        for secret in [
            "header-secret",
            "jwt-secret",
            "assertion-secret",
            "callback-secret",
            "com.cloudflare.warp",
            "eyJhbGciOiJIUzI1NiJ9",
        ] {
            assert!(!text.contains(secret), "log retained {secret}");
        }
    }

    #[test]
    fn writer_creates_a_bounded_jsonl_file() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.json");
        let factory = LogWriterFactory::open(&config).unwrap();
        {
            let mut writer = factory.make_writer();
            writer
                .write_all(br#"{"level":"INFO","message":"ready"}"#)
                .unwrap();
        }
        let contents = fs::read_to_string(log_directory(&config).join(ACTIVE_LOG_NAME)).unwrap();
        assert!(contents.ends_with('\n'));
        assert!(contents.contains("\"ready\""));
    }
}
