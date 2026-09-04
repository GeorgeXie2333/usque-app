import 'dart:async';

import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/network_quality_controller.dart';

import 'app_test.dart' show FakeEngineClient;

class QualityEngineStub extends FakeEngineClient {
  EngineCapabilities? capabilities = const EngineCapabilities(
    networkQuality: true,
    encryptedDirectDns: true,
    automaticPmtu: true,
    quicMigration: true,
  );
  Completer<NetworkQualitySnapshot?>? pendingQuality;
  int qualityRequests = 0;
  final modes = <DiagnosticMode>[];
  @override
  Future<EngineCapabilities?> getCapabilities() async => capabilities;
  @override
  Future<NetworkQualitySnapshot?> getNetworkQuality() {
    qualityRequests++;
    return pendingQuality?.future ?? Future.value(current.networkQuality);
  }

  @override
  Future<DiagnosticSession> startDiagnostics(DiagnosticMode mode) {
    modes.add(mode);
    return super.startDiagnostics(mode);
  }
}

NetworkQualitySnapshot qualityFixture(
  DateTime at, {
  String id = 'sample-connection',
  String state = 'h3',
  int rtt = 42,
}) {
  final h2 = state == 'h2';
  final poor = state == 'degraded';
  return NetworkQualitySnapshot(
    connectionInstanceId: id,
    sampledAt: at,
    level: state.endsWith('degraded')
        ? NetworkQualityLevel.poor
        : state == 'migration'
        ? NetworkQualityLevel.fair
        : NetworkQualityLevel.good,
    metrics: NetworkConnectionMetrics(
      latestRttMilliseconds: rtt,
      latestRttAvailability: MetricAvailability.available,
      smoothedRttMilliseconds: rtt + 4,
      smoothedRttAvailability: MetricAvailability.available,
      minimumRttMilliseconds: 18,
      minimumRttAvailability: MetricAvailability.available,
      intervalLossBasisPoints: h2
          ? null
          : poor
          ? 210
          : 10,
      intervalLossAvailability: h2
          ? MetricAvailability.unsupported
          : MetricAvailability.available,
      congestionWindowBytes: h2 ? null : 262144,
      congestionWindowAvailability: h2
          ? MetricAvailability.unsupported
          : MetricAvailability.available,
      sendRateBitsPerSecond: h2 ? null : 8 * 1024 * 1024,
      sendRateAvailability: h2
          ? MetricAvailability.unsupported
          : MetricAvailability.available,
      bytesInFlight: h2 ? null : 32768,
      bytesInFlightAvailability: h2
          ? MetricAvailability.unsupported
          : MetricAvailability.available,
      h2StreamReceiveWindowBytes: h2 ? 1048576 : 0,
      h2ConnectionReceiveWindowBytes: h2 ? 2097152 : 0,
      h2FlowControlStallCount: 2,
    ),
    queues: <NetworkQueueQuality>[
      NetworkQueueQuality(
        kind: NetworkQueueKind.tunToTransport,
        availability: MetricAvailability.available,
        currentItems: poor ? 90 : 3,
        capacityItems: 100,
        currentBytes: 4096,
        capacityBytes: 65536,
        highWaterItems: 90,
        highWaterBytes: 16384,
        dropItems: poor ? 2 : 0,
        oldestAgeMilliseconds: 6,
      ),
      const NetworkQueueQuality(
        kind: NetworkQueueKind.h3WireSend,
        availability: MetricAvailability.available,
        currentItems: 0,
        capacityItems: 64,
        currentBytes: 0,
        capacityBytes: 65536,
      ),
    ],
    pmtu: PmtuQualityInfo(
      availability: h2
          ? MetricAvailability.unsupported
          : MetricAvailability.available,
      effectivePayloadAvailability: h2
          ? MetricAvailability.unsupported
          : MetricAvailability.available,
      outerPmtuBytes: h2 ? null : 1350,
      effectiveConnectIpPayloadBytes: h2 ? null : 1280,
      phaseCode: h2
          ? 'unsupported'
          : state == 'degraded' || state == 'pmtu_degraded'
          ? 'degraded'
          : 'stable',
    ),
    migration: MigrationQualityInfo(
      phaseCode: state == 'migration' ? 'probing' : 'idle',
      attemptCount: 2,
      successCount: 1,
      failureCount: 1,
      lastDurationMilliseconds: 240,
      lastReasonCode: poor ? 'path_validation_timeout' : '',
    ),
    directDns: DirectDnsQualityInfo(
      mode: DirectDnsMode.doh,
      phaseCode: poor || state == 'dns_degraded' ? 'degraded' : 'ready',
      successCount: 82,
      failureCount: poor ? 3 : 0,
      timeoutCount: poor ? 1 : 0,
      lastRttMilliseconds: 29,
    ),
  );
}

AppController qualityApp(
  QualityEngineStub engine, {
  String state = 'h3',
  LocalePreference locale = LocalePreference.english,
}) {
  var now = DateTime.utc(2026, 9, 2, 12);
  final quality = NetworkQualityController(
    engine,
    now: () => now,
    autoTick: false,
  );
  final app = AppController(engine, qualityController: quality)
    ..localePreference = locale
    ..engineCapabilities = engine.capabilities;
  if (state == 'disconnected') return app;
  for (var second = 0; second < 60; second++) {
    now = DateTime.utc(2026, 9, 2, 12).add(Duration(seconds: second));
    app.snapshot = EngineSnapshot(
      phase: ConnectionPhase.connected,
      transport: state == 'h2' ? 'HTTP/2' : 'HTTP/3',
      addressFamily: 'IPv4',
      downloadedBytes: second * 750000,
      uploadedBytes: second * 210000,
      networkQuality: qualityFixture(
        now,
        state: state,
        rtt: 32 + (second % 11) * 3,
      ),
    );
  }
  if (state == 'stale') quality.markStreamUnavailable(true);
  return app;
}
