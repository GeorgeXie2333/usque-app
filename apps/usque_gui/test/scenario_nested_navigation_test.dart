import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/chain_proxy_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/section_navigator.dart';
import 'package:usque/widgets/unsaved_changes_guard.dart';

import 'scenarios_test.dart' show SceneEngine;
import 'ui_workflow_test.dart' show pumpWorkflow;

class _NestedSceneEngine extends SceneEngine implements ChainProfileClient {
  @override
  Future<EngineCapabilities?> getCapabilities() async =>
      const EngineCapabilities(
        connectionScenarios: true,
        networkSettingsApplication: true,
        chainProfileImport: true,
        chainOpenvpnUdp: true,
        automaticEndpoints: true,
      );

  @override
  Future<ChainProfileResult> chainProfile(Map<String, Object?> request) async =>
      const ChainProfileResult(
        profiles: [
          ChainProfileSummary(
            id: 'new-exit',
            revision: 'r1',
            editRevision: 'e1',
            name: 'Fresh selection',
            source: ChainSource.openvpnCustom,
            protocol: 'openvpn_udp',
            host: 'example.invalid',
            port: 1194,
          ),
        ],
      );

  @override
  Future<List<ChainConfigurationFile>> pickChainConfigurations() async => [];
}

Future<AppController> _openDirtyChild(
  WidgetTester tester,
  _NestedSceneEngine engine,
) async {
  final app = await pumpWorkflow(tester, engine, section: AppSection.profiles);
  app.engineCapabilities = await engine.getCapabilities();
  await app.scenarios.refresh();
  await tester.pumpAndSettle();
  await tester.tap(find.byKey(const ValueKey('scenario-create')));
  await tester.pumpAndSettle();
  await tester.enterText(
    find.byKey(const ValueKey('scenario-name')),
    'Parent draft',
  );
  await tester.tap(find.byKey(const ValueKey('scenario-chain-toggle')));
  await tester.pumpAndSettle();
  await tester.tap(find.byKey(const ValueKey('scenario-chain-settings')));
  await tester.pumpAndSettle();
  await tester.tap(find.byKey(const ValueKey('chain-proxy-toggle')));
  await tester.pumpAndSettle();
  expect(
    tester
        .widget<SwitchListTile>(
          find.byKey(const ValueKey('chain-proxy-toggle')),
        )
        .value,
    false,
  );
  return app;
}

Future<void> _answer(
  WidgetTester tester,
  AppStrings strings, {
  required bool discard,
  bool scene = false,
}) async {
  expect(
    find.text(
      strings.get(scene ? 'scenario_discard_title' : 'discard_changes_title'),
    ),
    findsOneWidget,
  );
  await tester.tap(
    find.text(strings.get(discard ? 'discard_changes' : 'keep_editing')),
  );
  await tester.pumpAndSettle();
}

void main() {
  for (final systemBack in [false, true]) {
    testWidgets(
      'cancelled scene departure restores child protection for ${systemBack ? 'system' : 'explicit'} back',
      (tester) async {
        final engine = _NestedSceneEngine();
        final app = await _openDirtyChild(tester, engine);
        final home = find.descendant(
          of: find.byType(NavigationRail),
          matching: find.text(app.strings.get('nav_home')),
        );
        await tester.tap(home);
        await tester.pumpAndSettle();
        // A concurrent request must not open another confirmation or commit
        // the in-progress section departure.
        final section = tester.state<SectionNavigatorState>(
          find.byWidgetPredicate(
            (widget) => widget is SectionNavigator && widget.active,
          ),
        );
        expect(await section.closeSubpages(), false);
        await _answer(tester, app.strings, scene: true, discard: true);
        await _answer(tester, app.strings, scene: true, discard: false);
        expect(find.byType(ChainProxyScreen), findsOneWidget);
        expect(app.section, AppSection.profiles);

        // This edit is newer than the cancelled departure's discard consent.
        await tester.ensureVisible(find.text('Fresh selection'));
        await tester.tap(find.text('Fresh selection'));
        await tester.pumpAndSettle();
        expect(
          tester
              .widget<RadioGroup<String>>(find.byType(RadioGroup<String>))
              .groupValue,
          'new-exit',
        );
        Future<void> back() async {
          if (systemBack) {
            await tester.binding.handlePopRoute();
          } else {
            await tester.ensureVisible(find.text(app.strings.get('back')));
            await tester.tap(find.text(app.strings.get('back')));
          }
          await tester.pumpAndSettle();
        }

        await back();
        await _answer(tester, app.strings, scene: true, discard: false);
        expect(find.byType(ChainProxyScreen), findsOneWidget);
        await back();
        await _answer(tester, app.strings, scene: true, discard: true);
        expect(find.byType(ChainProxyScreen), findsNothing);
        expect(
          tester
              .widget<TextFormField>(
                find.byKey(const ValueKey('scenario-name')),
              )
              .controller!
              .text,
          'Parent draft',
        );
        expect(
          engine.sceneCalls.where((request) => request['action'] == 'save'),
          isEmpty,
        );
      },
      variant: TargetPlatformVariant.only(TargetPlatform.windows),
    );
  }

  testWidgets(
    'saving that begins during another guard confirmation aborts and revokes the whole departure',
    (tester) async {
      final strings = AppStrings(LocalePreference.english);
      final section = GlobalKey<SectionNavigatorState>();
      final childSaving = ValueNotifier(false);
      addTearDown(childSaving.dispose);
      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: Scaffold(
            body: SectionNavigator(
              key: section,
              active: true,
              rootName: 'root',
              builder: (context) => TextButton(
                onPressed: () => Navigator.of(context).push<void>(
                  MaterialPageRoute(
                    builder: (context) => UnsavedChangesGuard(
                      strings: strings,
                      dirty: true,
                      saving: false,
                      child: Scaffold(
                        body: TextButton(
                          onPressed: () => Navigator.of(context).push<void>(
                            MaterialPageRoute(
                              builder: (_) => ValueListenableBuilder<bool>(
                                valueListenable: childSaving,
                                builder: (_, saving, _) => UnsavedChangesGuard(
                                  strings: strings,
                                  dirty: true,
                                  saving: saving,
                                  child: const Scaffold(
                                    body: Text('Child draft'),
                                  ),
                                ),
                              ),
                            ),
                          ),
                          child: const Text('Open child'),
                        ),
                      ),
                    ),
                  ),
                ),
                child: const Text('Open parent'),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Open parent'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Open child'));
      await tester.pumpAndSettle();
      final departure = section.currentState!.closeSubpages();
      await tester.pumpAndSettle();
      await _answer(tester, strings, discard: true);
      childSaving.value = true;
      await tester.pumpAndSettle();
      await _answer(tester, strings, discard: true);
      expect(await departure, false);
      expect(find.text('Child draft'), findsOneWidget);
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.text(strings.get('discard_changes_title')), findsNothing);
      expect(find.text('Child draft'), findsOneWidget);

      childSaving.value = false;
      await tester.pumpAndSettle();
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      await _answer(tester, strings, discard: false);
      expect(find.text('Child draft'), findsOneWidget);
      final retry = section.currentState!.closeSubpages();
      await tester.pumpAndSettle();
      await _answer(tester, strings, discard: true);
      await _answer(tester, strings, discard: true);
      expect(await retry, true);
      expect(find.text('Open parent'), findsOneWidget);
    },
  );
}
