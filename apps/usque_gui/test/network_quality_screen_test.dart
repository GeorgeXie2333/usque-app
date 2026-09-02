import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/screens/network_quality_screen.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/state/app_controller.dart';

import 'quality_test_support.dart';

Widget host(
  AppController app, {
  bool dark = false,
  double scale = 1,
  bool shell = false,
  bool disableAnimations = true,
}) => MaterialApp(
  theme: dark ? UsqueTheme.dark() : UsqueTheme.light(),
  home: Builder(
    builder: (context) => MediaQuery(
      data: MediaQuery.of(context).copyWith(
        textScaler: TextScaler.linear(scale),
        disableAnimations: disableAnimations,
      ),
      child: Scaffold(
        body: shell
            ? ShellScreen(controller: app)
            : NetworkQualityScreen(controller: app),
      ),
    ),
  ),
);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async {
    final material = FontLoader('MaterialIcons')
      ..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'));
    await material.load();
    final icons = FontLoader('packages/lucide_icons_flutter/Lucide')
      ..addFont(
        rootBundle.load('packages/lucide_icons_flutter/assets/lucide.ttf'),
      );
    await icons.load();
    for (final family in <String, List<String>>{
      'SpaceGrotesk': <String>['Medium', 'SemiBold', 'Bold'],
      'Manrope': <String>['Regular', 'Medium', 'SemiBold', 'Bold'],
      'IBMPlexMono': <String>['Regular', 'Medium'],
    }.entries) {
      final loader = FontLoader(family.key);
      for (final weight in family.value) {
        loader.addFont(
          rootBundle.load('assets/fonts/${family.key}-$weight.ttf'),
        );
      }
      await loader.load();
    }
  });

  for (final (size, locale, dark, scale, reducedMotion) in const [
    (Size(375, 900), LocalePreference.simplifiedChinese, false, 1.0, false),
    (Size(375, 900), LocalePreference.english, true, 2.0, true),
    (Size(900, 450), LocalePreference.simplifiedChinese, true, 1.0, true),
    (Size(900, 450), LocalePreference.english, false, 2.0, false),
  ]) {
    final variant =
        '${size.width}x${size.height} ${locale.name} dark=$dark scale=$scale';
    testWidgets(
      'quality queue details appear after scrolling disconnected view $variant',
      (tester) async {
        await tester.binding.setSurfaceSize(size);
        addTearDown(() => tester.binding.setSurfaceSize(null));
        final app = qualityApp(
          QualityEngineStub(),
          state: 'disconnected',
          locale: locale,
        )..selectSection(AppSection.networkQuality);
        addTearDown(app.dispose);
        await tester.pumpWidget(
          host(
            app,
            shell: true,
            dark: dark,
            scale: scale,
            disableAnimations: reducedMotion,
          ),
        );
        await tester.pumpAndSettle();

        final scrollable = find.descendant(
          of: find.byType(NetworkQualityScreen),
          matching: find.byType(Scrollable),
        );
        await tester.drag(scrollable, const Offset(0, -600));
        await tester.pumpAndSettle();
        expect(
          tester.state<ScrollableState>(scrollable).position.pixels,
          greaterThan(0),
        );
        expect(find.byType(ExpansionTile), findsNothing);

        app.snapshot = EngineSnapshot(
          phase: ConnectionPhase.connected,
          transport: 'HTTP/3',
          addressFamily: 'IPv4',
          networkQuality: qualityFixture(DateTime.utc(2026, 9, 2, 12)),
        );
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        expect(find.byType(ErrorWidget), findsNothing);
        expect(find.byType(ExpansionTile), findsOneWidget);

        await tester.scrollUntilVisible(
          find.text(app.strings.get('nq_doctor_evidence')),
          500,
          scrollable: scrollable,
          maxScrolls: 20,
        );
        await tester.pumpAndSettle();
        expect(
          find.text(app.strings.get('nq_doctor_evidence')).hitTestable(),
          findsOneWidget,
        );
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
      },
    );

    testWidgets(
      'quality queue expansion and scroll restore independently $variant',
      (tester) async {
        await tester.binding.setSurfaceSize(size);
        addTearDown(() => tester.binding.setSurfaceSize(null));
        final app = qualityApp(QualityEngineStub(), locale: locale)
          ..selectSection(AppSection.networkQuality);
        addTearDown(app.dispose);
        await tester.pumpWidget(
          host(
            app,
            shell: true,
            dark: dark,
            scale: scale,
            disableAnimations: reducedMotion,
          ),
        );
        await tester.pumpAndSettle();

        final scrollable = find.descendant(
          of: find.byType(NetworkQualityScreen),
          matching: find.byType(Scrollable),
        );
        final details = find.text(app.strings.get('nq_queue_details'));
        await tester.scrollUntilVisible(details, 500, scrollable: scrollable);
        await tester.pumpAndSettle();
        expect(details.hitTestable(), findsOneWidget);
        await tester.tap(details);
        await tester.pumpAndSettle();
        expect(find.text(app.strings.get('nq_h3WireSend')), findsOneWidget);
        final savedOffset = tester
            .state<ScrollableState>(scrollable)
            .position
            .pixels;
        expect(savedOffset, greaterThan(0));

        final navigation = find.byType(
          size.width >= 760 ? NavigationRail : NavigationBar,
        );
        final settingsDestination = find.descendant(
          of: navigation,
          matching: find.text(app.strings.get('nav_settings')),
        );
        await tester.ensureVisible(settingsDestination);
        await tester.pumpAndSettle();
        expect(settingsDestination.hitTestable(), findsOneWidget);
        await tester.tap(settingsDestination);
        await tester.pumpAndSettle();
        expect(app.section, AppSection.settings);
        final qualityDestination = find.descendant(
          of: navigation,
          matching: find.text(app.strings.get('nav_network_quality')),
        );
        await tester.ensureVisible(qualityDestination);
        await tester.pumpAndSettle();
        expect(qualityDestination.hitTestable(), findsOneWidget);
        await tester.tap(qualityDestination);
        await tester.pumpAndSettle();
        expect(app.section, AppSection.networkQuality);
        expect(tester.takeException(), isNull);
        expect(find.byType(ErrorWidget), findsNothing);
        expect(find.text(app.strings.get('nq_h3WireSend')), findsOneWidget);
        expect(
          tester.state<ScrollableState>(scrollable).position.pixels,
          closeTo(savedOffset, 1),
        );
        await tester.pumpWidget(const SizedBox.shrink());
      },
    );
  }

  for (final width in <double>[375, 768, 1280, 1920]) {
    for (final locale in <LocalePreference>[
      LocalePreference.english,
      LocalePreference.simplifiedChinese,
    ]) {
      for (final dark in <bool>[false, true]) {
        testWidgets('quality $width ${locale.name} dark=$dark at 200 percent', (
          tester,
        ) async {
          await tester.binding.setSurfaceSize(Size(width, 900));
          addTearDown(() => tester.binding.setSurfaceSize(null));
          final app = qualityApp(
            QualityEngineStub(),
            state: 'degraded',
            locale: locale,
          );
          await tester.pumpWidget(host(app, dark: dark, scale: 2));
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);
          expect(find.text(app.strings.get('nq_poor')), findsOneWidget);
          await tester.drag(
            find.byType(CustomScrollView).first,
            const Offset(0, -10000),
          );
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);
          expect(find.text(app.strings.get('nq_direct_dns')), findsOneWidget);
          expect(find.textContaining('private.example'), findsNothing);
          await tester.pumpWidget(const SizedBox.shrink());
          app.dispose();
        });
      }
    }
  }

  testWidgets('H2 has protocol PING but no loss, PMTU or migration values', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(1280, 2000));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final app = qualityApp(QualityEngineStub(), state: 'h2');
    await tester.pumpWidget(host(app));
    expect(find.text('HTTP/2'), findsOneWidget);
    expect(find.text(app.strings.get('nq_h2_ping')), findsOneWidget);
    expect(find.text(app.strings.get('nq_loss_h2')), findsOneWidget);
    expect(find.text('0.10%'), findsNothing);
    expect(find.text('1.3 KiB'), findsNothing);
    app.dispose();
  });

  for (final locale in <LocalePreference>[
    LocalePreference.english,
    LocalePreference.simplifiedChinese,
  ]) {
    testWidgets('five phone destinations at 200 percent ${locale.name}', (
      tester,
    ) async {
      await tester.binding.setSurfaceSize(const Size(375, 900));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final app = qualityApp(QualityEngineStub(), locale: locale)
        ..selectSection(AppSection.networkQuality);
      await tester.pumpWidget(host(app, scale: 2, shell: true));
      await tester.pumpAndSettle();
      expect(
        tester.widget<NavigationBar>(find.byType(NavigationBar)).destinations,
        hasLength(5),
      );
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      app.dispose();
    });
  }

  testWidgets(
    'graphs have text alternatives and controls support keyboard traversal',
    (tester) async {
      await tester.binding.setSurfaceSize(const Size(1280, 2000));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final app = qualityApp(QualityEngineStub());
      final semantics = tester.ensureSemantics();
      await tester.pumpWidget(host(app));
      expect(
        find.bySemanticsLabel(RegExp(r'Round-trip time.*60/60.*Range')),
        findsWidgets,
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      final first = FocusManager.instance.primaryFocus;
      expect(first, isNotNull);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      expect(FocusManager.instance.primaryFocus, isNot(same(first)));
      await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      expect(FocusManager.instance.primaryFocus, same(first));
      await tester.tap(find.text(app.strings.get('nq_pause')));
      await tester.pump();
      expect(app.quality.paused, isTrue);
      expect(find.text(app.strings.get('nq_paused')), findsOneWidget);
      semantics.dispose();
      app.dispose();
    },
  );

  testWidgets(
    'capability controls navigation and TV D-pad uses visible sections',
    (tester) async {
      await tester.binding.setSurfaceSize(const Size(1920, 1080));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final app = qualityApp(QualityEngineStub());
      await tester.pumpWidget(host(app, shell: true));
      expect(
        tester.widget<NavigationRail>(find.byType(NavigationRail)).destinations,
        hasLength(5),
      );
      await tester.tap(find.text(app.strings.get('proxy')).first);
      await tester.pumpAndSettle();
      final proxyLabel = find.descendant(
        of: find.byType(NavigationRail),
        matching: find.text(app.strings.get('proxy')),
      );
      Focus.of(tester.element(proxyLabel)).requestFocus();
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
      await tester.pumpAndSettle();
      expect(app.section, AppSection.networkQuality);
      app.engineCapabilities = const EngineCapabilities();
      app.selectSection(AppSection.home);
      await tester.pump();
      expect(app.availableSections, hasLength(4));
      expect(
        tester.widget<NavigationRail>(find.byType(NavigationRail)).destinations,
        hasLength(4),
      );
      expect(app.section, AppSection.home);
      app.dispose();
    },
  );

  testWidgets('one tap runs Standard; Deep requires explicit dialog consent', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(1280, 1000));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final engine = QualityEngineStub();
    final app = qualityApp(engine);
    await tester.pumpWidget(host(app));
    await tester.tap(
      find.byKey(const ValueKey<String>('network-doctor-standard')),
    );
    await tester.pumpAndSettle();
    expect(engine.modes, <DiagnosticMode>[DiagnosticMode.standard]);
    await tester.tap(find.text(app.strings.get('diag_mode_deep')));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text(app.strings.get('diag_start')));
    await tester.tap(find.text(app.strings.get('diag_start')));
    await tester.pumpAndSettle();
    expect(find.text(app.strings.get('nq_doctor_deep_title')), findsOneWidget);
    expect(engine.modes, hasLength(1));
    await tester.tap(find.text(app.strings.get('cancel')));
    await tester.pumpAndSettle();
    expect(engine.modes, hasLength(1));
    await tester.tap(find.text(app.strings.get('diag_start')));
    await tester.pumpAndSettle();
    await tester.tap(find.text(app.strings.get('nq_doctor_deep_run')));
    await tester.pumpAndSettle();
    expect(engine.modes, <DiagnosticMode>[
      DiagnosticMode.standard,
      DiagnosticMode.deep,
    ]);
    app.dispose();
  });

  for (final state in <String>[
    'disconnected',
    'h2',
    'h3',
    'migration',
    'degraded',
    'pmtu_degraded',
    'dns_degraded',
    'stale',
  ]) {
    testWidgets('quality golden $state', (tester) async {
      await tester.binding.setSurfaceSize(const Size(1280, 2400));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final app = qualityApp(QualityEngineStub(), state: state);
      await tester.pumpWidget(
        host(app, dark: state == 'h2' || state == 'degraded'),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await expectLater(
        find.byType(Scaffold).first,
        matchesGoldenFile('goldens/network_quality_$state.png'),
      );
      app.dispose();
    });
  }
}
