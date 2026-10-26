import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/quality_presentation.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/widgets/common.dart';

void main() {
  final strings = AppStrings(LocalePreference.english);

  test('only a fresh connected sample is good, fair or poor', () {
    QualityLevelPresentation level({
      bool connected = true,
      bool stale = false,
      NetworkQualityLevel? value,
    }) => QualityLevelPresentation.of(
      connected: connected,
      stale: stale,
      level: value,
    );
    expect(level(connected: false).labelKey, 'nq_disconnected');
    expect(level(value: NetworkQualityLevel.good).labelKey, 'nq_good');
    expect(level(value: NetworkQualityLevel.good).tone, StatusTone.success);
    expect(level(value: NetworkQualityLevel.fair).tone, StatusTone.warning);
    expect(level(value: NetworkQualityLevel.poor).tone, StatusTone.danger);
    // A stale or missing reading is limited, never a reassuring "good".
    expect(
      level(stale: true, value: NetworkQualityLevel.good).labelKey,
      'nq_limited',
    );
    expect(level().labelKey, 'nq_limited');
    expect(level(value: NetworkQualityLevel.poor).needsDiagnosis, isTrue);
    expect(level(value: NetworkQualityLevel.fair).needsDiagnosis, isTrue);
    expect(level(value: NetworkQualityLevel.good).needsDiagnosis, isFalse);
    expect(level(connected: false).needsDiagnosis, isFalse);
  });

  test('readings format consistently', () {
    expect(formatLossBasisPoints(210), '2.10%');
    expect(formatLossBasisPoints(0), '0.00%');
    expect(formatRoundTrip(strings, 44, MetricAvailability.available), '44 ms');
    expect(
      formatRoundTrip(strings, 44, MetricAvailability.stale),
      strings.get('nq_unavailable'),
    );
    expect(
      formatRoundTrip(strings, null, MetricAvailability.available),
      strings.get('nq_unavailable'),
    );
  });

  test('phases map to tones and never expose unknown codes', () {
    expect(qualityPhaseTone('degraded'), StatusTone.warning);
    expect(qualityPhaseTone('aborted'), StatusTone.warning);
    expect(qualityPhaseTone('stable'), StatusTone.success);
    expect(qualityPhaseTone('probing'), StatusTone.neutral);
    expect(
      qualityPhaseLabel(strings, 'engine_private_state'),
      strings.get('nq_phase_unknown'),
    );
    expect(
      qualityMigrationReasonLabel(strings, 'engine_private_reason'),
      strings.get('nq_reason_unknown'),
    );
    expect(directDnsPhase('system').tone, StatusTone.success);
    expect(directDnsPhase('degraded').labelKey, 'nq_degraded');
    expect(directDnsPhase('anything').labelKey, 'nq_not_ready');
  });

  test('attention counts degraded folded phases only while connected', () {
    expect(
      qualityAttentionCount(
        connected: true,
        pmtuPhase: 'degraded',
        migrationPhase: 'aborted',
        directDnsPhase: 'degraded',
      ),
      3,
    );
    expect(
      qualityAttentionCount(
        connected: true,
        pmtuPhase: 'stable',
        migrationPhase: 'idle',
        directDnsPhase: 'ready',
      ),
      0,
    );
    expect(
      qualityAttentionCount(
        connected: false,
        pmtuPhase: 'degraded',
        migrationPhase: 'degraded',
        directDnsPhase: 'degraded',
      ),
      0,
    );
  });
}
