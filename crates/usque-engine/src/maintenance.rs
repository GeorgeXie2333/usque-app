use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use crate::logging::{log_directory, sanitize_log_bytes};

use chrono::Utc;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use thiserror::Error;
use tokio::sync::Mutex;
use usque_core::{
    AppConfig, ConnectionSnapshot, DiagnosticSession, TransportFailure,
    update::{UpdateChecker, UpdateError, UpdateInfo},
};
use usque_transport::{ConnectionEventType, ConnectionTimelineSnapshot};

const MAX_DIAGNOSTIC_LOG_BYTES: usize = 2 * 1024 * 1024;

pub struct Maintenance {
    update_checker: UpdateChecker,
    legacy_update_state_path: PathBuf,
    log_directory: PathBuf,
    flag_cache_directory: PathBuf,
    config_backup_path: PathBuf,
    update_lock: Mutex<()>,
}

impl Maintenance {
    pub fn new(config_path: &Path) -> Self {
        let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
        let legacy_update_state_path = parent.join("update-state-v1.json");
        let _ = remove_file_if_present(&legacy_update_state_path);
        Self {
            update_checker: UpdateChecker::new()
                .expect("the static GitHub update client configuration must be valid"),
            legacy_update_state_path,
            log_directory: log_directory(config_path),
            flag_cache_directory: parent.join("cache").join("flag-icons-7.5.0"),
            config_backup_path: config_path.with_extension("json.bak"),
            update_lock: Mutex::new(()),
        }
    }

    pub async fn check_update(
        &self,
        manual: bool,
        enabled: bool,
    ) -> Result<UpdateInfo, MaintenanceError> {
        if !manual && !enabled {
            return Ok(UpdateInfo::current());
        }
        let _guard = self.update_lock.lock().await;
        // Older releases cached automatic checks for 24 hours. A launch now
        // always performs a fresh request; remove the obsolete cache without
        // allowing cleanup failure to block discovery.
        let _ = remove_file_if_present(&self.legacy_update_state_path);
        Ok(self.update_checker.check(env!("CARGO_PKG_VERSION")).await?)
    }

    pub async fn export_diagnostics(
        &self,
        destination: PathBuf,
        config: AppConfig,
        snapshot: ConnectionSnapshot,
        diagnostic_session: Option<DiagnosticSession>,
        timeline: ConnectionTimelineSnapshot,
    ) -> Result<(), MaintenanceError> {
        let log_directory = self.log_directory.clone();
        tokio::task::spawn_blocking(move || {
            write_diagnostic_bundle(
                &destination,
                &config,
                &snapshot,
                diagnostic_session.as_ref(),
                &timeline,
                &log_directory,
            )
        })
        .await
        .map_err(|error| MaintenanceError::Worker(error.to_string()))?
    }

    pub async fn clear_local_state(&self) -> Result<(), MaintenanceError> {
        let update_state_path = self.legacy_update_state_path.clone();
        let log_directory = self.log_directory.clone();
        let flag_cache_directory = self.flag_cache_directory.clone();
        let config_backup_path = self.config_backup_path.clone();
        tokio::task::spawn_blocking(move || {
            remove_file_if_present(&update_state_path)?;
            remove_file_if_present(&config_backup_path)?;
            if flag_cache_directory.is_dir() {
                fs::remove_dir_all(&flag_cache_directory)?;
            }
            clear_engine_logs(&log_directory)
        })
        .await
        .map_err(|error| MaintenanceError::Worker(error.to_string()))??;
        Ok(())
    }
}

fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn clear_engine_logs(directory: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "engine.jsonl" {
            OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(entry.path())?;
        } else if name.starts_with("engine-") && name.ends_with(".jsonl") {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn write_diagnostic_bundle(
    destination: &Path,
    config: &AppConfig,
    snapshot: &ConnectionSnapshot,
    diagnostic_session: Option<&DiagnosticSession>,
    timeline: &ConnectionTimelineSnapshot,
    log_directory: &Path,
) -> Result<(), MaintenanceError> {
    if !destination.is_absolute()
        || !destination
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        return Err(MaintenanceError::InvalidDestination(destination.to_owned()));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| MaintenanceError::InvalidDestination(destination.to_owned()))?;
    if !parent.is_dir() {
        return Err(MaintenanceError::InvalidDestination(destination.to_owned()));
    }

    let log = collect_sanitized_logs(log_directory)?;
    let configuration = configuration_summary(config);
    let connection = connection_summary(snapshot);
    let timeline = connection_timeline_summary(timeline);
    let platform = platform_health_summary(snapshot);
    let readme = concat!(
        "Usque diagnostic bundle\n\n",
        "This archive is created locally and is never uploaded automatically.\n",
        "Identity secrets, cryptographic material, full network addresses, and ",
        "user-provided profile names are deliberately excluded.\n"
    );

    let mut entries = vec![
        (
            "configuration-summary.json".to_owned(),
            serde_json::to_vec_pretty(&configuration)?.into_boxed_slice(),
        ),
        (
            "connection-summary.json".to_owned(),
            serde_json::to_vec_pretty(&connection)?.into_boxed_slice(),
        ),
        (
            "connection-timeline.json".to_owned(),
            serde_json::to_vec_pretty(&timeline)?.into_boxed_slice(),
        ),
        (
            "platform-health.json".to_owned(),
            serde_json::to_vec_pretty(&platform)?.into_boxed_slice(),
        ),
        (
            "README.txt".to_owned(),
            readme.as_bytes().to_vec().into_boxed_slice(),
        ),
    ];
    if let Some(session) = diagnostic_session {
        entries.push((
            "diagnostic-session.json".to_owned(),
            serde_json::to_vec_pretty(&diagnostic_session_summary(session))?.into_boxed_slice(),
        ));
    }
    if !log.is_empty() {
        entries.push(("logs/engine.jsonl".to_owned(), log.into_boxed_slice()));
    }
    let contents = entries
        .iter()
        .map(|(name, bytes)| {
            serde_json::json!({
                "path": name,
                "size": bytes.len(),
                "sha256": sha256_hex(bytes),
            })
        })
        .collect::<Vec<_>>();
    let manifest = serde_json::json!({
        "schema_version": 2,
        "created_at": Utc::now(),
        "app_version": env!("CARGO_PKG_VERSION"),
        "operating_system": std::env::consts::OS,
        "architecture": std::env::consts::ARCH,
        "diagnostic_complete": diagnostic_session.is_some_and(|session| {
            session.state == usque_core::DiagnosticSessionState::Completed
        }),
        "diagnostic_cancelled": diagnostic_session.is_some_and(|session| {
            session.state == usque_core::DiagnosticSessionState::Cancelled
        }),
        "sanitization_policy": "allowlist-v2",
        "contents": contents,
        "excluded": [
            "WARP Secret",
            "private key",
            "token and organization credential",
            "Cloudflare Access assertion",
            "Zero Trust callback URL",
            "device identifier",
            "license",
            "endpoint pin",
            "full IP address and hostname",
            "custom endpoint and DNS address",
            "profile name and split-exclusion CIDR",
            "Windows user directory",
            "Android package list and SSID"
        ]
    });
    entries.insert(
        0,
        (
            "manifest.json".to_owned(),
            serde_json::to_vec_pretty(&manifest)?.into_boxed_slice(),
        ),
    );
    let mut temporary = NamedTempFile::new_in(parent)?;
    write_stored_zip(&mut temporary, &entries)?;
    temporary.as_file().sync_all()?;
    replace_file(temporary.path(), destination)?;
    let _ = temporary.keep();
    Ok(())
}

fn configuration_summary(config: &AppConfig) -> serde_json::Value {
    let profiles = config
        .runtime_profiles()
        .into_iter()
        .enumerate()
        .map(|(index, profile)| {
            serde_json::json!({
                "profile": index + 1,
                "active": config.active_profile_id == Some(profile.id),
                "mode": profile.mode,
                "transport": profile.transport,
                "ip_policy": profile.ip_policy,
                "mtu": profile.mtu,
                "dns_mode": profile.dns_mode,
                "dns_server_count": profile.dns_servers.len(),
                "allow_lan": profile.allow_lan,
                "split_exclusion_count": profile.split_exclusions.len(),
                "kill_switch": profile.kill_switch,
                "auto_connect": profile.auto_connect,
                "endpoint": {
                    "uses_default_ipv4": profile.endpoint.ipv4
                        == usque_core::config::DEFAULT_ENDPOINT_V4,
                    "uses_default_ipv6": profile.endpoint.ipv6
                        == usque_core::config::DEFAULT_ENDPOINT_V6,
                    "port": profile.endpoint.port,
                    "uses_default_sni": profile.endpoint.sni
                        == usque_core::config::DEFAULT_SNI,
                },
                "proxy": {
                    "socks5_listener_count": profile.proxy.socks5_listeners.len(),
                    "http_listener_count": profile.proxy.http_listeners.len(),
                    "system_proxy": profile.proxy.system_proxy,
                    "dns_mode": profile.proxy.dns_mode,
                    "dns_server_count": profile.proxy.dns_servers.len(),
                    "udp_idle_timeout_seconds": profile.proxy.udp_idle_timeout_seconds,
                    "listener_auth": profile.proxy.listener_auth_username().is_some(),
                }
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema_version": config.schema_version,
        "profile_count": profiles.len(),
        "preferences": {
            "locale": config.preferences.locale,
            "theme": config.preferences.theme,
            "update_check_enabled": config.preferences.update_check_enabled,
            "log_level": config.preferences.log_level,
        },
        "profiles": profiles,
    })
}

fn connection_summary(snapshot: &ConnectionSnapshot) -> serde_json::Value {
    serde_json::json!({
        "phase": snapshot.phase,
        "changed_at": snapshot.changed_at,
        "transport": snapshot.transport,
        "address_family": snapshot.address_family,
        "ipv4_available": snapshot.ipv4_available,
        "ipv6_available": snapshot.ipv6_available,
        "statistics": snapshot.statistics,
        "exit_ipv4_observed": snapshot.exit.as_ref().and_then(|exit| exit.ipv4).is_some(),
        "exit_ipv6_observed": snapshot.exit.as_ref().and_then(|exit| exit.ipv6).is_some(),
        "error": snapshot.error.as_ref().map(|error| serde_json::json!({
            "code": error.code,
            "retryable": error.retryable,
        })),
        "failure": snapshot.failure.as_ref().map(sanitized_failure_summary),
        "kill_switch_state": snapshot.kill_switch_state,
        "lockdown_state": snapshot.lockdown_state,
        "reconnect_count": snapshot.reconnect_count,
        "active_listener_count": snapshot.active_listeners.len(),
        "warning_codes": snapshot
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>(),
    })
}

fn connection_timeline_summary(timeline: &ConnectionTimelineSnapshot) -> serde_json::Value {
    let events = timeline
        .events
        .iter()
        .map(|event| {
            serde_json::json!({
                "sequence": event.sequence,
                "elapsed_from_attempt_start_milliseconds": duration_milliseconds(
                    event.elapsed_from_attempt_start,
                ),
                "event_type": connection_event_type_name(event.event_type),
                "stage": event.stage.map(usque_core::TransportStage::as_str),
                "transport": event.transport,
                "address_family": event.address_family,
                "duration_milliseconds": event.duration.map(duration_milliseconds),
                "failure": event.failure.as_ref().map(sanitized_failure_summary),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema_version": 1,
        "events": events,
        "dropped_event_count": timeline.dropped_event_count,
        "metrics": {
            "last_connect_duration_milliseconds": timeline.metrics.last_connect_duration.map(duration_milliseconds),
            "last_h3_handshake_duration_milliseconds": timeline.metrics.last_h3_handshake_duration.map(duration_milliseconds),
            "last_h2_handshake_duration_milliseconds": timeline.metrics.last_h2_handshake_duration.map(duration_milliseconds),
            "current_smoothed_rtt_milliseconds": timeline.metrics.current_smoothed_rtt.map(duration_milliseconds),
            "reconnect_count": timeline.metrics.reconnect_count,
            "fallback_count": timeline.metrics.fallback_count,
            "network_change_count": timeline.metrics.network_change_count,
            "send_queue_high_watermark": timeline.metrics.send_queue_high_watermark,
            "send_queue_drop_count": timeline.metrics.send_queue_drop_count,
            "last_failure_code": timeline.metrics.last_failure_code.map(usque_core::TransportFailureCode::as_str),
            "last_reconnect_code": timeline.metrics.last_reconnect_code.map(usque_core::TransportFailureCode::as_str),
        }
    })
}

fn platform_health_summary(snapshot: &ConnectionSnapshot) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "operating_system": std::env::consts::OS,
        "architecture": std::env::consts::ARCH,
        "kill_switch_state": snapshot.kill_switch_state,
        "lockdown_state": snapshot.lockdown_state,
        "frontends": snapshot.frontends.iter().map(|frontend| serde_json::json!({
            "kind": frontend.kind,
            "phase": frontend.phase,
            "listener_count": frontend.listeners.len(),
            "error_code": frontend.error.as_ref().map(|error| error.code),
        })).collect::<Vec<_>>(),
        "agent_state": "unknown",
        "tun_state": "unknown",
        "route_state": "unknown",
        "dns_state": "unknown",
        "system_proxy_state": "unknown",
        "recovery_journal_state": "unknown",
        "independent_leak_verification": false,
    })
}

fn diagnostic_session_summary(session: &DiagnosticSession) -> serde_json::Value {
    let completed_after_milliseconds = session.completed_at.map(|completed| {
        completed
            .signed_duration_since(session.started_at)
            .num_milliseconds()
            .max(0)
    });
    serde_json::json!({
        "schema_version": 1,
        "session_id": session.session_id,
        "state": session.state,
        "mode": session.mode,
        "completed_after_milliseconds": completed_after_milliseconds,
        "current_check": session.current_check.as_deref().filter(|id| known_diagnostic_check(id)),
        "progress_percent": session.progress_percent,
        "summary": session.summary,
        "findings": session.findings.iter().map(|finding| {
            let started_after_milliseconds = finding.started_at.map(|started| {
                started
                    .signed_duration_since(session.started_at)
                    .num_milliseconds()
                    .max(0)
            });
            serde_json::json!({
                "check_id": if known_diagnostic_check(&finding.check_id) {
                    finding.check_id.as_str()
                } else {
                    "unknown"
                },
                "category": finding.category,
                "status": finding.status,
                "severity": finding.severity,
                "summary_key": safe_summary_key(&finding.summary_key),
                "remediation_key": safe_remediation_key(&finding.remediation_key),
                "sanitized_evidence": finding.sanitized_evidence.iter()
                    .filter(|value| safe_evidence(value))
                    .take(16)
                    .collect::<Vec<_>>(),
                "started_after_milliseconds": started_after_milliseconds,
                "duration_milliseconds": finding.duration_milliseconds,
                "dependency_reason": finding.dependency_reason.as_deref()
                    .filter(|id| known_diagnostic_check(id)),
                "failure": finding.failure.as_ref().map(sanitized_failure_summary),
            })
        }).collect::<Vec<_>>(),
    })
}

fn sanitized_failure_summary(failure: &TransportFailure) -> serde_json::Value {
    serde_json::json!({
        "code": failure.code.as_str(),
        "stage": failure.stage.as_str(),
        "transport": failure.transport,
        "address_family": failure.address_family,
        "retryable": failure.retryable,
        "fallback_allowed": failure.fallback_allowed,
        "severity": failure.severity,
        "remediation_key": safe_remediation_key(&failure.remediation_key).unwrap_or("retry"),
        "sanitized_detail": failure.sanitized_detail.as_deref()
            .filter(|detail| TransportFailure::sanitized_detail_is_safe(detail)),
    })
}

fn safe_remediation_key(value: &str) -> Option<&str> {
    matches!(
        value,
        "none"
            | "nq_profile"
            | "nq_retry"
            | "nq_network"
            | "nq_reconnect"
            | "retry"
            | "try_http2"
            | "check_physical_network"
            | "refresh_or_replace_identity"
            | "replace_identity"
            | "review_configuration"
            | "restore_platform_state"
            | "resolve_dependency"
            | "run_deep_diagnostics"
            | "run_release_leak_gate"
            | "inspect_platform_state"
            | "generate_tunnel_traffic"
            | "export_diagnostics"
            | "not_configured"
            | "platform_capability_unavailable"
            | "connect_or_run_deep_diagnostics"
            | "active_probe_requires_disconnected_deep_mode"
            | "http2_fallback_active"
            | "h3_not_active"
            | "h3_active"
            | "no_transport_handshake"
            | "no_active_runtime"
            | "no_active_tunnel"
            | "payload_family_unavailable"
    )
    .then_some(value)
}

fn safe_summary_key(value: &str) -> Option<&str> {
    matches!(
        value,
        "diagnostic_address_assignment_missing"
            | "nq_finding_unavailable"
            | "nq_finding_invalid_configuration"
            | "nq_finding_dns_system"
            | "nq_finding_unsupported"
            | "nq_finding_dns_custom_valid"
            | "nq_finding_stale"
            | "nq_finding_rtt_high"
            | "nq_finding_healthy"
            | "nq_finding_loss_high"
            | "nq_finding_queue_pressure"
            | "nq_finding_pmtu_degraded"
            | "nq_finding_migration_reconnect"
            | "nq_finding_dns_changed"
            | "nq_finding_dns_runtime"
            | "nq_finding_dns_degraded"
            | "nq_finding_probe_unsafe"
            | "nq_finding_probe_success"
            | "nq_finding_probe_cancelled"
            | "nq_finding_probe_timeout"
            | "nq_finding_probe_failed"
            | "diagnostic_address_assignment_unknown"
            | "diagnostic_address_assignment_valid"
            | "diagnostic_cancelled"
            | "diagnostic_capabilities_ok"
            | "diagnostic_check_failed_internally"
            | "diagnostic_check_timed_out"
            | "diagnostic_configuration_invalid"
            | "diagnostic_configuration_ok"
            | "diagnostic_dependency_failed"
            | "diagnostic_dns_path_actual_state_unknown"
            | "diagnostic_dns_path_consistent"
            | "diagnostic_dns_path_mismatch"
            | "diagnostic_dns_path_not_tunnel"
            | "diagnostic_egress_family_unavailable"
            | "diagnostic_endpoint_pin_mismatch"
            | "diagnostic_endpoint_pin_not_tested"
            | "diagnostic_endpoint_pin_valid"
            | "diagnostic_engine_control_ok"
            | "diagnostic_event_stream_ok"
            | "diagnostic_fallback_policy_valid"
            | "diagnostic_fallback_policy_violation"
            | "diagnostic_first_packet_not_observed"
            | "diagnostic_first_packet_observed"
            | "diagnostic_first_packet_unknown"
            | "diagnostic_frontend_disabled"
            | "diagnostic_frontend_listener_failed"
            | "diagnostic_frontend_listener_ok"
            | "diagnostic_frontend_not_configured"
            | "diagnostic_h2_not_required"
            | "diagnostic_h2_not_tested"
            | "diagnostic_h2_stage_ready"
            | "diagnostic_h3_connected"
            | "diagnostic_h3_datagram_available"
            | "diagnostic_h3_datagram_not_tested"
            | "diagnostic_h3_not_active"
            | "diagnostic_h3_not_tested"
            | "diagnostic_ipv4_egress_requires_external_observer"
            | "diagnostic_ipv4_route_available"
            | "diagnostic_ipv4_route_unavailable"
            | "diagnostic_ipv4_route_unknown"
            | "diagnostic_ipv6_egress_requires_external_observer"
            | "diagnostic_ipv6_route_available"
            | "diagnostic_ipv6_route_unavailable"
            | "diagnostic_ipv6_route_unknown"
            | "diagnostic_kill_switch_actual_state_unknown"
            | "diagnostic_kill_switch_disabled"
            | "diagnostic_kill_switch_state_consistent"
            | "diagnostic_kill_switch_state_mismatch"
            | "diagnostic_network_generation_observed"
            | "diagnostic_physical_dns_available"
            | "diagnostic_physical_dns_unavailable"
            | "diagnostic_physical_dns_unknown"
            | "diagnostic_physical_network_not_observed"
            | "diagnostic_physical_network_present"
            | "diagnostic_recovery_journal_agent_unavailable"
            | "diagnostic_recovery_journal_consistent"
            | "diagnostic_recovery_journal_not_supported"
            | "diagnostic_recovery_journal_pending_cleanup"
            | "diagnostic_requires_deep_mode"
            | "diagnostic_route_ownership_actual_state_unknown"
            | "diagnostic_route_ownership_consistent"
            | "diagnostic_route_ownership_mismatch"
            | "diagnostic_route_ownership_not_supported"
            | "diagnostic_secure_storage_available"
            | "diagnostic_secure_storage_not_supported"
            | "diagnostic_system_proxy_actual_state_unknown"
            | "diagnostic_system_proxy_disabled"
            | "diagnostic_system_proxy_lease_missing"
            | "diagnostic_system_proxy_lease_only"
            | "diagnostic_system_proxy_runtime_mismatch"
            | "diagnostic_tunnel_dns_configured"
            | "diagnostic_tunnel_dns_disabled"
            | "diagnostic_tunnel_dns_unknown"
            | "diagnostic_tunnel_routes_consistent"
            | "diagnostic_tunnel_routes_unknown"
    )
    .then_some(value)
}

fn safe_evidence(value: &str) -> bool {
    if let Some((key, number)) = value.split_once('=') {
        return matches!(
            key,
            "rtt_ms"
                | "loss_basis_points"
                | "queue_percent"
                | "queue_drops"
                | "pmtu_bytes"
                | "pmtu_failures"
                | "migration_failures"
                | "dns_successes"
                | "dns_failures"
                | "dns_timeouts"
                | "plaintext_fallback"
                | "probe_ms"
        ) && !number.is_empty()
            && number.bytes().all(|byte| byte.is_ascii_digit())
            && number.parse::<u64>().is_ok();
    }
    matches!(
        value,
        "responsive"
            | "recoverable"
            | "append_only_api"
            | "schema_valid"
            | "metadata_only"
            | "runtime_path"
            | "payload_family"
            | "runtime_started"
            | "stable"
            | "changed"
            | "active_path"
            | "verified_before_ready"
            | "typed_matrix"
            | "family_flags"
            | "configuration_consistent_not_leak_test"
            | "bidirectional"
            | "internal_state_only"
            | "agent_read_only_inspection"
            | "listener_active"
    )
}

fn known_diagnostic_check(value: &str) -> bool {
    matches!(
        value,
        "engine.control_channel"
            | "quality.rtt"
            | "quality.packet_loss"
            | "quality.queue_pressure"
            | "quality.pmtu"
            | "transport.migration_capability"
            | "dns.direct_encrypted_configuration"
            | "dns.direct_encrypted_runtime_state"
            | "dns.direct_encrypted_reachability"
            | "transport.h3_path_validation_probe"
            | "engine.event_stream"
            | "engine.capabilities"
            | "engine.configuration"
            | "engine.secure_storage_metadata"
            | "frontend.socks_port"
            | "frontend.http_port"
            | "frontend.system_proxy_state"
            | "physical.network_present"
            | "physical.ipv4_route"
            | "physical.ipv6_route"
            | "physical.dns_available"
            | "physical.network_generation"
            | "transport.h3_connect"
            | "transport.h3_datagram"
            | "transport.h2_tcp"
            | "transport.h2_tls"
            | "transport.h2_connect"
            | "transport.endpoint_pin"
            | "transport.fallback_policy"
            | "tunnel.address_assignment"
            | "tunnel.routes"
            | "tunnel.dns"
            | "tunnel.first_packet"
            | "tunnel.ipv4_egress"
            | "tunnel.ipv6_egress"
            | "protection.kill_switch"
            | "protection.dns_path"
            | "protection.route_ownership"
            | "protection.recovery_journal"
    )
}

const fn connection_event_type_name(event: ConnectionEventType) -> &'static str {
    match event {
        ConnectionEventType::AttemptStarted => "attempt_started",
        ConnectionEventType::EndpointResolved => "endpoint_resolved",
        ConnectionEventType::SocketConnected => "socket_connected",
        ConnectionEventType::TlsReady => "tls_ready",
        ConnectionEventType::QuicReady => "quic_ready",
        ConnectionEventType::MasqueAccepted => "masque_accepted",
        ConnectionEventType::PeerSettingsReceived => "peer_settings_received",
        ConnectionEventType::AddressAssigned => "address_assigned",
        ConnectionEventType::TunnelReady => "tunnel_ready",
        ConnectionEventType::FirstPacketSent => "first_packet_sent",
        ConnectionEventType::FirstPacketReceived => "first_packet_received",
        ConnectionEventType::FallbackStarted => "fallback_started",
        ConnectionEventType::ReconnectScheduled => "reconnect_scheduled",
        ConnectionEventType::NetworkChanged => "network_changed",
        ConnectionEventType::RecoveryProbeStarted => "recovery_probe_started",
        ConnectionEventType::RecoveryProbeSucceeded => "recovery_probe_succeeded",
        ConnectionEventType::RecoveryProbeFailed => "recovery_probe_failed",
        ConnectionEventType::PathPromoted => "path_promoted",
        ConnectionEventType::MigrationStarted => "migration_started",
        ConnectionEventType::MigrationPathValidated => "migration_path_validated",
        ConnectionEventType::MigrationPromoted => "migration_promoted",
        ConnectionEventType::MigrationFailed => "migration_failed",
        ConnectionEventType::QueueSaturated => "queue_saturated",
        ConnectionEventType::PmtuChanged => "pmtu_changed",
        ConnectionEventType::PmtuRevalidationStarted => "pmtu_revalidation_started",
        ConnectionEventType::PmtuRevalidationFailed => "pmtu_revalidation_failed",
        ConnectionEventType::Disconnected => "disconnected",
        ConnectionEventType::Failed => "failed",
    }
}

fn duration_milliseconds(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_stored_zip(
    writer: &mut impl Write,
    entries: &[(String, Box<[u8]>)],
) -> Result<(), MaintenanceError> {
    if entries.len() > usize::from(u16::MAX) {
        return Err(MaintenanceError::BundleTooLarge);
    }
    let mut central_entries = Vec::with_capacity(entries.len());
    let mut offset = 0_u32;
    for (name, contents) in entries {
        let name = name.as_bytes();
        let name_length =
            u16::try_from(name.len()).map_err(|_| MaintenanceError::BundleTooLarge)?;
        let content_length =
            u32::try_from(contents.len()).map_err(|_| MaintenanceError::BundleTooLarge)?;
        let crc32 = crc32(contents);
        write_u32(writer, 0x0403_4b50)?;
        write_u16(writer, 20)?;
        write_u16(writer, 0x0800)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u32(writer, crc32)?;
        write_u32(writer, content_length)?;
        write_u32(writer, content_length)?;
        write_u16(writer, name_length)?;
        write_u16(writer, 0)?;
        writer.write_all(name)?;
        writer.write_all(contents)?;

        central_entries.push((name.to_vec(), crc32, content_length, offset));
        offset = offset
            .checked_add(30)
            .and_then(|value| value.checked_add(u32::from(name_length)))
            .and_then(|value| value.checked_add(content_length))
            .ok_or(MaintenanceError::BundleTooLarge)?;
    }

    let central_offset = offset;
    for (name, crc32, content_length, local_offset) in &central_entries {
        let name_length =
            u16::try_from(name.len()).map_err(|_| MaintenanceError::BundleTooLarge)?;
        write_u32(writer, 0x0201_4b50)?;
        write_u16(writer, 0x0314)?;
        write_u16(writer, 20)?;
        write_u16(writer, 0x0800)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u32(writer, *crc32)?;
        write_u32(writer, *content_length)?;
        write_u32(writer, *content_length)?;
        write_u16(writer, name_length)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u32(writer, 0o100600 << 16)?;
        write_u32(writer, *local_offset)?;
        writer.write_all(name)?;
        offset = offset
            .checked_add(46)
            .and_then(|value| value.checked_add(u32::from(name_length)))
            .ok_or(MaintenanceError::BundleTooLarge)?;
    }
    let central_size = offset
        .checked_sub(central_offset)
        .ok_or(MaintenanceError::BundleTooLarge)?;
    let entry_count =
        u16::try_from(central_entries.len()).map_err(|_| MaintenanceError::BundleTooLarge)?;
    write_u32(writer, 0x0605_4b50)?;
    write_u16(writer, 0)?;
    write_u16(writer, 0)?;
    write_u16(writer, entry_count)?;
    write_u16(writer, entry_count)?;
    write_u32(writer, central_size)?;
    write_u32(writer, central_offset)?;
    write_u16(writer, 0)?;
    Ok(())
}

fn collect_sanitized_logs(directory: &Path) -> Result<Vec<u8>, MaintenanceError> {
    let mut files = match fs::read_dir(directory) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !(name == "engine.jsonl"
                    || (name.starts_with("engine-") && name.ends_with(".jsonl")))
                {
                    return None;
                }
                let metadata = fs::symlink_metadata(entry.path()).ok()?;
                metadata.file_type().is_file().then_some((
                    entry.path(),
                    metadata.modified().unwrap_or(std::time::UNIX_EPOCH),
                    metadata.len(),
                ))
            })
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    files.sort_by_key(|(_, modified, _)| *modified);

    let mut selected = Vec::new();
    let mut selected_bytes = 0_u64;
    for file in files.into_iter().rev() {
        if selected_bytes >= MAX_DIAGNOSTIC_LOG_BYTES as u64 {
            break;
        }
        selected_bytes = selected_bytes.saturating_add(file.2);
        selected.push(file);
    }
    selected.reverse();

    let mut output = Vec::new();
    for (path, _, length) in selected {
        let remaining = MAX_DIAGNOSTIC_LOG_BYTES.saturating_sub(output.len());
        if remaining == 0 {
            break;
        }
        let mut file = File::open(path)?;
        if length > remaining as u64 {
            file.seek(SeekFrom::End(-(remaining as i64)))?;
        }
        let mut source = Vec::with_capacity(remaining);
        file.take(remaining as u64).read_to_end(&mut source)?;
        if length > remaining as u64
            && let Some(first_newline) = source.iter().position(|byte| *byte == b'\n')
        {
            source.drain(..=first_newline);
        }
        for line in source.split(|byte| *byte == b'\n') {
            let sanitized = sanitize_log_bytes(line);
            if sanitized.is_empty() {
                continue;
            }
            if output.len().saturating_add(sanitized.len() + 1) > MAX_DIAGNOSTIC_LOG_BYTES {
                return Ok(output);
            }
            output.extend_from_slice(&sanitized);
            output.push(b'\n');
        }
    }
    Ok(output)
}

fn write_u16(writer: &mut impl Write, value: u16) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u32(writer: &mut impl Write, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: source and destination are null-terminated wide paths that outlive
    // the synchronous MoveFileExW call.
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum MaintenanceError {
    #[error("update check failed: {0}")]
    Update(#[from] UpdateError),
    #[error(
        "diagnostic destination must be an absolute path to an existing directory and end in .zip: {0}"
    )]
    InvalidDestination(PathBuf),
    #[error("maintenance I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("maintenance JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("the diagnostic bundle exceeded the classic ZIP safety limit")]
    BundleTooLarge,
    #[error("maintenance worker failed: {0}")]
    Worker(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_doctor_export_allowlist_accepts_numbers_but_no_private_text() {
        for id in [
            "quality.rtt",
            "quality.packet_loss",
            "quality.queue_pressure",
            "quality.pmtu",
            "transport.migration_capability",
            "dns.direct_encrypted_configuration",
            "dns.direct_encrypted_runtime_state",
            "dns.direct_encrypted_reachability",
            "transport.h3_path_validation_probe",
        ] {
            assert!(known_diagnostic_check(id));
        }
        assert_eq!(
            safe_summary_key("nq_finding_dns_runtime"),
            Some("nq_finding_dns_runtime")
        );
        assert_eq!(safe_remediation_key("nq_profile"), Some("nq_profile"));
        for evidence in [
            "rtt_ms=42",
            "plaintext_fallback=0",
            "probe_ms=300",
            "queue_drops=0",
        ] {
            assert!(safe_evidence(evidence));
        }
        for evidence in [
            "resolver=private.example",
            "rtt_ms=192.0.2.1",
            "probe_ms=",
            "queue_drops=-1",
            "probe_ms=99999999999999999999999",
            "probe_ms=10\nsecret",
        ] {
            assert!(!safe_evidence(evidence));
        }
    }
    use usque_core::{
        DiagnosticCategory, DiagnosticCheckStatus, DiagnosticFinding, DiagnosticMode,
        DiagnosticSessionState,
    };

    #[test]
    fn diagnostic_bundle_contains_only_sanitized_summaries() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("diagnostics.zip");
        let mut config = AppConfig::default();
        config.profiles[0].name = "private hotel name".to_owned();
        config.network.endpoint.sni = "private.example".to_owned();
        let log_directory = directory.path().join("logs");
        fs::create_dir_all(&log_directory).unwrap();
        fs::write(
            log_directory.join("engine.jsonl"),
            br#"{"peer":"192.0.2.1:443","message":"failed example.com"}"#,
        )
        .unwrap();
        write_diagnostic_bundle(
            &destination,
            &config,
            &ConnectionSnapshot::default(),
            None,
            &ConnectionTimelineSnapshot::default(),
            &log_directory,
        )
        .unwrap();

        let combined = String::from_utf8_lossy(&fs::read(destination).unwrap()).into_owned();
        assert!(!combined.contains("private hotel name"));
        assert!(!combined.contains("private.example"));
        assert!(!combined.contains("192.0.2.1"));
        assert!(!combined.contains("example.com"));
        assert!(combined.contains("uses_default_sni"));
        assert!(combined.contains("WARP Secret"));
    }

    #[test]
    fn diagnostic_bundle_rejects_relative_or_non_zip_destinations() {
        assert!(matches!(
            write_diagnostic_bundle(
                Path::new("diagnostics.zip"),
                &AppConfig::default(),
                &ConnectionSnapshot::default(),
                None,
                &ConnectionTimelineSnapshot::default(),
                Path::new("missing-logs")
            ),
            Err(MaintenanceError::InvalidDestination(_))
        ));
    }

    #[test]
    fn inv_export_sanitized_rejects_hostile_diagnostic_session_values() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("diagnostics.zip");
        let mut finding =
            DiagnosticFinding::pending("transport.h3_connect", DiagnosticCategory::Transport);
        finding.status = DiagnosticCheckStatus::Failed;
        finding.summary_key = "private.example".to_owned();
        finding.remediation_key = r"C:\Users\private\secret".to_owned();
        finding.sanitized_evidence = vec![
            "active_path".to_owned(),
            "192.0.2.44".to_owned(),
            "token=supersecret".to_owned(),
            "private.example".to_owned(),
        ];
        finding.dependency_reason = Some("private.example".to_owned());
        let mut hostile_failure = TransportFailure::new(
            usque_core::TransportFailureCode::Internal,
            usque_core::TransportStage::Diagnostics,
        );
        hostile_failure.remediation_key = "private_remediation".to_owned();
        hostile_failure.sanitized_detail = Some("rawsecret".to_owned());
        finding.failure = Some(hostile_failure);
        let mut session = DiagnosticSession::pending(DiagnosticMode::Deep, vec![finding]);
        session.state = DiagnosticSessionState::Completed;
        session.completed_at = Some(session.started_at + chrono::Duration::milliseconds(25));
        session.current_check = Some("private.example".to_owned());
        session.recompute_summary();

        write_diagnostic_bundle(
            &destination,
            &AppConfig::default(),
            &ConnectionSnapshot::default(),
            Some(&session),
            &ConnectionTimelineSnapshot::default(),
            directory.path().join("missing-logs").as_path(),
        )
        .unwrap();

        let combined = String::from_utf8_lossy(&fs::read(destination).unwrap()).into_owned();
        assert!(combined.contains("active_path"));
        for private in [
            "192.0.2.44",
            "token=supersecret",
            "private.example",
            r"C:\Users\private\secret",
            "private_remediation",
            "rawsecret",
        ] {
            assert!(!combined.contains(private), "bundle leaked {private}");
        }
    }

    #[tokio::test]
    async fn clear_local_state_removes_caches_backups_and_rotated_logs() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.json");
        let maintenance = Maintenance::new(&config_path);
        fs::write(directory.path().join("update-state-v1.json"), b"cached").unwrap();
        fs::write(config_path.with_extension("json.bak"), b"backup").unwrap();
        let flag_cache = directory.path().join("cache").join("flag-icons-7.5.0");
        fs::create_dir_all(&flag_cache).unwrap();
        fs::write(flag_cache.join("us.svg"), b"<svg/>").unwrap();
        let logs = directory.path().join("logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(logs.join("engine.jsonl"), b"active").unwrap();
        fs::write(logs.join("engine-1-0.jsonl"), b"rotated").unwrap();

        maintenance.clear_local_state().await.unwrap();

        assert!(!directory.path().join("update-state-v1.json").exists());
        assert!(!config_path.with_extension("json.bak").exists());
        assert!(!flag_cache.exists());
        assert_eq!(fs::read(logs.join("engine.jsonl")).unwrap(), b"");
        assert!(!logs.join("engine-1-0.jsonl").exists());
    }
}
