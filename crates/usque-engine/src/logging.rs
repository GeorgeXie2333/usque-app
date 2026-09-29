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
use usque_core::diagnostics_contract_generated::LOG_EVENT_CODES;

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
    if key.is_some_and(is_public_numeric_key) && (value.is_number() || value.is_boolean()) {
        return;
    }
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
        "path",
        "directory",
        "hostname",
        "ssid",
        "username",
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
    let normalized = input.to_ascii_lowercase();
    // An assignment may include quoted, whitespace-separated secret material.
    // Remove the whole free-text value instead of trying to guess its boundary.
    if [
        "password",
        "passwd",
        "token",
        "secret",
        "private_key",
        "authorization",
        "cookie",
        "assertion",
        "license",
        "username",
        "credential",
        "device_id",
        "certificate",
        "path",
        "directory",
    ]
    .iter()
    .any(|key| {
        normalized.match_indices(key).any(|(offset, _)| {
            normalized[offset + key.len()..]
                .trim_start_matches([' ', '\t', '"', '\''])
                .starts_with(['=', ':'])
        })
    }) {
        return "[REDACTED]".to_owned();
    }
    input
        .split_inclusive(char::is_whitespace)
        .map(|part| {
            let (token, whitespace) = part
                .strip_suffix(char::is_whitespace)
                .map_or((part, ""), |token| (token, &part[token.len()..]));
            let trimmed = token.trim_matches(|character: char| {
                matches!(character, '"' | '\'' | '(' | ')' | ',' | ';')
            });
            let identifier = trimmed.rsplit_once('=').map_or(trimmed, |(_, value)| value);
            if looks_like_network_identifier(identifier.trim_end_matches(':')) {
                format!("[NETWORK_REDACTED]{whitespace}")
            } else if looks_like_file_path(identifier) {
                format!("[PATH_REDACTED]{whitespace}")
            } else {
                part.to_owned()
            }
        })
        .collect()
}

fn looks_like_file_path(token: &str) -> bool {
    token.starts_with('/')
        || token.starts_with("./")
        || token.starts_with("../")
        || token.contains('\\')
        || token.contains(":/")
}

fn is_public_numeric_key(key: &str) -> bool {
    matches!(
        key,
        "sequence"
            | "elapsed_ms"
            | "duration_ms"
            | "received_frames"
            | "sent_frames"
            | "received_bytes"
            | "sent_bytes"
            | "attempt"
            | "attempts"
            | "attempt_limit"
            | "reconnect_count"
            | "fallback_count"
            | "listener_count"
            | "active_listener_count"
            | "endpoint_count"
            | "profiles"
            | "gate_driver_id"
            | "worker_pending"
            | "fatal"
            | "retryable"
            | "historical_terminal"
            | "ipv4_available"
            | "ipv6_available"
            | "endpoint_pin_valid"
            | "request_accepted"
            | "journal_generation"
            | "os_code"
            | "win32_code"
            | "panic_line"
            | "panic_column"
            | "queue_items"
            | "queue_bytes"
            | "drop_items"
            | "drop_bytes"
            | "queue_drops"
            | "network_generation"
            | "backlog"
            | "capacity"
            | "sent_packets"
            | "received_packets"
    ) || usque_core::diagnostics_contract_generated::EVIDENCE_KEYS.contains(&key)
}

/// Public bundles have a stronger boundary than local debug logs. Unknown text,
/// messages, errors, keys and nested objects have no path through this projection.
pub fn project_public_log(bytes: &[u8]) -> Vec<u8> {
    let Ok(Value::Object(input)) = serde_json::from_slice(bytes) else {
        return Vec::new();
    };
    let mut output = serde_json::Map::new();
    output.insert("schema_version".into(), Value::from(1));
    if let Some(level) = input
        .get("level")
        .and_then(Value::as_str)
        .filter(|level| matches!(*level, "ERROR" | "WARN" | "INFO" | "DEBUG" | "TRACE"))
    {
        output.insert("level".into(), Value::String(level.into()));
    }
    if let Some(timestamp) = input
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
    {
        output.insert("timestamp".into(), Value::String(timestamp.to_rfc3339()));
    }
    if let Some(target) = input
        .get("target")
        .and_then(Value::as_str)
        .and_then(|target| target.split("::").next())
        .filter(|target| {
            matches!(
                *target,
                "usque_engine"
                    | "usque_transport"
                    | "usque_agent"
                    | "usque_android"
                    | "usque_openvpn"
            )
        })
    {
        output.insert("component".into(), Value::String(target.into()));
    }
    let project_fields = |fields: &serde_json::Map<String, Value>| {
        fields
            .iter()
            .filter_map(|(key, value)| {
                if is_public_numeric_key(key)
                    && (value.is_boolean() || value.as_u64().is_some() || value.as_i64().is_some())
                {
                    return Some((key.clone(), value.clone()));
                }
                let text = value.as_str()?;
                let safe = match key.as_str() {
                    "gate_event" | "recovery_event" => LOG_EVENT_CODES.contains(&text),
                    "event_type" => {
                        usque_core::diagnostics_contract_generated::EVENT_TYPES.contains(&text)
                    }
                    "failure_code" | "error_code" => {
                        usque_core::diagnostics_contract_generated::FAILURE_CODES.contains(&text)
                    }
                    "check_id" => {
                        usque_core::diagnostics_contract_generated::CHECK_IDS.contains(&text)
                    }
                    "io_error_kind" => matches!(
                        text,
                        "NotFound"
                            | "PermissionDenied"
                            | "ConnectionRefused"
                            | "ConnectionReset"
                            | "ConnectionAborted"
                            | "NotConnected"
                            | "AddrInUse"
                            | "AddrNotAvailable"
                            | "BrokenPipe"
                            | "AlreadyExists"
                            | "WouldBlock"
                            | "InvalidInput"
                            | "InvalidData"
                            | "TimedOut"
                            | "WriteZero"
                            | "Interrupted"
                            | "UnexpectedEof"
                            | "Unsupported"
                            | "OutOfMemory"
                            | "Other"
                    ),
                    "state" => matches!(
                        text,
                        "Pending" | "Running" | "Cancelling" | "Completed" | "Failed" | "Cancelled"
                    ),
                    "reason_code" => matches!(
                        text,
                        "direct_send_failed"
                            | "direct_resolution_failed"
                            | "direct_connect_failed"
                            | "timeout"
                            | "query_failed"
                    ),
                    "transport" => {
                        matches!(text, "h2" | "h3" | "Http2" | "Http3" | "HTTP/2" | "HTTP/3")
                    }
                    "phase" => matches!(
                        text,
                        "disconnected"
                            | "preparing"
                            | "connecting_h3"
                            | "connecting_h2"
                            | "connected"
                            | "degraded"
                            | "reconnecting"
                            | "disconnecting"
                            | "error"
                    ),
                    _ => false,
                };
                safe.then(|| (key.clone(), value.clone()))
            })
            .collect::<serde_json::Map<_, _>>()
    };
    let mut fields = project_fields(&input);
    if let Some(nested) = input.get("fields").and_then(Value::as_object) {
        fields.extend(project_fields(nested));
    }
    output.insert("fields".into(), Value::Object(fields));
    serde_json::to_vec(&Value::Object(output)).unwrap_or_default()
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
    let authority = token
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(token)
        .rsplit('@')
        .next()
        .unwrap_or(token);
    if authority.parse::<IpAddr>().is_ok() || authority.parse::<SocketAddr>().is_ok() {
        return true;
    }
    if let Some(bracketed) = authority
        .strip_prefix('[')
        .and_then(|value| value.split(']').next())
        && bracketed.parse::<IpAddr>().is_ok()
    {
        return true;
    }
    let host = authority
        .rsplit_once(':')
        .filter(|(host, port)| {
            !host.contains(':') && port.bytes().all(|byte| byte.is_ascii_digit())
        })
        .map_or(authority, |(host, _)| host)
        .trim_end_matches('.');
    host.eq_ignore_ascii_case("localhost")
        || host.contains('.')
            && !host.contains(['/', '\\'])
            && host.split('.').all(|label| {
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
    fn assignments_punctuation_and_paths_are_scrubbed_without_losing_typed_counts() {
        for message in [
            "password=super-secret with spaces",
            "password = \"super-secret\"",
            "username = super-secret",
            "Authorization: Bearer super-secret",
            "203.0.113.1:443: timeout",
            "endpoint=203.0.113.1:443",
            r"C:\Users\private\config.json",
            "/Users/private/config.json",
        ] {
            let input = serde_json::to_vec(&serde_json::json!({"message": message, "listener_count": 3, "ipv4_available": true, "endpoint_pin_valid": true})).unwrap();
            let sanitized = sanitize_log_bytes(&input);
            let text = String::from_utf8(sanitized.clone()).unwrap();
            for secret in ["super-secret", "203.0.113.1", "private"] {
                assert!(!text.contains(secret), "retained private fixture in {text}");
            }
            let value: Value = serde_json::from_slice(&sanitized).unwrap();
            assert_eq!(value["listener_count"], 3);
            assert_eq!(value["ipv4_available"], true);
            assert_eq!(value["endpoint_pin_valid"], true);
        }
    }

    #[test]
    fn public_projection_excludes_unknown_text_and_preserves_typed_failure_evidence() {
        let input = serde_json::json!({
            "level": "WARN", "target": "usque_transport::vpngate",
            "message": "a raw secret", "arbitrary_secret_key": "another secret",
            "fields": {"gate_event": "TCP_READ_FAILED", "io_error_kind": "UnexpectedEof", "sent_frames": 1,
                "listener_count": 3, "ipv4_available": true, "error": "token=secret", "remote": "203.0.113.1:443",
                "sequence": "private", "state": "private", "elapsed_ms": {"secret": "private"}}
        });
        let projected = project_public_log(&serde_json::to_vec(&input).unwrap());
        let text = String::from_utf8(projected.clone()).unwrap();
        for private in ["secret", "private", "203.0.113.1", "arbitrary_secret_key"] {
            assert!(!text.contains(private));
        }
        let value: Value = serde_json::from_slice(&projected).unwrap();
        assert_eq!(value["component"], "usque_transport");
        assert_eq!(value["fields"]["gate_event"], "TCP_READ_FAILED");
        assert_eq!(value["fields"]["io_error_kind"], "UnexpectedEof");
        assert_eq!(value["fields"]["sent_frames"], 1);
        assert_eq!(value["fields"]["listener_count"], 3);
        assert_eq!(value["fields"]["ipv4_available"], true);
        assert!(project_public_log(br#"["secret"]"#).is_empty());
    }

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
            assert!(!text.contains(secret), "log retained a sensitive fixture");
        }
        assert!(text.contains("[REDACTED]"));
    }

    #[test]
    fn gate_stage_diagnostics_survive_export_without_endpoint_or_credentials() {
        let sanitized = sanitize_log_bytes(
            br#"{"fields":{"gate_event":"TCP_READ_FAILED","io_error_kind":"UnexpectedEof","received_frames":0,"sent_frames":1,"remote":"203.0.113.1:443","private_key":"fixture-secret"}}"#,
        );
        let value: Value = serde_json::from_slice(&sanitized).unwrap();
        assert_eq!(value["fields"]["gate_event"], "TCP_READ_FAILED");
        assert_eq!(value["fields"]["io_error_kind"], "UnexpectedEof");
        assert_eq!(value["fields"]["sent_frames"], 1);
        assert_eq!(value["fields"]["remote"], "[REDACTED]");
        assert_eq!(value["fields"]["private_key"], "[REDACTED]");
    }

    #[test]
    fn hostname_ports_paths_and_bracketed_ipv6_are_redacted() {
        let sanitized = sanitize_log_bytes(
            br#"{"message":"example.com:443 example.net/path user@private.example:8443 [2001:db8::5]:443/path localhost"}"#,
        );
        let text = String::from_utf8(sanitized).unwrap();
        for private in [
            "example.com",
            "example.net",
            "private.example",
            "2001:db8",
            "localhost",
        ] {
            assert!(!text.contains(private), "log retained {private}");
        }
        assert_eq!(text.matches("[NETWORK_REDACTED]").count(), 5);
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
            assert!(!text.contains(secret), "log retained a sensitive fixture");
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
