import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/desktop_engine_transport.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/diagnostics_controller.dart';
import 'package:usque/widgets/external_link.dart';
import 'diagnostics_controller_test.dart' show DiagnosticsEngineStub;

class ExportFailureEngine extends DiagnosticsEngineStub {
  Object? failure;
  @override
  Future<String?> exportDiagnostics({String? diagnosticSessionId}) async {
    final error = failure;
    if (error != null) throw error;
    return null;
  }
}

void main() {
  testWidgets(
    'native destination errors use the EngineException contract; cancel remains null',
    (tester) async {
      const channel = MethodChannel('io.github.georgexie2333.usque/engine');
      final messenger =
          TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
      messenger.setMockMethodCallHandler(channel, (call) async {
        expect(call.method, 'selectDiagnosticsDestination');
        throw PlatformException(
          code: 'DIAGNOSTICS_DESTINATION_FAILED',
          message: 'private platform details',
        );
      });
      addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
      final transport = DesktopEngineTransport();
      addTearDown(transport.dispose);
      await expectLater(
        transport.selectDiagnosticsDestination(),
        throwsA(
          isA<EngineException>().having(
            (error) => error.code,
            'code',
            'DIAGNOSTICS_DESTINATION_FAILED',
          ),
        ),
      );
      messenger.setMockMethodCallHandler(channel, (_) async => null);
      expect(await transport.selectDiagnosticsDestination(), isNull);
    },
  );

  testWidgets(
    'every export failure leaves localized feedback and releases busy state',
    (tester) async {
      final engine = ExportFailureEngine();
      final controller = DiagnosticsController(engine);
      addTearDown(controller.dispose);
      for (final error in <Object>[
        PlatformException(
          code: 'DIAGNOSTICS_DESTINATION_FAILED',
          message: 'private path',
        ),
        const EngineException('DIAGNOSTICS_DESTINATION_FAILED', 'private path'),
        const FormatException('private path'),
      ]) {
        engine.failure = error;
        expect(await controller.export(), isNull);
        expect(controller.exporting, isFalse);
        expect(controller.lastError, isNotNull);
        expect(controller.lastError, isNot(contains('private')));
      }
      engine.failure = null;
      expect(await controller.export(), isNull);
      expect(controller.exporting, isFalse);
      expect(controller.lastError, isNull);
    },
  );

  for (final outcome in ['success', 'false', 'exception', 'disposed']) {
    testWidgets('external link feedback handles $outcome', (tester) async {
      final strings = AppStrings(LocalePreference.english);
      final delayed = Completer<bool>();
      Future<bool>? operation;
      Uri? requested;
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Builder(
              builder: (context) => TextButton(
                onPressed: () {
                  operation = openExternalLink(
                    context,
                    strings,
                    'https://example.invalid/release',
                    launcher: (uri) async {
                      requested = uri;
                      if (outcome == 'disposed') return delayed.future;
                      if (outcome == 'exception') {
                        throw PlatformException(
                          code: 'OPEN_FAILED',
                          message: 'private path',
                        );
                      }
                      return outcome == 'success';
                    },
                  );
                },
                child: const Text('Open'),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Open'));
      if (outcome == 'disposed') {
        await tester.pumpWidget(const SizedBox());
        delayed.complete(false);
      }
      expect(await operation, outcome == 'success');
      await tester.pumpAndSettle();
      expect(requested.toString(), 'https://example.invalid/release');
      expect(
        find.text(strings.get('zero_trust_browser_failed')),
        outcome == 'false' || outcome == 'exception'
            ? findsOneWidget
            : findsNothing,
      );
      expect(find.textContaining('private'), findsNothing);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    });
  }
}
