import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/app.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/screens/onboarding_screen.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/usque_logo.dart';

import 'app_test.dart' show FakeEngineClient, advanceToOnboardingIdentity;

const Size _desktop = Size(1280, 900);

void _useSize(WidgetTester tester, Size size) {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = size;
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
}

Future<void> _finishSetup(WidgetTester tester) async {
  await advanceToOnboardingIdentity(tester);
  final finish = find.widgetWithText(FilledButton, 'Finish setup');
  await tester.ensureVisible(finish);
  await tester.pumpAndSettle();
  await tester.tap(finish);
}

/// Pumps single frames until the shell has been added to the tree.
Future<void> _pumpUntilShell(WidgetTester tester) async {
  for (var frame = 0; frame < 50; frame++) {
    await tester.pump(const Duration(milliseconds: 1));
    if (find.byType(ShellScreen).evaluate().isNotEmpty) return;
  }
  fail('The shell never appeared after setup completed.');
}

Finder _setupLogo() => find.descendant(
  of: find.byType(OnboardingScreen),
  matching: find.byType(UsqueLogo),
);

void main() {
  setUp(() => SharedPreferences.setMockInitialValues(<String, Object>{}));

  testWidgets('completing setup hands over to the shell without input', (
    tester,
  ) async {
    _useSize(tester, _desktop);
    await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
    await tester.pumpAndSettle();
    await _finishSetup(tester);
    await _pumpUntilShell(tester);

    // Both screens share the frame, and setup can no longer be reached.
    expect(find.byType(OnboardingScreen), findsOneWidget);
    for (final matcher in <bool Function(Widget)>[
      (widget) => widget is IgnorePointer && widget.ignoring,
      (widget) => widget is ExcludeFocus && widget.excluding,
      (widget) => widget is ExcludeSemantics && widget.excluding,
    ]) {
      expect(
        find.ancestor(
          of: find.byType(OnboardingScreen),
          matching: find.byWidgetPredicate(matcher),
        ),
        findsOneWidget,
      );
    }
    expect(find.text('Finish setup').hitTestable(), findsNothing);
    // Setup holds its last frame instead of showing the account it just
    // created.
    expect(find.text('Continue with saved account'), findsNothing);
    expect(find.text('Your saved WARP account is ready.'), findsNothing);

    await tester.pump(const Duration(milliseconds: 360));
    expect(find.byType(OnboardingScreen), findsOneWidget);

    await tester.pumpAndSettle();
    expect(find.byType(OnboardingScreen), findsNothing);
    expect(find.byType(ShellScreen), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('reduced motion opens the shell directly', (tester) async {
    _useSize(tester, _desktop);
    tester.platformDispatcher.accessibilityFeaturesTestValue =
        const FakeAccessibilityFeatures(disableAnimations: true);
    addTearDown(tester.platformDispatcher.clearAccessibilityFeaturesTestValue);
    await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
    await tester.pumpAndSettle();
    await _finishSetup(tester);
    await _pumpUntilShell(tester);

    expect(find.byType(OnboardingScreen), findsNothing);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('the setup mark folds onto the extended rail mark', (
    tester,
  ) async {
    _useSize(tester, _desktop);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
    await tester.pumpAndSettle();
    final rail = find.byType(NavigationRail);
    final railLogo = find.descendant(
      of: rail,
      matching: find.byType(UsqueLogo),
    );
    expect(tester.getRect(railLogo), ShellScreen.railBrandLogo);
    expect(
      tester.getSize(rail).width + 1,
      ShellScreen.railExtent(_desktop.width),
    );
    await tester.pumpWidget(const SizedBox.shrink());

    SharedPreferences.setMockInitialValues(<String, Object>{});
    final app = AppController(FakeEngineClient());
    addTearDown(app.dispose);
    Future<Rect> setupLogoAt(double exit) async {
      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: OnboardingScreen(
            controller: app,
            exit: exit == 0 ? null : AlwaysStoppedAnimation<double>(exit),
          ),
        ),
      );
      await tester.pump();
      return tester.getRect(_setupLogo());
    }

    expect(await setupLogoAt(0), const Rect.fromLTWH(48, 48, 72, 72));
    // The fold ends at 65 % of the hand-off, before the mark fades.
    final landed = await setupLogoAt(0.66);
    expect(landed.left, moreOrLessEquals(ShellScreen.railBrandLogo.left));
    expect(landed.top, moreOrLessEquals(ShellScreen.railBrandLogo.top));
    expect(landed.width, moreOrLessEquals(ShellScreen.railBrandLogo.width));
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
