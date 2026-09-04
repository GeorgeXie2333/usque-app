import 'package:flutter/foundation.dart';

enum DiagnosticMode { standard, deep }

enum DiagnosticSessionState {
  pending,
  running,
  cancelling,
  completed,
  failed,
  cancelled,
}

enum DiagnosticCheckStatus {
  pending,
  running,
  passed,
  warning,
  failed,
  skipped,
  cancelled,
}

enum DiagnosticCategory {
  localComponent,
  physicalNetwork,
  transport,
  tunnel,
  protection,
  recovery,
}

enum DiagnosticSeverity { info, warning, error, critical }

enum DiagnosticsControllerState {
  idle,
  starting,
  running,
  cancelling,
  completed,
  failed,
}

@immutable
class TransportFailureInfo {
  const TransportFailureInfo({
    required this.code,
    required this.stage,
    this.transport,
    this.addressFamily,
    this.retryable = false,
    this.fallbackAllowed = false,
    this.severity = DiagnosticSeverity.error,
    this.remediationKey = '',
    this.sanitizedDetail,
  });

  final String code;
  final String stage;
  final String? transport;
  final String? addressFamily;
  final bool retryable;
  final bool fallbackAllowed;
  final DiagnosticSeverity severity;
  final String remediationKey;
  final String? sanitizedDetail;

  factory TransportFailureInfo.fromMap(Map<Object?, Object?> map) {
    return TransportFailureInfo(
      code: map['code'] as String? ?? 'INTERNAL',
      stage: map['stage'] as String? ?? 'diagnostics',
      transport: map['transport'] as String?,
      addressFamily: map['address_family'] as String?,
      retryable: map['retryable'] as bool? ?? false,
      fallbackAllowed: map['fallback_allowed'] as bool? ?? false,
      severity: _enumByName(
        DiagnosticSeverity.values,
        map['severity'] as String?,
        DiagnosticSeverity.error,
      ),
      remediationKey: map['remediation_key'] as String? ?? '',
      sanitizedDetail: map['sanitized_detail'] as String?,
    );
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is TransportFailureInfo &&
            code == other.code &&
            stage == other.stage &&
            transport == other.transport &&
            addressFamily == other.addressFamily &&
            retryable == other.retryable &&
            fallbackAllowed == other.fallbackAllowed &&
            severity == other.severity &&
            remediationKey == other.remediationKey &&
            sanitizedDetail == other.sanitizedDetail;
  }

  @override
  int get hashCode => Object.hash(
    code,
    stage,
    transport,
    addressFamily,
    retryable,
    fallbackAllowed,
    severity,
    remediationKey,
    sanitizedDetail,
  );
}

@immutable
class DiagnosticFinding {
  const DiagnosticFinding({
    required this.checkId,
    required this.category,
    required this.status,
    this.failure,
    this.severity = DiagnosticSeverity.info,
    this.summaryKey = '',
    this.remediationKey = '',
    this.sanitizedEvidence = const <String>[],
    this.startedAt,
    this.durationMilliseconds,
    this.dependencyReason,
  });

  final String checkId;
  final DiagnosticCategory category;
  final DiagnosticCheckStatus status;
  final TransportFailureInfo? failure;
  final DiagnosticSeverity severity;
  final String summaryKey;
  final String remediationKey;
  final List<String> sanitizedEvidence;
  final DateTime? startedAt;
  final int? durationMilliseconds;
  final String? dependencyReason;

  factory DiagnosticFinding.fromMap(Map<Object?, Object?> map) {
    final failure = map['failure'];
    return DiagnosticFinding(
      checkId: map['check_id'] as String? ?? '',
      category: _enumByName(
        DiagnosticCategory.values,
        map['category'] as String?,
        DiagnosticCategory.localComponent,
      ),
      status: _enumByName(
        DiagnosticCheckStatus.values,
        map['status'] as String?,
        DiagnosticCheckStatus.pending,
      ),
      failure: failure is Map
          ? TransportFailureInfo.fromMap(Map<Object?, Object?>.from(failure))
          : null,
      severity: _enumByName(
        DiagnosticSeverity.values,
        map['severity'] as String?,
        DiagnosticSeverity.info,
      ),
      summaryKey: map['summary_key'] as String? ?? '',
      remediationKey: map['remediation_key'] as String? ?? '',
      sanitizedEvidence:
          (map['sanitized_evidence'] as List?)?.whereType<String>().toList(
            growable: false,
          ) ??
          const <String>[],
      startedAt: _dateFromMilliseconds(map['started_at_unix_milliseconds']),
      durationMilliseconds: (map['duration_milliseconds'] as num?)?.toInt(),
      dependencyReason: map['dependency_reason'] as String?,
    );
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is DiagnosticFinding &&
            checkId == other.checkId &&
            category == other.category &&
            status == other.status &&
            failure == other.failure &&
            severity == other.severity &&
            summaryKey == other.summaryKey &&
            remediationKey == other.remediationKey &&
            listEquals(sanitizedEvidence, other.sanitizedEvidence) &&
            startedAt == other.startedAt &&
            durationMilliseconds == other.durationMilliseconds &&
            dependencyReason == other.dependencyReason;
  }

  @override
  int get hashCode => Object.hashAll(<Object?>[
    checkId,
    category,
    status,
    failure,
    severity,
    summaryKey,
    remediationKey,
    Object.hashAll(sanitizedEvidence),
    startedAt,
    durationMilliseconds,
    dependencyReason,
  ]);
}

@immutable
class DiagnosticSummary {
  const DiagnosticSummary({
    this.passed = 0,
    this.warnings = 0,
    this.failed = 0,
    this.skipped = 0,
    this.cancelled = 0,
  });

  final int passed;
  final int warnings;
  final int failed;
  final int skipped;
  final int cancelled;

  factory DiagnosticSummary.fromMap(Map<Object?, Object?> map) {
    return DiagnosticSummary(
      passed: (map['passed'] as num?)?.toInt() ?? 0,
      warnings: (map['warnings'] as num?)?.toInt() ?? 0,
      failed: (map['failed'] as num?)?.toInt() ?? 0,
      skipped: (map['skipped'] as num?)?.toInt() ?? 0,
      cancelled: (map['cancelled'] as num?)?.toInt() ?? 0,
    );
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is DiagnosticSummary &&
            passed == other.passed &&
            warnings == other.warnings &&
            failed == other.failed &&
            skipped == other.skipped &&
            cancelled == other.cancelled;
  }

  @override
  int get hashCode => Object.hash(passed, warnings, failed, skipped, cancelled);
}

@immutable
class DiagnosticSession {
  const DiagnosticSession({
    required this.sessionId,
    required this.state,
    required this.startedAt,
    required this.mode,
    this.completedAt,
    this.currentCheck,
    this.progressPercent = 0,
    this.findings = const <DiagnosticFinding>[],
    this.summary = const DiagnosticSummary(),
  });

  final String sessionId;
  final DiagnosticSessionState state;
  final DateTime startedAt;
  final DateTime? completedAt;
  final DiagnosticMode mode;
  final String? currentCheck;
  final int progressPercent;
  final List<DiagnosticFinding> findings;
  final DiagnosticSummary summary;

  bool get isActive =>
      state == DiagnosticSessionState.pending ||
      state == DiagnosticSessionState.running ||
      state == DiagnosticSessionState.cancelling;

  factory DiagnosticSession.fromMap(Map<Object?, Object?> map) {
    final summary = map['summary'];
    return DiagnosticSession(
      sessionId: map['session_id'] as String? ?? '',
      state: _enumByName(
        DiagnosticSessionState.values,
        map['state'] as String?,
        DiagnosticSessionState.failed,
      ),
      startedAt:
          _dateFromMilliseconds(map['started_at_unix_milliseconds']) ??
          DateTime.fromMillisecondsSinceEpoch(0, isUtc: true),
      completedAt: _dateFromMilliseconds(map['completed_at_unix_milliseconds']),
      mode: _enumByName(
        DiagnosticMode.values,
        map['mode'] as String?,
        DiagnosticMode.standard,
      ),
      currentCheck: map['current_check'] as String?,
      progressPercent: ((map['progress_percent'] as num?)?.toInt() ?? 0).clamp(
        0,
        100,
      ),
      findings:
          (map['findings'] as List?)
              ?.whereType<Map<Object?, Object?>>()
              .map(
                (value) => DiagnosticFinding.fromMap(
                  Map<Object?, Object?>.from(value),
                ),
              )
              .toList(growable: false) ??
          const <DiagnosticFinding>[],
      summary: summary is Map
          ? DiagnosticSummary.fromMap(Map<Object?, Object?>.from(summary))
          : const DiagnosticSummary(),
    );
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is DiagnosticSession &&
            sessionId == other.sessionId &&
            state == other.state &&
            startedAt == other.startedAt &&
            completedAt == other.completedAt &&
            mode == other.mode &&
            currentCheck == other.currentCheck &&
            progressPercent == other.progressPercent &&
            listEquals(findings, other.findings) &&
            summary == other.summary;
  }

  @override
  int get hashCode => Object.hashAll(<Object?>[
    sessionId,
    state,
    startedAt,
    completedAt,
    mode,
    currentCheck,
    progressPercent,
    Object.hashAll(findings),
    summary,
  ]);
}

enum ConnectionTimelineEventType {
  attemptStarted,
  endpointResolved,
  socketConnected,
  tlsReady,
  quicReady,
  masqueAccepted,
  peerSettingsReceived,
  addressAssigned,
  tunnelReady,
  firstPacketSent,
  firstPacketReceived,
  fallbackStarted,
  reconnectScheduled,
  networkChanged,
  recoveryProbeStarted,
  recoveryProbeSucceeded,
  recoveryProbeFailed,
  pathPromoted,
  queueSaturated,
  disconnected,
  failed,
  migrationStarted,
  migrationPathValidated,
  migrationPromoted,
  migrationFailed,
  pmtuChanged,
  pmtuRevalidationStarted,
  pmtuRevalidationFailed,
  directDnsDegraded,
  directDnsRecovered,
  unknown,
}

@immutable
class ConnectionTimelineEvent {
  const ConnectionTimelineEvent({
    required this.sequence,
    required this.elapsedMilliseconds,
    required this.eventType,
    this.timestamp,
    this.stage,
    this.transport,
    this.addressFamily,
    this.durationMilliseconds,
    this.failure,
  });

  final int sequence;
  final DateTime? timestamp;
  final int elapsedMilliseconds;
  final ConnectionTimelineEventType eventType;
  final String? stage;
  final String? transport;
  final String? addressFamily;
  final int? durationMilliseconds;
  final TransportFailureInfo? failure;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is ConnectionTimelineEvent &&
            sequence == other.sequence &&
            timestamp == other.timestamp &&
            elapsedMilliseconds == other.elapsedMilliseconds &&
            eventType == other.eventType &&
            stage == other.stage &&
            transport == other.transport &&
            addressFamily == other.addressFamily &&
            durationMilliseconds == other.durationMilliseconds &&
            failure == other.failure;
  }

  @override
  int get hashCode => Object.hash(
    sequence,
    timestamp,
    elapsedMilliseconds,
    eventType,
    stage,
    transport,
    addressFamily,
    durationMilliseconds,
    failure,
  );
}

@immutable
class ConnectionMetrics {
  const ConnectionMetrics({
    this.lastConnectDurationMilliseconds,
    this.lastH3HandshakeDurationMilliseconds,
    this.lastH2HandshakeDurationMilliseconds,
    this.currentSmoothedRttMilliseconds,
    this.reconnectCount = 0,
    this.fallbackCount = 0,
    this.networkChangeCount = 0,
    this.sendQueueHighWatermark = 0,
    this.sendQueueDropCount = 0,
    this.lastFailureCode,
    this.lastReconnectCode,
  });

  final int? lastConnectDurationMilliseconds;
  final int? lastH3HandshakeDurationMilliseconds;
  final int? lastH2HandshakeDurationMilliseconds;
  final int? currentSmoothedRttMilliseconds;
  final int reconnectCount;
  final int fallbackCount;
  final int networkChangeCount;
  final int sendQueueHighWatermark;
  final int sendQueueDropCount;
  final String? lastFailureCode;
  final String? lastReconnectCode;
}

@immutable
class ConnectionTimeline {
  const ConnectionTimeline({
    this.events = const <ConnectionTimelineEvent>[],
    this.metrics = const ConnectionMetrics(),
    this.droppedEventCount = 0,
  });

  final List<ConnectionTimelineEvent> events;
  final ConnectionMetrics metrics;
  final int droppedEventCount;
}

ConnectionTimeline connectionTimelineFromMap(Map<Object?, Object?> map) {
  final metricsMap = map['metrics'];
  final metrics = metricsMap is Map
      ? Map<Object?, Object?>.from(metricsMap)
      : const <Object?, Object?>{};
  int? optionalInt(String key) {
    final value = (metrics[key] as num?)?.toInt() ?? 0;
    return value == 0 ? null : value;
  }

  final events =
      (map['events'] as List?)
          ?.whereType<Map<Object?, Object?>>()
          .map((raw) {
            final event = Map<Object?, Object?>.from(raw);
            final failure = event['failure'];
            return ConnectionTimelineEvent(
              sequence: (event['sequence'] as num?)?.toInt() ?? 0,
              timestamp: _dateFromMilliseconds(
                event['timestamp_unix_milliseconds'],
              ),
              elapsedMilliseconds:
                  (event['elapsed_from_attempt_start_milliseconds'] as num?)
                      ?.toInt() ??
                  0,
              eventType: _enumByName(
                ConnectionTimelineEventType.values,
                event['event_type'] as String?,
                ConnectionTimelineEventType.unknown,
              ),
              stage: event['stage'] as String?,
              transport: event['transport'] as String?,
              addressFamily: event['address_family'] as String?,
              durationMilliseconds: (event['duration_milliseconds'] as num?)
                  ?.toInt(),
              failure: failure is Map
                  ? TransportFailureInfo.fromMap(
                      Map<Object?, Object?>.from(failure),
                    )
                  : null,
            );
          })
          .toList(growable: false) ??
      const <ConnectionTimelineEvent>[];
  return ConnectionTimeline(
    events: List<ConnectionTimelineEvent>.unmodifiable(events),
    metrics: ConnectionMetrics(
      lastConnectDurationMilliseconds: optionalInt(
        'last_connect_duration_milliseconds',
      ),
      lastH3HandshakeDurationMilliseconds: optionalInt(
        'last_h3_handshake_duration_milliseconds',
      ),
      lastH2HandshakeDurationMilliseconds: optionalInt(
        'last_h2_handshake_duration_milliseconds',
      ),
      currentSmoothedRttMilliseconds:
          metrics['current_smoothed_rtt_known'] == true
          ? (metrics['current_smoothed_rtt_milliseconds'] as num?)?.toInt()
          : null,
      reconnectCount: (metrics['reconnect_count'] as num?)?.toInt() ?? 0,
      fallbackCount: (metrics['fallback_count'] as num?)?.toInt() ?? 0,
      networkChangeCount:
          (metrics['network_change_count'] as num?)?.toInt() ?? 0,
      sendQueueHighWatermark:
          (metrics['send_queue_high_watermark'] as num?)?.toInt() ?? 0,
      sendQueueDropCount:
          (metrics['send_queue_drop_count'] as num?)?.toInt() ?? 0,
      lastFailureCode: metrics['last_failure_code'] as String?,
      lastReconnectCode: metrics['last_reconnect_code'] as String?,
    ),
    droppedEventCount: (map['dropped_event_count'] as num?)?.toInt() ?? 0,
  );
}

T _enumByName<T extends Enum>(List<T> values, String? name, T fallback) {
  if (name == null) {
    return fallback;
  }
  for (final value in values) {
    if (value.name == name || value.name == _snakeToCamel(name)) {
      return value;
    }
  }
  return fallback;
}

String _snakeToCamel(String value) {
  final parts = value.toLowerCase().split('_');
  if (parts.isEmpty) {
    return value;
  }
  return parts.first +
      parts
          .skip(1)
          .map(
            (part) => part.isEmpty
                ? ''
                : '${part[0].toUpperCase()}${part.substring(1)}',
          )
          .join();
}

DateTime? _dateFromMilliseconds(Object? value) {
  final milliseconds = (value as num?)?.toInt() ?? 0;
  return milliseconds <= 0
      ? null
      : DateTime.fromMillisecondsSinceEpoch(milliseconds, isUtc: true);
}
