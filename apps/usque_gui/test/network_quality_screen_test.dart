import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/screens/network_quality_screen.dart';
import 'package:usque/screens/settings_screen.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/common.dart';

import 'quality_test_support.dart';

Widget host(
  AppController app, {
  bool dark = false,
  double scale = 1,
  bool shell = false,
  bool disableAnimations = true,
}) => MaterialApp(
  theme: dark ? UsqueTheme.dark() : UsqueTheme.light(),
  builder: (context, child) => MediaQuery(
    data: MediaQuery.of(context).copyWith(
      textScaler: TextScaler.linear(scale),
      disableAnimations: disableAnimations,
    ),
    child: child ?? const SizedBox.shrink(),
  ),
  home: Scaffold(
    body: shell
        ? ShellScreen(controller: app)
        : NetworkQualityScreen(controller: app),
  ),
);

Future<void> _openQualityFromSettings(
  WidgetTester tester,
  AppController app, {
  double scale = 1,
  bool disableAnimations = true,
}) async {
  final card = find.widgetWithText(Panel, app.strings.get('network_quality'));
  expect(card, findsOneWidget);
  await tester.ensureVisible(card);
  await tester.pumpAndSettle();
  expect(card.hitTestable(), findsOneWidget);
  await tester.tap(card);
  await tester.pumpAndSettle();
  expect(find.byType(NetworkQualityScreen), findsOneWidget);
  expect(app.section, AppSection.settings);
  final context = tester.element(find.byType(NetworkQualityScreen));
  expect(MediaQuery.textScalerOf(context).scale(16), 16 * scale);
  expect(MediaQuery.disableAnimationsOf(context), disableAnimations);
}

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

  testWidgets('Quality lives in Settings with its own icon and back path', (
    tester,
  ) async {
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final semantics = tester.ensureSemantics();
    try {
      for (final size in const <Size>[Size(375, 900), Size(1280, 1100)]) {
        for (final locale in const <LocalePreference>[
          LocalePreference.english,
          LocalePreference.simplifiedChinese,
        ]) {
          await tester.binding.setSurfaceSize(size);
          final engine = QualityEngineStub();
          final app = qualityApp(engine, locale: locale)
            ..selectSection(AppSection.settings);
          addTearDown(app.dispose);
          await tester.pumpWidget(
            host(
              app,
              shell: true,
              scale: 2,
              dark: locale == LocalePreference.simplifiedChinese,
            ),
          );
          await tester.pumpAndSettle();

          final navigation = size.width < 760
              ? find.byType(NavigationBar)
              : find.byType(NavigationRail);
          expect(app.availableSections, hasLength(4));
          expect(
            find.descendant(
              of: navigation,
              matching: find.text(app.strings.get('nav_network_quality')),
            ),
            findsNothing,
          );

          final qualityCard = find.widgetWithText(
            Panel,
            app.strings.get('network_quality'),
          );
          final diagnosticsCard = find.widgetWithText(
            Panel,
            app.strings.get('diagnostics'),
          );
          expect(qualityCard, findsOneWidget);
          expect(diagnosticsCard, findsOneWidget);
          final qualityTitle = tester.widget<SectionTitle>(
            find.descendant(
              of: qualityCard,
              matching: find.byType(SectionTitle),
            ),
          );
          final diagnosticsTitle = tester.widget<SectionTitle>(
            find.descendant(
              of: diagnosticsCard,
              matching: find.byType(SectionTitle),
            ),
          );
          expect(qualityTitle.icon, LucideIcons.gauge);
          expect(qualityTitle.icon, isNot(diagnosticsTitle.icon));

          await tester.ensureVisible(qualityCard);
          await tester.pumpAndSettle();
          expect(tester.getSize(qualityCard).height, greaterThanOrEqualTo(48));
          expect(
            tester.getSemantics(qualityCard).rect.height,
            greaterThanOrEqualTo(48),
          );
          final settingsScroll = find.descendant(
            of: find.byType(SettingsScreen),
            matching: find.byType(Scrollable),
          );
          final settingsOffset = tester
              .state<ScrollableState>(settingsScroll)
              .position
              .pixels;
          await tester.tap(qualityCard);
          await tester.pumpAndSettle();
          expect(find.byType(NetworkQualityScreen), findsOneWidget);
          expect(navigation, findsNothing);
          expect(app.section, AppSection.settings);
          expect(engine.modes, isEmpty);
          expect(tester.takeException(), isNull);

          await tester.tap(
            find.widgetWithText(TextButton, app.strings.get('back')),
          );
          await tester.pumpAndSettle();
          expect(find.byType(NetworkQualityScreen), findsNothing);
          expect(find.byType(SettingsScreen), findsOneWidget);
          expect(app.section, AppSection.settings);
          expect(navigation, findsOneWidget);
          expect(
            tester.state<ScrollableState>(settingsScroll).position.pixels,
            closeTo(settingsOffset, 1),
          );
          expect(tester.takeException(), isNull);
          await tester.pumpWidget(const SizedBox.shrink());
        }
      }
    } finally {
      semantics.dispose();
    }
  });

  testWidgets('Quality subpage stays live and handles capability loss', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(430, 900));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final app = qualityApp(QualityEngineStub())
      ..selectSection(AppSection.settings);
    addTearDown(app.dispose);
    await tester.pumpWidget(host(app, shell: true));
    await tester.pumpAndSettle();
    await _openQualityFromSettings(tester, app);
    expect(find.text('44 ms'), findsOneWidget);

    app.networkQuality = qualityFixture(
      DateTime.utc(2026, 9, 2, 12, 1),
      rtt: 123,
    );
    await tester.pumpAndSettle();
    expect(find.text('123 ms'), findsOneWidget);
    expect(find.text('44 ms'), findsNothing);
    expect(app.section, AppSection.settings);

    app.engineCapabilities = const EngineCapabilities();
    app.selectSection(AppSection.settings);
    await tester.pumpAndSettle();
    expect(find.byType(NetworkQualityScreen), findsOneWidget);
    expect(
      find.widgetWithText(WarningBanner, app.strings.get('nq_unsupported')),
      findsOneWidget,
    );
    expect(tester.takeException(), isNull);
    await tester.binding.handlePopRoute();
    await tester.pumpAndSettle();
    expect(find.byType(SettingsScreen), findsOneWidget);
    expect(
      find.widgetWithText(Panel, app.strings.get('network_quality')),
      findsNothing,
    );
    expect(app.availableSections, hasLength(4));
    expect(app.section, AppSection.settings);
    await tester.pumpWidget(const SizedBox.shrink());
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
        )..selectSection(AppSection.settings);
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
        await _openQualityFromSettings(
          tester,
          app,
          scale: scale,
          disableAnimations: reducedMotion,
        );

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
          ..selectSection(AppSection.settings);
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
        await _openQualityFromSettings(
          tester,
          app,
          scale: scale,
          disableAnimations: reducedMotion,
        );

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

        final scrollContext = tester.element(scrollable);
        final detailsContext = tester.element(find.byType(ExpansionTile));
        expect(
          PageStorage.of(scrollContext).readState(scrollContext),
          closeTo(savedOffset, 1),
        );
        expect(
          PageStorage.of(detailsContext).readState(detailsContext),
          isTrue,
        );

        await tester.binding.handlePopRoute();
        await tester.pumpAndSettle();
        expect(app.section, AppSection.settings);
        expect(find.byType(NetworkQualityScreen), findsNothing);
        await _openQualityFromSettings(
          tester,
          app,
          scale: scale,
          disableAnimations: reducedMotion,
        );
        expect(tester.takeException(), isNull);
        expect(find.byType(ErrorWidget), findsNothing);
        expect(find.text(app.strings.get('nq_h3WireSend')), findsNothing);
        expect(tester.state<ScrollableState>(scrollable).position.pixels, 0);
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
    testWidgets('four phone destinations at 200 percent ${locale.name}', (
      tester,
    ) async {
      await tester.binding.setSurfaceSize(const Size(375, 900));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final app = qualityApp(QualityEngineStub(), locale: locale)
        ..selectSection(AppSection.settings);
      await tester.pumpWidget(host(app, scale: 2, shell: true));
      await tester.pumpAndSettle();
      expect(
        tester.widget<NavigationBar>(find.byType(NavigationBar)).destinations,
        hasLength(4),
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
    'quality capability gates Settings entry without changing TV navigation',
    (tester) async {
      await tester.binding.setSurfaceSize(const Size(1920, 1080));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final app = qualityApp(QualityEngineStub());
      await tester.pumpWidget(host(app, shell: true));
      expect(
        tester.widget<NavigationRail>(find.byType(NavigationRail)).destinations,
        hasLength(4),
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
      expect(app.section, AppSection.settings);
      final qualityCard = find.widgetWithText(
        Panel,
        app.strings.get('network_quality'),
      );
      expect(qualityCard, findsOneWidget);
      app.engineCapabilities = const EngineCapabilities();
      app.selectSection(AppSection.settings);
      await tester.pump();
      expect(app.availableSections, hasLength(4));
      expect(
        tester.widget<NavigationRail>(find.byType(NavigationRail)).destinations,
        hasLength(4),
      );
      expect(app.section, AppSection.settings);
      expect(qualityCard, findsNothing);
      app.engineCapabilities = const EngineCapabilities(networkQuality: true);
      app.selectSection(AppSection.settings);
      await tester.pumpAndSettle();
      expect(qualityCard, findsOneWidget);
      expect(app.availableSections, hasLength(4));
      await tester.pumpWidget(const SizedBox.shrink());
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

  testWidgets('short rail keeps keyboard selection visible and focused', (
    tester,
  ) async {
    addTearDown(() => tester.binding.setSurfaceSize(null));
    for (final height in <double>[375, 450]) {
      await tester.binding.setSurfaceSize(Size(900, height));
      final app = qualityApp(QualityEngineStub(), state: 'disconnected');
      addTearDown(app.dispose);
      await tester.pumpWidget(
        host(app, shell: true, scale: 2, disableAnimations: false),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      final rail = find.byType(NavigationRail);
      final homeLabel = find.byWidget(
        tester.widget<NavigationRail>(rail).destinations.first.label,
      );
      Focus.of(tester.element(homeLabel)).requestFocus();
      await tester.pump();
      for (final (key, section) in <(LogicalKeyboardKey, AppSection)>[
        (LogicalKeyboardKey.arrowUp, AppSection.settings),
        (LogicalKeyboardKey.arrowDown, AppSection.home),
        (LogicalKeyboardKey.arrowUp, AppSection.settings),
      ]) {
        await tester.sendKeyEvent(key);
        await tester.pumpAndSettle();
        expect(app.section, section);
        expect(tester.takeException(), isNull);
        final navigation = tester.widget<NavigationRail>(rail);
        final selectedLabel = find.byWidget(
          navigation.destinations[navigation.selectedIndex!].label,
        );
        expect(
          selectedLabel.hitTestable(),
          findsOneWidget,
          reason: '${section.name} at height $height',
        );
        final destination = navigation.destinations[navigation.selectedIndex!];
        expect(
          find.byWidget(destination.selectedIcon).hitTestable(),
          findsOneWidget,
          reason: '${section.name} icon at height $height',
        );
        expect(
          FocusManager.instance.primaryFocus,
          same(Focus.of(tester.element(selectedLabel))),
        );
      }
      await tester.pumpWidget(const SizedBox.shrink());
    }
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
