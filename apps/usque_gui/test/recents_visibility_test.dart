import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/settings_screen.dart';
import 'package:usque/services/desktop_engine_client.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';

import 'ui_workflow_test.dart' show WorkflowEngine, pumpWorkflow, workflowHost;

class RecentsEngine extends WorkflowEngine {
  bool hidden = false;
  bool failRead = false;
  bool failWrite = false;
  int visibilityWrites = 0;
  int clears = 0;
  Completer<void>? pendingVisibilityWrite;

  @override
  Future<PlatformPreferences> platformPreferences() async {
    if (failRead) throw const FormatException('private path');
    return PlatformPreferences(hideFromRecents: hidden);
  }

  @override
  Future<void> setHideFromRecents(bool enabled) async {
    visibilityWrites++;
    await pendingVisibilityWrite?.future;
    if (failWrite) throw const FormatException('private path');
    hidden = enabled;
  }

  @override
  Future<void> clearAllData({required bool confirmed}) async {
    clears++;
    await super.clearAllData(confirmed: confirmed);
    hidden = false;
  }
}

const _row = ValueKey('settings-hide-recents');
const _retry = ValueKey('settings-recents-retry');

Future<AppController> _settings(
  WidgetTester tester,
  RecentsEngine engine, {
  bool refresh = true,
  double scale = 1,
  LocalePreference locale = LocalePreference.english,
}) async {
  final app = await pumpWorkflow(
    tester,
    engine,
    section: AppSection.settings,
    size: const Size(390, 844),
  );
  app.localePreference = locale;
  if (refresh) await app.refreshHideFromRecents();
  await tester.pumpWidget(
    workflowHost(
      app,
      scale: scale,
      home: ListenableBuilder(
        listenable: app,
        builder: (_, _) => SettingsScreen(controller: app),
      ),
    ),
  );
  await tester.pumpAndSettle();
  return app;
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(() => SharedPreferences.setMockInitialValues({}));

  test(
    'native preference default and channel payload are compatible',
    () async {
      expect(PlatformPreferences.fromMap({}).hideFromRecents, isFalse);
      const channel = MethodChannel('io.github.georgexie2333.usque/engine');
      final calls = <MethodCall>[];
      final messenger =
          TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
      messenger.setMockMethodCallHandler(channel, (call) async {
        calls.add(call);
        return call.method == 'platformPreferences'
            ? {'hide_from_recents': true}
            : null;
      });
      addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
      final engine = MethodChannelEngineClient();
      expect((await engine.platformPreferences()).hideFromRecents, isTrue);
      await engine.setHideFromRecents(false);
      expect(calls.map((call) => call.method), [
        'platformPreferences',
        'setHideFromRecents',
      ]);
      expect(calls.last.arguments, {'enabled': false});
      calls.clear();
      await expectLater(
        DesktopEngineClient().setHideFromRecents(true),
        throwsUnsupportedError,
      );
      expect(calls, isEmpty);
    },
  );

  for (final platform in [TargetPlatform.windows, TargetPlatform.linux]) {
    testWidgets(
      'desktop does not offer recent task hiding: ${platform.name}',
      (tester) async {
        final engine = RecentsEngine();
        await _settings(tester, engine);
        expect(find.byKey(_row), findsNothing);
        expect(engine.visibilityWrites, 0);
      },
      variant: TargetPlatformVariant.only(platform),
    );
  }

  testWidgets(
    'Android restores saved value and waits for confirmed serial writes',
    (tester) async {
      final engine = RecentsEngine()..hidden = true;
      final app = await _settings(tester, engine);
      final row = find.byKey(_row);
      expect(tester.widget<SwitchListTile>(row).value, isTrue);
      app.snapshot = const EngineSnapshot(phase: ConnectionPhase.connected);
      final connected = app.snapshot;
      engine.pendingVisibilityWrite = Completer<void>();
      await tester.ensureVisible(row);
      final operation = app.setHideFromRecents(false);
      await app.setHideFromRecents(false);
      await tester.pump();
      expect(engine.visibilityWrites, 1);
      expect(tester.widget<SwitchListTile>(row).value, isTrue);
      expect(tester.widget<SwitchListTile>(row).onChanged, isNull);
      engine.pendingVisibilityWrite!.complete();
      await operation;
      await tester.pumpAndSettle();
      expect(tester.widget<SwitchListTile>(row).value, isFalse);
      expect(tester.widget<SwitchListTile>(row).onChanged, isNotNull);
      expect(engine.writes, 0);
      expect(engine.calls, isEmpty);
      expect(identical(app.snapshot, connected), isTrue);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets(
    'unknown and failed read stay disabled until Retry confirms native state',
    (tester) async {
      final engine = RecentsEngine()..hidden = true;
      final app = await _settings(tester, engine, refresh: false);
      final row = find.byKey(_row);
      expect(tester.widget<SwitchListTile>(row).onChanged, isNull);
      expect(
        find.text(app.strings.get('hide_recents_loading')),
        findsOneWidget,
      );
      engine.failRead = true;
      await app.refreshHideFromRecents();
      await tester.pumpAndSettle();
      expect(app.hideFromRecentsKnown, isFalse);
      expect(tester.widget<SwitchListTile>(row).onChanged, isNull);
      expect(find.textContaining('private path'), findsNothing);
      engine.failRead = false;
      await tester.ensureVisible(find.byKey(_retry));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(_retry));
      await tester.pumpAndSettle();
      expect(app.hideFromRecentsKnown, isTrue);
      expect(tester.widget<SwitchListTile>(row).value, isTrue);
      expect(find.byKey(_retry), findsNothing);
      expect(app.lastError, isNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets(
    'failed write reads back native state and exposes a retry',
    (tester) async {
      final engine = RecentsEngine()..failWrite = true;
      final app = await _settings(tester, engine);
      await app.setHideFromRecents(true);
      await tester.pumpAndSettle();
      expect(app.hideFromRecents, isFalse);
      expect(app.hideFromRecentsKnown, isTrue);
      expect(app.recentsSettingsError, isTrue);
      expect(app.lastError, app.strings.get('hide_recents_failed'));
      engine.failRead = true;
      await app.setHideFromRecents(true);
      await tester.pumpAndSettle();
      expect(app.hideFromRecentsKnown, isFalse);
      expect(tester.widget<SwitchListTile>(find.byKey(_row)).onChanged, isNull);
      engine
        ..failRead = false
        ..failWrite = false;
      await app.refreshHideFromRecents();
      await app.setHideFromRecents(true);
      expect(app.hideFromRecents, isTrue);
      expect(app.recentsSettingsError, isFalse);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets(
    'clear waits for pending visibility write and cannot be overwritten by its reply',
    (tester) async {
      final engine = RecentsEngine()
        ..pendingVisibilityWrite = Completer<void>();
      final app = await _settings(tester, engine);
      final write = app.setHideFromRecents(true);
      final clear = app.clearAllData();
      await tester.pump();
      await app.setHideFromRecents(true);
      expect(engine.clears, 0);
      expect(engine.visibilityWrites, 1);
      engine.pendingVisibilityWrite!.complete();
      await write;
      expect(await clear, isTrue);
      expect(engine.clears, 1);
      expect(engine.hidden, isFalse);
      expect(app.hideFromRecents, isFalse);
      expect(app.hideFromRecentsKnown, isTrue);
      expect(app.hideFromRecentsBusy, isFalse);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets(
    'startup reads native preference without a separate Flutter default',
    (tester) async {
      final engine = RecentsEngine()..hidden = true;
      final app = AppController(engine);
      try {
        await app.initialize();
        expect(app.hideFromRecentsKnown, isTrue);
        expect(app.hideFromRecents, isTrue);
      } finally {
        app.dispose();
      }
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets(
    'failed clear reconciles a write completed during reset',
    (tester) async {
      final engine = RecentsEngine()
        ..clearAllDataError = const FormatException('private path')
        ..pendingVisibilityWrite = Completer<void>();
      final app = await _settings(tester, engine);
      final write = app.setHideFromRecents(true);
      final clear = app.clearAllData();
      await tester.pump();
      engine.pendingVisibilityWrite!.complete();
      await write;
      expect(await clear, isFalse);
      expect(engine.hidden, isTrue);
      expect(app.hideFromRecentsKnown, isTrue);
      expect(app.hideFromRecents, isTrue);
      expect(app.lastError, isNotNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  for (final locale in [LocalePreference.english, LocalePreference.persian]) {
    testWidgets(
      'recent task switch supports 200% text, semantics and D-pad: ${locale.name}',
      (tester) async {
        final semantics = tester.ensureSemantics();
        try {
          final engine = RecentsEngine();
          final app = await _settings(tester, engine, scale: 2, locale: locale);
          final row = find.byKey(_row);
          await tester.ensureVisible(row);
          await tester.pumpAndSettle();
          expect(
            tester.getSemantics(row).getSemanticsData().label,
            contains(app.strings.get('hide_from_recents')),
          );
          // SwitchListTile excludes its embedded Switch from focus. Traverse to
          // the tile's own focus target, as a remote/keyboard user would.
          for (var i = 0; i < 30; i++) {
            FocusScope.of(tester.element(row)).nextFocus();
            await tester.pumpAndSettle();
            if (FocusManager.instance.primaryFocus?.context
                    ?.findAncestorWidgetOfExactType<SwitchListTile>()
                    ?.key ==
                _row) {
              break;
            }
          }
          expect(
            FocusManager.instance.primaryFocus?.context
                ?.findAncestorWidgetOfExactType<SwitchListTile>()
                ?.key,
            _row,
          );
          await tester.sendKeyEvent(LogicalKeyboardKey.select);
          await tester.pumpAndSettle();
          expect(engine.visibilityWrites, 1);
          expect(app.hideFromRecents, isTrue);
          await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);
        } finally {
          semantics.dispose();
        }
      },
      variant: TargetPlatformVariant.only(TargetPlatform.android),
    );
  }
}
