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
}
