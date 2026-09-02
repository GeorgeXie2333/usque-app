import '../models/diagnostics_models.dart';
import 'app_strings.dart';

String diagnosticStatusLabel(AppStrings strings, DiagnosticCheckStatus status) {
  return strings.get(switch (status) {
    DiagnosticCheckStatus.pending => 'diag_status_pending',
    DiagnosticCheckStatus.running => 'diag_status_running',
    DiagnosticCheckStatus.passed => 'diag_status_passed',
    DiagnosticCheckStatus.warning => 'diag_status_warning',
    DiagnosticCheckStatus.failed => 'diag_status_failed',
    DiagnosticCheckStatus.skipped => 'diag_status_skipped',
    DiagnosticCheckStatus.cancelled => 'diag_status_cancelled',
  });
}

String diagnosticCategoryLabel(
  AppStrings strings,
  DiagnosticCategory category,
) {
  return strings.get(switch (category) {
    DiagnosticCategory.localComponent => 'diag_cat_local',
    DiagnosticCategory.physicalNetwork => 'diag_cat_physical',
    DiagnosticCategory.transport => 'diag_cat_transport',
    DiagnosticCategory.tunnel => 'diag_cat_tunnel',
    DiagnosticCategory.protection => 'diag_cat_protection',
    DiagnosticCategory.recovery => 'diag_cat_recovery',
  });
}

String diagnosticCheckLabel(AppStrings strings, String checkId) {
  return _catalogOrHumanize(
    strings,
    'diag_check_${checkId.replaceAll('.', '_')}',
    checkId.split('.').last,
  );
}

String diagnosticFailureTitle(AppStrings strings, String code) {
  return _catalogOrHumanize(strings, 'diag_fail_$code', code);
}

String diagnosticRemediation(AppStrings strings, String key) {
  if (key == 'none' || key.isEmpty) {
    return strings.get('diag_fix_none');
  }
  final catalogKey = 'diag_fix_$key';
  final value = strings.get(catalogKey);
  if (value == catalogKey) {
    return strings.get('diag_fix_default');
  }
  return value;
}

String diagnosticFindingSummary(AppStrings strings, DiagnosticFinding finding) {
  if (finding.summaryKey.startsWith('nq_finding_')) {
    final localized = strings.get(finding.summaryKey);
    if (localized != finding.summaryKey) return localized;
  }
  return strings.get(switch (finding.status) {
    DiagnosticCheckStatus.passed => 'diag_finding_passed',
    DiagnosticCheckStatus.warning => 'diag_finding_attention',
    DiagnosticCheckStatus.failed => 'diag_finding_failed',
    DiagnosticCheckStatus.skipped => 'diag_finding_skipped',
    DiagnosticCheckStatus.cancelled => 'diag_finding_cancelled',
    DiagnosticCheckStatus.running => 'diag_finding_running',
    DiagnosticCheckStatus.pending => 'diag_finding_pending',
  });
}

String diagnosticSessionStateLabel(
  AppStrings strings,
  DiagnosticSessionState state,
) {
  return strings.get(switch (state) {
    DiagnosticSessionState.pending => 'diag_session_pending',
    DiagnosticSessionState.running => 'diag_session_running',
    DiagnosticSessionState.cancelling => 'diag_session_cancelling',
    DiagnosticSessionState.completed => 'diag_session_completed',
    DiagnosticSessionState.failed => 'diag_session_failed',
    DiagnosticSessionState.cancelled => 'diag_session_cancelled',
  });
}

String connectionEventLabel(
  AppStrings strings,
  ConnectionTimelineEventType type,
) {
  return strings.get(switch (type) {
    ConnectionTimelineEventType.attemptStarted => 'diag_event_attempt_started',
    ConnectionTimelineEventType.endpointResolved =>
      'diag_event_endpoint_resolved',
    ConnectionTimelineEventType.socketConnected =>
      'diag_event_socket_connected',
    ConnectionTimelineEventType.tlsReady => 'diag_event_tls_ready',
    ConnectionTimelineEventType.quicReady => 'diag_event_quic_ready',
    ConnectionTimelineEventType.masqueAccepted => 'diag_event_masque_accepted',
    ConnectionTimelineEventType.peerSettingsReceived =>
      'diag_event_peer_settings_received',
    ConnectionTimelineEventType.addressAssigned =>
      'diag_event_address_assigned',
    ConnectionTimelineEventType.tunnelReady => 'diag_event_tunnel_ready',
    ConnectionTimelineEventType.firstPacketSent =>
      'diag_event_first_packet_sent',
    ConnectionTimelineEventType.firstPacketReceived =>
      'diag_event_first_packet_received',
    ConnectionTimelineEventType.fallbackStarted =>
      'diag_event_fallback_started',
    ConnectionTimelineEventType.reconnectScheduled =>
      'diag_event_reconnect_scheduled',
    ConnectionTimelineEventType.networkChanged => 'diag_event_network_changed',
    ConnectionTimelineEventType.recoveryProbeStarted =>
      'diag_event_recovery_probe_started',
    ConnectionTimelineEventType.recoveryProbeSucceeded =>
      'diag_event_recovery_probe_succeeded',
    ConnectionTimelineEventType.recoveryProbeFailed =>
      'diag_event_recovery_probe_failed',
    ConnectionTimelineEventType.pathPromoted => 'diag_event_path_promoted',
    ConnectionTimelineEventType.queueSaturated => 'diag_event_queue_saturated',
    ConnectionTimelineEventType.disconnected => 'diag_event_disconnected',
    ConnectionTimelineEventType.failed => 'diag_event_failed',
    ConnectionTimelineEventType.migrationStarted =>
      'diag_event_recovery_probe_started',
    ConnectionTimelineEventType.migrationPathValidated =>
      'diag_event_recovery_probe_succeeded',
    ConnectionTimelineEventType.migrationPromoted => 'diag_event_path_promoted',
    ConnectionTimelineEventType.migrationFailed =>
      'diag_event_recovery_probe_failed',
    ConnectionTimelineEventType.pmtuChanged => 'diag_event_path_promoted',
    ConnectionTimelineEventType.pmtuRevalidationStarted =>
      'diag_event_recovery_probe_started',
    ConnectionTimelineEventType.pmtuRevalidationFailed =>
      'diag_event_recovery_probe_failed',
    ConnectionTimelineEventType.directDnsDegraded =>
      'diag_event_network_changed',
    ConnectionTimelineEventType.directDnsRecovered =>
      'diag_event_recovery_probe_succeeded',
    ConnectionTimelineEventType.unknown => 'diag_event_failed',
  });
}

String _catalogOrHumanize(AppStrings strings, String key, String fallback) {
  final value = strings.get(key);
  if (value == key) {
    return _humanize(fallback);
  }
  return value;
}

String _humanize(String value) {
  final words = value.replaceAll(RegExp(r'[_\-.]+'), ' ').trim().toLowerCase();
  if (words.isEmpty) {
    return value;
  }
  return '${words[0].toUpperCase()}${words.substring(1)}';
}
