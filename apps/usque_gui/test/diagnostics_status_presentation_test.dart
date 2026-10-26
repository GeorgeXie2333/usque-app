import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/diagnostics_presentation.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/state/diagnostics_controller.dart';
import 'package:usque/widgets/common.dart';

import 'diagnostics_controller_test.dart'
    show DiagnosticsEngineStub, runningSession;

void main() {
  test('session states map to tones', () {
    expect(
      diagnosticSessionTone(DiagnosticSessionState.running),
      StatusTone.brand,
    );
    expect(
      diagnosticSessionTone(DiagnosticSessionState.completed),
      StatusTone.success,
    );
    expect(
      diagnosticSessionTone(DiagnosticSessionState.failed),
      StatusTone.danger,
    );
    expect(
      diagnosticSessionTone(DiagnosticSessionState.cancelled),
      StatusTone.warning,
    );
  });

  test('every category and check status has an icon', () {
    for (final category in DiagnosticCategory.values) {
      expect(diagnosticCategoryIcon(category), isNotNull);
    }
    for (final status in DiagnosticCheckStatus.values) {
      expect(diagnosticCheckStatusIcon(status), isNotNull);
    }
  });

  test('busy covers starting, running and stopping runs', () async {
    final engine = DiagnosticsEngineStub()
      ..pendingStart = Completer<DiagnosticSession>();
    final controller = DiagnosticsController(engine);
    addTearDown(controller.dispose);
    expect(controller.busy, isFalse);
    expect(controller.canCancel, isFalse);
    final starting = controller.start(DiagnosticMode.standard);
    expect(controller.state, DiagnosticsControllerState.starting);
    expect(controller.busy, isTrue);
    expect(controller.canCancel, isTrue);
    engine.pendingStart!.complete(runningSession());
    await starting;
    expect(controller.isActive, isTrue);
    expect(controller.busy, isTrue);
    expect(controller.canCancel, isTrue);
  });

  DiagnosticFinding finding(
    String id,
    DiagnosticCategory category,
    DiagnosticCheckStatus status, {
    String remediation = '',
    TransportFailureInfo? failure,
  }) => DiagnosticFinding(
    checkId: id,
    category: category,
    status: status,
    remediationKey: remediation,
    failure: failure,
  );

  test('problems lead the results, most severe first', () {
    final findings = [
      finding(
        'engine.control_channel',
        DiagnosticCategory.localComponent,
        DiagnosticCheckStatus.passed,
      ),
      finding(
        'physical.network_present',
        DiagnosticCategory.physicalNetwork,
        DiagnosticCheckStatus.skipped,
      ),
      finding(
        'quality.packet_loss',
        DiagnosticCategory.tunnel,
        DiagnosticCheckStatus.warning,
      ),
      finding(
        'transport.h2_connect',
        DiagnosticCategory.transport,
        DiagnosticCheckStatus.passed,
      ),
      finding(
        'transport.h3_connect',
        DiagnosticCategory.transport,
        DiagnosticCheckStatus.failed,
      ),
    ];
    final groups = groupDiagnosticFindings(findings);
    expect(groups.map((group) => group.category), [
      DiagnosticCategory.transport,
      DiagnosticCategory.tunnel,
      DiagnosticCategory.localComponent,
      DiagnosticCategory.physicalNetwork,
    ]);
    // Inside a category the failure comes before the passing check.
    expect(groups.first.findings.map((finding) => finding.checkId), [
      'transport.h3_connect',
      'transport.h2_connect',
    ]);
    expect(groups.map((group) => group.settled), [false, false, true, true]);
    expect(diagnosticProblems(findings).map((finding) => finding.checkId), [
      'transport.h3_connect',
      'quality.packet_loss',
    ]);
    expect(diagnosticProblems(findings.take(2).toList()), isEmpty);
  });

  test('remediation prefers the finding, then its failure', () {
    const failure = TransportFailureInfo(
      code: 'H3_UDP_UNREACHABLE',
      stage: 'quic_handshake',
      remediationKey: 'try_http2',
    );
    expect(
      diagnosticRemediationKey(
        finding(
          'transport.h3_connect',
          DiagnosticCategory.transport,
          DiagnosticCheckStatus.failed,
          remediation: 'check_physical_network',
          failure: failure,
        ),
      ),
      'check_physical_network',
    );
    expect(
      diagnosticRemediationKey(
        finding(
          'transport.h3_connect',
          DiagnosticCategory.transport,
          DiagnosticCheckStatus.failed,
          remediation: 'none',
          failure: failure,
        ),
      ),
      'try_http2',
    );
  });
}
