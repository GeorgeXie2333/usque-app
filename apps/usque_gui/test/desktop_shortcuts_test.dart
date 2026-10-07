import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/screens/diagnostics_screen.dart';
import 'package:usque/screens/settings_screen.dart';
import 'package:usque/widgets/desktop_shortcuts.dart';

import 'section_navigation_test.dart' show openSettingsRow;
import 'ui_workflow_test.dart'
    show WorkflowEngine, fieldWithLabel, pumpWorkflow;

final _windows = TargetPlatformVariant.only(TargetPlatform.windows);

Future<void> _chord(
  WidgetTester tester,
  LogicalKeyboardKey key, {
  LogicalKeyboardKey? modifier,
}) async {
  if (modifier != null) await tester.sendKeyDownEvent(modifier);
  await tester.sendKeyEvent(key);
  if (modifier != null) await tester.sendKeyUpEvent(modifier);
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('Ctrl+1 to Ctrl+4 select the shell sections', (tester) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.home,
    );
    const control = LogicalKeyboardKey.controlLeft;
    await _chord(tester, LogicalKeyboardKey.digit2, modifier: control);
    expect(app.section, AppSection.profiles);
    await _chord(tester, LogicalKeyboardKey.digit4, modifier: control);
    expect(app.section, AppSection.settings);
    await _chord(tester, LogicalKeyboardKey.digit3, modifier: control);
    expect(app.section, AppSection.proxy);
    await _chord(tester, LogicalKeyboardKey.digit1, modifier: control);
    expect(app.section, AppSection.home);

    await _chord(tester, LogicalKeyboardKey.digit2);
    expect(app.section, AppSection.home);
  }, variant: _windows);

  testWidgets('section shortcuts stay off on Android', (tester) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.home,
    );
    await _chord(
      tester,
      LogicalKeyboardKey.digit2,
      modifier: LogicalKeyboardKey.controlLeft,
    );
    expect(app.section, AppSection.home);
  });

  testWidgets('Escape and Alt+Left leave a subpage through its guard', (
    tester,
  ) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.settings,
    );
    await openSettingsRow(tester, app, 'diagnostics');
    expect(find.byType(DiagnosticsScreen), findsOneWidget);
    await _chord(tester, LogicalKeyboardKey.escape);
    expect(find.byType(DiagnosticsScreen), findsNothing);
    expect(find.byType(SettingsScreen), findsOneWidget);

    await openSettingsRow(tester, app, 'advanced');
    await tester.enterText(fieldWithLabel('SNI'), 'review.example');
    await tester.pumpAndSettle();
    // Escape belongs to the focused text field.
    await _chord(tester, LogicalKeyboardKey.escape);
    expect(find.byType(AdvancedSettingsScreen), findsOneWidget);
    expect(find.text(app.strings.get('discard_changes_title')), findsNothing);

    await _chord(
      tester,
      LogicalKeyboardKey.arrowLeft,
      modifier: LogicalKeyboardKey.altLeft,
    );
    expect(find.text(app.strings.get('discard_changes_title')), findsOneWidget);

    // The open dialog hides the shell from section shortcuts.
    await _chord(
      tester,
      LogicalKeyboardKey.digit1,
      modifier: LogicalKeyboardKey.controlLeft,
    );
    expect(app.section, AppSection.settings);

    await tester.tap(find.text(app.strings.get('keep_editing')));
    await tester.pumpAndSettle();
    expect(find.byType(AdvancedSettingsScreen), findsOneWidget);
    expect(
      tester.widget<TextField>(fieldWithLabel('SNI')).controller!.text,
      'review.example',
    );
  }, variant: _windows);

  testWidgets('the mouse back button leaves a subpage', (tester) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.settings,
    );
    await openSettingsRow(tester, app, 'diagnostics');
    final gesture = await tester.createGesture(
      kind: PointerDeviceKind.mouse,
      buttons: kBackMouseButton,
    );
    await gesture.down(tester.getCenter(find.byType(DiagnosticsScreen)));
    await gesture.up();
    await tester.pumpAndSettle();
    expect(find.byType(DiagnosticsScreen), findsNothing);
    expect(find.byType(SettingsScreen), findsOneWidget);
  }, variant: _windows);

  testWidgets('page shortcuts fire only on the visible top route', (
    tester,
  ) async {
    var refreshes = 0;
    var muted = 0;
    late BuildContext pageContext;
    await tester.pumpWidget(
      MaterialApp(
        home: Builder(
          builder: (context) {
            pageContext = context;
            return Column(
              children: [
                PageShortcut(
                  activator: const SingleActivator(LogicalKeyboardKey.f5),
                  onInvoke: () => refreshes++,
                  child: const SizedBox(height: 10),
                ),
                TickerMode(
                  enabled: false,
                  child: PageShortcut(
                    activator: const SingleActivator(LogicalKeyboardKey.f5),
                    onInvoke: () => muted++,
                    child: const SizedBox(height: 10),
                  ),
                ),
              ],
            );
          },
        ),
      ),
    );
    await _chord(tester, LogicalKeyboardKey.f5);
    expect(refreshes, 1);
    expect(muted, 0);

    showDialog<void>(
      context: pageContext,
      builder: (_) => const AlertDialog(content: Text('busy')),
    );
    await tester.pumpAndSettle();
    await _chord(tester, LogicalKeyboardKey.f5);
    expect(refreshes, 1);
  }, variant: _windows);

  testWidgets('Ctrl+S applies the visible draft once', (tester) async {
    final engine = WorkflowEngine();
    final app = await pumpWorkflow(
      tester,
      engine,
      section: AppSection.settings,
    );
    await openSettingsRow(tester, app, 'advanced');
    await _chord(
      tester,
      LogicalKeyboardKey.keyS,
      modifier: LogicalKeyboardKey.controlLeft,
    );
    expect(engine.writes, 0);

    await tester.enterText(fieldWithLabel('SNI'), 'review.example');
    await tester.pumpAndSettle();
    await _chord(
      tester,
      LogicalKeyboardKey.keyS,
      modifier: LogicalKeyboardKey.controlLeft,
    );
    expect(engine.writes, 1);
    expect(app.activeProfile.sni, 'review.example');
    expect(find.text('Unapplied changes'), findsNothing);
  }, variant: _windows);
}
