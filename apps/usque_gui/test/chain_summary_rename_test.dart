import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/chain_proxy_screen.dart';
import 'package:usque/state/app_controller.dart';

import 'chain_proxy_test.dart' show ChainEngine, imported;
import 'ui_workflow_test.dart' show workflowHost;

class _RenamingEngine extends ChainEngine {
  @override
  Future<ChainProfileResult> chainProfile(Map<String, Object?> request) async {
    final result = await super.chainProfile(request);
    if (request['action'] != 'rename') return result;
    final profile = library.single;
    expect(request['profile_id'], profile.id);
    expect(request['revision'], profile.editRevision);
    // The native store preserves the selected protocol revision on rename.
    library = [
      ChainProfileSummary(
        id: profile.id,
        revision: profile.revision,
        editRevision: 'renamed-edit',
        name: request['name']! as String,
        protocol: profile.protocol,
        source: profile.source,
        host: profile.host,
        port: profile.port,
      ),
    ];
    return ChainProfileResult(profiles: library);
  }
}

Future<AppController> _host(WidgetTester tester, _RenamingEngine engine) async {
  SharedPreferences.setMockInitialValues({'update_checks_enabled': false});
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = const Size(1280, 1100);
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
  engine
    ..library = [imported]
    ..legacyProfilesImported = true;
  engine.storedProfiles[0] = engine.storedProfiles[0].copyWith(
    chainExit: ChainExitSettings(
      enabled: true,
      source: imported.source,
      profileId: imported.id,
      revision: imported.revision,
    ),
  );
  final app = AppController(engine);
  await app.initialize();
  app.localePreference = LocalePreference.english;
  addTearDown(app.dispose);
  await tester.pumpWidget(workflowHost(app));
  await tester.pumpAndSettle();
  return app;
}

String _selectionText(WidgetTester tester, String key) => tester
    .widget<Text>(
      find.descendant(
        of: find.byKey(ValueKey(key)),
        matching: find.byType(Text),
      ),
    )
    .textSpan!
    .toPlainText();

void main() {
  testWidgets(
    'renaming a selected chain refreshes mounted Home and Proxy',
    (tester) async {
      final engine = _RenamingEngine();
      final app = await _host(tester, engine);
      expect(
        _selectionText(tester, 'home-chain-selection'),
        contains(imported.name),
      );
      app.selectSection(AppSection.proxy);
      await tester.pumpAndSettle();
      expect(
        _selectionText(tester, 'proxy-chain-selection'),
        contains(imported.name),
      );

      await tester.tap(find.byKey(const ValueKey('proxy-chain-proxy-entry')));
      await tester.pumpAndSettle();
      // Use the editor's real mutation path while both summary pages stay mounted.
      await app.chainProfile({
        'action': 'rename',
        'profile_id': imported.id,
        'revision': imported.editRevision,
        'name': 'Renamed office',
      });
      await tester.pumpAndSettle();
      expect(engine.actions, contains('rename'));
      expect(engine.library.single.name, 'Renamed office');
      expect(app.activeProfile.chainExit?.revision, imported.revision);
      Navigator.of(tester.element(find.byType(ChainProxyScreen))).pop();
      await tester.pumpAndSettle();
      expect(
        _selectionText(tester, 'proxy-chain-selection'),
        'WireGuard · Renamed office',
      );
      app.selectSection(AppSection.home);
      await tester.pumpAndSettle();
      expect(
        _selectionText(tester, 'home-chain-selection'),
        'WireGuard · Renamed office',
      );

      final reads = engine.actions.where((action) => action == 'list').length;
      for (var index = 0; index < 3; index++) {
        await app.refreshSnapshot();
        app.selectSection(AppSection.proxy);
        await tester.pumpAndSettle();
        app.selectSection(AppSection.home);
        await tester.pumpAndSettle();
      }
      expect(
        engine.actions.where((action) => action == 'list'),
        hasLength(reads),
      );
      expect(tester.takeException(), isNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'renaming a live selection refreshes its saved name after disconnect',
    (tester) async {
      final engine = _RenamingEngine()
        ..current = EngineSnapshot(
          phase: ConnectionPhase.connected,
          chainExit: ChainExitStatus(
            stage: 'connected',
            currentProfile: imported,
          ),
        );
      final app = await _host(tester, engine);
      expect(
        _selectionText(tester, 'home-chain-selection'),
        contains(imported.name),
      );
      await app.chainProfile({
        'action': 'rename',
        'profile_id': imported.id,
        'revision': imported.editRevision,
        'name': 'Renamed office',
      });
      await tester.pumpAndSettle();
      // Current-session metadata keeps priority while the session is running.
      expect(
        _selectionText(tester, 'home-chain-selection'),
        contains(imported.name),
      );
      engine.current = const EngineSnapshot();
      await app.refreshSnapshot();
      await tester.pumpAndSettle();
      expect(
        _selectionText(tester, 'home-chain-selection'),
        'WireGuard · Renamed office',
      );
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );
}
