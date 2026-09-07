import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/state/app_controller.dart';

import 'app_test.dart' show FakeEngineClient;
import 'ui_workflow_test.dart' show workflowHost;

const codec = ControlCodec();

class CongestionSaveEngine extends FakeEngineClient {
  int reconfigurations = 0;
  final runtimeReconfigure = Completer<void>();

  @override
  Future<void> reconfigureActiveProfile(UsqueProfile profile) async {
    reconfigurations++;
    await runtimeReconfigure.future;
    await super.reconfigureActiveProfile(profile);
  }
}

Uint8List response(int field, Uint8List payload) => codec.frame(
  (ControlPayloadWriter()
        ..string(1, 'cc')
        ..message(field, payload))
      .takeBytes(),
);

UsqueProfile decodeProfile(Uint8List payload) => codec
    .requireProfileCatalog(
      codec.decodeResponse(
        response(
          12,
          (ControlPayloadWriter()
                ..message(1, payload)
                ..string(2, UsqueProfile.defaultProfileId))
              .takeBytes(),
        ),
        'cc',
      ),
    )
    .profiles
    .single;

void main() {
  test('four exact values round-trip JSON and appended protobuf field 18', () {
    for (final algorithm in CongestionControlAlgorithm.values) {
      final profile = UsqueProfile.defaultProfile().copyWith(
        congestionControl: algorithm,
      );
      expect(
        UsqueProfile.fromMap(profile.toMap()).congestionControl,
        algorithm,
      );
      final encoded = codec.encodeProfile(profile);
      expect(encoded.sublist(encoded.length - 3), [
        0x90,
        0x01,
        algorithm.index + 1,
      ]);
      expect(decodeProfile(encoded).congestionControl, algorithm);
      expect(
        profile.resetAdvancedDefaults().congestionControl,
        CongestionControlAlgorithm.cubic,
      );
    }
    final legacy = UsqueProfile.defaultProfile().toMap()
      ..remove('congestion_control');
    expect(
      UsqueProfile.fromMap(legacy).congestionControl,
      CongestionControlAlgorithm.cubic,
    );
    final encoded = codec.encodeProfile(UsqueProfile.defaultProfile());
    expect(
      decodeProfile(encoded.sublist(0, encoded.length - 3)).congestionControl,
      CongestionControlAlgorithm.cubic,
    );
    for (final value in ['bbr2', 'BBR3', 'unknown', null, 4]) {
      expect(
        () => UsqueProfile.fromMap({...legacy, 'congestion_control': value}),
        throwsFormatException,
      );
    }
  });

  test('capabilities decode packed, unpacked, empty and old engines', () {
    final packed =
        (ControlPayloadWriter()
              ..message(24, Uint8List.fromList([1, 2, 3, 4, 99])))
            .takeBytes();
    final unpacked =
        (ControlPayloadWriter()
              ..enumeration(24, 1)
              ..enumeration(24, 2)
              ..enumeration(24, 3)
              ..enumeration(24, 4))
            .takeBytes();
    for (final payload in [packed, unpacked]) {
      expect(
        codec
            .decodeResponse(response(15, payload), 'cc')
            .capabilities!
            .h3CongestionControlAlgorithms,
        CongestionControlAlgorithm.values,
      );
    }
    for (final payload in [
      Uint8List(0),
      (ControlPayloadWriter()..message(24, Uint8List(0))).takeBytes(),
    ]) {
      expect(
        codec
            .decodeResponse(response(15, payload), 'cc')
            .capabilities!
            .h3CongestionControlAlgorithms,
        isEmpty,
      );
    }
  });

  test(
    'session selection is independent from saved values and unknown stays unknown',
    () {
      for (final algorithm in CongestionControlAlgorithm.values) {
        final payload =
            (ControlPayloadWriter()
                  ..enumeration(1, 5)
                  ..enumeration(18, algorithm.index + 1))
                .takeBytes();
        expect(
          codec
              .decodeResponse(response(11, payload), 'cc')
              .snapshot!
              .sessionCongestionControl,
          algorithm,
        );
        expect(
          EngineSnapshot.fromMap({
            'phase': 'connected',
            'session_congestion_control': algorithm.name,
          }).sessionCongestionControl,
          algorithm,
        );
      }
      expect(
        const EngineSnapshot(
          sessionCongestionControl: CongestionControlAlgorithm.cubic,
        ),
        isNot(
          const EngineSnapshot(
            sessionCongestionControl: CongestionControlAlgorithm.bbr3,
          ),
        ),
      );
      expect(
        codec
            .decodeResponse(response(11, Uint8List(0)), 'cc')
            .snapshot!
            .sessionCongestionControl,
        isNull,
      );
      expect(
        EngineSnapshot.fromMap({
          'phase': 'connected',
          'session_congestion_control': 'future',
        }).sessionCongestionControl,
        isNull,
      );
    },
  );

  Future<AppController> screen(
    WidgetTester tester,
    FakeEngineClient engine, {
    bool supported = true,
    bool h2 = false,
    bool chinese = false,
    ConnectionPhase phase = ConnectionPhase.connected,
    bool pushed = false,
    double scale = 1,
    Size size = const Size(1280, 900),
  }) async {
    tester.view.physicalSize = size;
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final app = AppController(engine)
      ..localePreference = chinese
          ? LocalePreference.simplifiedChinese
          : LocalePreference.english
      ..engineCapabilities = EngineCapabilities(
        h3CongestionControlAlgorithms: supported
            ? CongestionControlAlgorithm.values
            : const [],
      )
      ..snapshot = EngineSnapshot(
        phase: phase,
        transport: h2 ? 'h2' : 'h3',
        sessionCongestionControl: CongestionControlAlgorithm.cubic,
      );
    if (h2) {
      app.sharedNetwork = app.sharedNetwork.copyWith(
        transport: TransportPolicy.http2,
      );
    }
    addTearDown(app.dispose);
    await tester.pumpWidget(
      workflowHost(
        app,
        dark: chinese,
        scale: scale,
        home: pushed
            ? Builder(
                builder: (context) => Scaffold(
                  body: TextButton(
                    onPressed: () => Navigator.of(context).push<void>(
                      MaterialPageRoute(
                        builder: (_) => AdvancedSettingsScreen(controller: app),
                      ),
                    ),
                    child: const Text('Open advanced'),
                  ),
                ),
              )
            : AdvancedSettingsScreen(controller: app),
      ),
    );
    await tester.pumpAndSettle();
    if (pushed) {
      await tester.tap(find.text('Open advanced'));
      await tester.pumpAndSettle();
    }
    return app;
  }

  for (final phase in [
    ConnectionPhase.connectingH3,
    ConnectionPhase.connected,
    ConnectionPhase.reconnecting,
  ]) {
    testWidgets(
      'algorithm-only save and back do not wait for runtime in $phase',
      (tester) async {
        final engine = CongestionSaveEngine();
        final app = await screen(tester, engine, phase: phase, pushed: true);
        addTearDown(() {
          if (!engine.runtimeReconfigure.isCompleted) {
            engine.runtimeReconfigure.complete();
          }
        });
        await tester.tap(find.byKey(const ValueKey('congestion-control')));
        await tester.pumpAndSettle();
        await tester.tap(find.text('BBRv3').last);
        await tester.pumpAndSettle();
        await tester.tap(find.widgetWithText(FilledButton, 'Apply changes'));
        await tester.pump();
        expect(engine.reconfigurations, 0);
        await tester.pumpAndSettle();
        expect(
          app.sharedNetwork.congestionControl,
          CongestionControlAlgorithm.bbr3,
        );
        expect(app.snapshot.phase, phase);
        expect(
          app.snapshot.sessionCongestionControl,
          CongestionControlAlgorithm.cubic,
        );
        await tester.binding.handlePopRoute();
        await tester.pumpAndSettle();
        expect(find.byType(AdvancedSettingsScreen), findsNothing);
        expect(find.text('Open advanced'), findsOneWidget);
      },
      variant: TargetPlatformVariant.only(TargetPlatform.android),
    );
  }

  testWidgets(
    'connecting draft can be discarded with Android back',
    (tester) async {
      final engine = CongestionSaveEngine();
      final app = await screen(
        tester,
        engine,
        phase: ConnectionPhase.connectingH3,
        pushed: true,
        size: const Size(375, 812),
      );
      final selector = find.byKey(const ValueKey('congestion-control'));
      await tester.ensureVisible(selector);
      await tester.pumpAndSettle();
      await tester.tap(selector);
      await tester.pumpAndSettle();
      await tester.tap(find.text('BBRv3').last);
      await tester.pumpAndSettle();
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      await tester.tap(find.text('Discard changes'));
      await tester.pumpAndSettle();
      expect(find.byType(AdvancedSettingsScreen), findsNothing);
      expect(
        app.sharedNetwork.congestionControl,
        CongestionControlAlgorithm.cubic,
      );
      expect(engine.reconfigurations, 0);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  test('mixed algorithm and runtime edits still reconfigure', () async {
    final engine = CongestionSaveEngine();
    final app = AppController(engine)
      ..snapshot = const EngineSnapshot(
        phase: ConnectionPhase.connected,
        sessionCongestionControl: CongestionControlAlgorithm.cubic,
      );
    addTearDown(app.dispose);
    engine.runtimeReconfigure.complete();
    expect(
      await app.saveNetwork(
        app.activeProfile.copyWith(
          congestionControl: CongestionControlAlgorithm.bbr3,
          sni: 'changed.example',
        ),
      ),
      isTrue,
    );
    expect(engine.reconfigurations, 1);
    expect(
      app.snapshot.sessionCongestionControl,
      CongestionControlAlgorithm.cubic,
    );
  });

  for (final size in [const Size(375, 812), const Size(1280, 900)]) {
    testWidgets('selector is below SNI with concise labels at $size', (
      tester,
    ) async {
      final app = await screen(tester, FakeEngineClient(), size: size);
      final selector = find.byKey(const ValueKey('congestion-control'));
      final sni = find.byWidgetPredicate(
        (widget) =>
            widget is TextField &&
            widget.decoration?.labelText == app.strings.get('sni'),
      );
      expect(
        tester.getTopLeft(selector).dy,
        greaterThan(tester.getBottomLeft(sni).dy),
      );
      final field = tester.widget<DropdownButton<CongestionControlAlgorithm>>(
        find.descendant(
          of: selector,
          matching: find.byType(DropdownButton<CongestionControlAlgorithm>),
        ),
      );
      expect(field.items!.map((item) => (item.child as Text).data), [
        'cubic',
        'BBRv2',
        'BBRv3',
        'reno',
      ]);
      expect(find.textContaining('bbr uses'), findsNothing);
      expect(tester.takeException(), isNull);
    });
  }

  for (final platform in [TargetPlatform.windows, TargetPlatform.android]) {
    testWidgets(
      'selector supports keyboard activation on $platform',
      (tester) async {
        await screen(tester, FakeEngineClient());
        final selector = find.byKey(const ValueKey('congestion-control'));
        await tester.ensureVisible(selector);
        await tester.pumpAndSettle();
        final focus = tester
            .widgetList<Focus>(
              find.descendant(of: selector, matching: find.byType(Focus)),
            )
            .map((widget) => widget.focusNode)
            .whereType<FocusNode>()
            .first;
        focus.requestFocus();
        await tester.pump();
        await tester.sendKeyEvent(LogicalKeyboardKey.enter);
        await tester.pumpAndSettle();
        await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
        await tester.sendKeyEvent(LogicalKeyboardKey.enter);
        await tester.pumpAndSettle();
        expect(
          tester
              .widget<DropdownButtonFormField<CongestionControlAlgorithm>>(
                selector,
              )
              .initialValue,
          CongestionControlAlgorithm.bbr,
        );
        expect(tester.getSize(selector).height, greaterThanOrEqualTo(48));
        expect(tester.takeException(), isNull);
      },
      variant: TargetPlatformVariant.only(platform),
    );
  }

  testWidgets(
    'saving bbr3 leaves current session unchanged and clears only the draft',
    (tester) async {
      final engine = FakeEngineClient();
      final app = await screen(tester, engine);
      final dropdown = find.byKey(const ValueKey('congestion-control'));
      await tester.tap(dropdown);
      await tester.pumpAndSettle();
      await tester.tap(find.text('BBRv3').last);
      await tester.pumpAndSettle();
      expect(
        app.sharedNetwork.congestionControl,
        CongestionControlAlgorithm.cubic,
      );
      await tester.tap(find.widgetWithText(FilledButton, 'Apply changes'));
      await tester.pumpAndSettle();
      expect(
        app.sharedNetwork.congestionControl,
        CongestionControlAlgorithm.bbr3,
      );
      expect(
        app.snapshot.sessionCongestionControl,
        CongestionControlAlgorithm.cubic,
      );
      expect(engine.lastConnectedProfile, isNull);
      expect(find.textContaining('Current:'), findsNothing);
      expect(find.text('Changes applied'), findsNothing);
      expect(
        find.byKey(const ValueKey('congestion-control-pending')),
        findsOneWidget,
      );
      expect(find.text('Unapplied changes'), findsNothing);
      await tester.tap(dropdown);
      await tester.pumpAndSettle();
      await tester.tap(find.text('cubic').last);
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(FilledButton, 'Apply changes'));
      await tester.pumpAndSettle();
      expect(
        find.byKey(const ValueKey('congestion-control-pending')),
        findsNothing,
      );
    },
  );

  testWidgets('failed save keeps selection as an unsaved draft', (
    tester,
  ) async {
    final engine = FakeEngineClient()..failProfileUpsert = true;
    final app = await screen(tester, engine);
    await tester.tap(find.byKey(const ValueKey('congestion-control')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('BBRv3').last);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Apply changes'));
    await tester.pumpAndSettle();
    expect(
      app.sharedNetwork.congestionControl,
      CongestionControlAlgorithm.cubic,
    );
    expect(find.text(app.strings.get('changes_failed')), findsOneWidget);
    expect(
      tester.widget<PopScope<Object?>>(find.byType(PopScope<Object?>)).canPop,
      isFalse,
    );
    expect(
      tester
          .widget<DropdownButtonFormField<CongestionControlAlgorithm>>(
            find.byKey(const ValueKey('congestion-control')),
          )
          .initialValue,
      CongestionControlAlgorithm.bbr3,
    );
  });

  testWidgets('H2 and old engines disable selection with explanatory text', (
    tester,
  ) async {
    final app = await screen(tester, FakeEngineClient(), h2: true);
    final finder = find.byKey(const ValueKey('congestion-control'));
    expect(
      tester
          .widget<DropdownButtonFormField<CongestionControlAlgorithm>>(finder)
          .onChanged,
      isNull,
    );
    expect(find.text(app.strings.get('cc_h2')), findsOneWidget);
    app.engineCapabilities = const EngineCapabilities();
    await tester.pumpWidget(
      workflowHost(app, home: AdvancedSettingsScreen(controller: app)),
    );
    await tester.pumpAndSettle();
    expect(find.text(app.strings.get('cc_upgrade')), findsOneWidget);
  });

  testWidgets(
    'Chinese dark 200 percent keeps the selector and helper accessible',
    (tester) async {
      final app = await screen(
        tester,
        FakeEngineClient(),
        chinese: true,
        scale: 2,
      );
      await tester.ensureVisible(
        find.byKey(const ValueKey('congestion-control')),
      );
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('cc_label')), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );
}
