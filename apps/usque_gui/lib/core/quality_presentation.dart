import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../models/app_models.dart';
import '../models/network_quality_models.dart';
import '../widgets/common.dart';
import 'app_strings.dart';

/// Overall connection quality as Network quality presents it: a catalog key,
/// its tone and icon. Only a live, fresh sample can be good, fair or poor.
class QualityLevelPresentation {
  const QualityLevelPresentation(this.labelKey, this.tone, this.icon);
  final String labelKey;
  final StatusTone tone;
  final IconData icon;

  /// Whether the level asks the user to investigate.
  bool get needsDiagnosis => labelKey == 'nq_fair' || labelKey == 'nq_poor';

  static QualityLevelPresentation of({
    required bool connected,
    required bool stale,
    NetworkQualityLevel? level,
  }) {
    final key = !connected
        ? 'disconnected'
        : stale
        ? 'limited'
        : switch (level) {
            NetworkQualityLevel.good => 'good',
            NetworkQualityLevel.fair => 'fair',
            NetworkQualityLevel.poor => 'poor',
            _ => 'limited',
          };
    return QualityLevelPresentation('nq_$key', switch (key) {
      'good' => StatusTone.success,
      'fair' => StatusTone.warning,
      'poor' => StatusTone.danger,
      _ => StatusTone.neutral,
    }, key == 'good' ? LucideIcons.circleCheck : LucideIcons.activity);
  }
}

/// Interval loss, reported in basis points, as a two-decimal percentage.
String formatLossBasisPoints(int basisPoints) =>
    '${(basisPoints / 100).toStringAsFixed(2)}%';

/// A round-trip reading, or the unavailable label when the engine has none.
String formatRoundTrip(
  AppStrings strings,
  int? milliseconds,
  MetricAvailability availability,
) => availableMetric(milliseconds, availability) == null
    ? strings.get('nq_unavailable')
    : '$milliseconds ms';

/// Path MTU and migration share one phase vocabulary.
const _qualityPhases = <String>{
  'idle',
  'preparing_socket',
  'probing',
  'validated',
  'promoting',
  'stable',
  'aborted',
  'revalidating',
  'degraded',
  'unsupported',
};

StatusTone qualityPhaseTone(String phase) => switch (phase) {
  'degraded' || 'aborted' => StatusTone.warning,
  'validated' || 'stable' => StatusTone.success,
  _ => StatusTone.neutral,
};

String qualityPhaseLabel(AppStrings strings, String phase) => strings.get(
  'nq_phase_${_qualityPhases.contains(phase) ? phase : 'unknown'}',
);

const _migrationReasons = <String>{
  'family_unavailable',
  'socket_protect_failed',
  'generation_changed_during_setup',
  'peer_cid_unavailable',
  'local_cid_unavailable',
  'path_probe_rejected',
  'path_validation_timeout',
  'superseded',
  'promotion_failed',
  'connection_closed',
  'unsupported',
};

String qualityMigrationReasonLabel(AppStrings strings, String reason) =>
    strings.get(
      'nq_reason_${_migrationReasons.contains(reason) ? reason : 'unknown'}',
    );

/// Direct DNS phases use their own catalog keys and tones.
({String labelKey, StatusTone tone}) directDnsPhase(String phase) =>
    switch (phase) {
      'ready' || 'system' => (labelKey: 'nq_ready', tone: StatusTone.success),
      'degraded' => (labelKey: 'nq_degraded', tone: StatusTone.warning),
      'connecting' => (labelKey: 'nq_connecting', tone: StatusTone.neutral),
      _ => (labelKey: 'nq_not_ready', tone: StatusTone.neutral),
    };

/// Folded transport phases that need attention. Counted on the Advanced
/// metrics heading, so a collapsed group never hides a degraded path.
int qualityAttentionCount({
  required bool connected,
  required String pmtuPhase,
  required String migrationPhase,
  required String directDnsPhase,
}) {
  if (!connected) return 0;
  return <bool>[
    qualityPhaseTone(pmtuPhase) == StatusTone.warning,
    qualityPhaseTone(migrationPhase) == StatusTone.warning,
    directDnsPhase == 'degraded',
  ].where((value) => value).length;
}
