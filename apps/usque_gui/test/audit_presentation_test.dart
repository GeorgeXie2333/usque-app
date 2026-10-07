import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/home_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/animated_index_stack.dart';
import 'package:usque/widgets/common.dart';
import 'package:usque/widgets/connection_ring.dart';
import 'package:usque/widgets/country_flag.dart';
import 'package:usque/widgets/window_titlebar.dart';
import 'app_test.dart' show FakeEngineClient;
import 'ui_workflow_test.dart' show workflowHost;

Widget shell(Widget child) => MaterialApp(
  theme: UsqueTheme.light(),
  home: Scaffold(body: child),
);

void main() {
  for (final duration in [Duration.zero, const Duration(milliseconds: 300)]) {
    testWidgets(
      'inactive section releases focus before fade completes: $duration',
      (tester) async {
        final oldFocus = FocusNode(), newFocus = FocusNode();
        var oldCalls = 0, newCalls = 0;
        Widget page(int index) => shell(
          AnimatedIndexStack(
            index: index,
            duration: duration,
            children: [
              TextButton(
                focusNode: oldFocus,
                onPressed: () => oldCalls++,
                child: const Text('Old action'),
              ),
              TextButton(
                focusNode: newFocus,
                onPressed: () => newCalls++,
                child: const Text('New action'),
              ),
            ],
          ),
        );
        await tester.pumpWidget(page(0));
        oldFocus.requestFocus();
        await tester.pump();
        expect(oldFocus.hasFocus, isTrue);
        await tester.pumpWidget(page(1));
        await tester.pump();
        expect(oldFocus.hasFocus, isFalse);
        newFocus.requestFocus();
        await tester.pump();
        await tester.sendKeyEvent(LogicalKeyboardKey.enter);
        await tester.pump();
        expect(oldCalls, 0);
        expect(newCalls, 1);
        await tester.pumpAndSettle();
        await tester.pumpWidget(const SizedBox());
        oldFocus.dispose();
        newFocus.dispose();
      },
    );
  }

  testWidgets('RTL body keeps native caption buttons at the physical right', (
    tester,
  ) async {
    final strings = AppStrings(LocalePreference.english);
    await tester.pumpWidget(
      shell(
        Directionality(
          textDirection: TextDirection.rtl,
          child: Column(
            children: [
              WindowTitleBar(
                strings: strings,
                phase: ConnectionPhase.disconnected,
              ),
              const Text('Body', key: ValueKey('body-direction')),
            ],
          ),
        ),
      ),
    );
    final title = tester.getRect(find.byType(WindowTitleBar));
    Rect button(String key) => tester.getRect(
      find.byWidgetPredicate(
        (widget) =>
            widget is Semantics && widget.properties.label == strings.get(key),
      ),
    );
    expect(button('window_close').right, title.right);
    expect(button('window_close').width, kWindowCaptionButtonWidth);
    expect(
      button('window_maximize').right,
      title.right - kWindowCaptionButtonWidth,
    );
    expect(
      button('window_minimize').right,
      title.right - 2 * kWindowCaptionButtonWidth,
    );
    expect(
      Directionality.of(
        tester.element(find.byKey(const ValueKey('body-direction'))),
      ),
      TextDirection.rtl,
    );
  });

  testWidgets(
    'reduced motion stops an active scan and lock transition immediately',
    (tester) async {
      Widget page(bool reduced, ConnectionPhase phase) => shell(
        MediaQuery(
          data: MediaQueryData(disableAnimations: reduced),
          child: ConnectionRing(
            phase: phase,
            busy: false,
            actionLabel: 'Connect',
            onPressed: () {},
          ),
        ),
      );
      await tester.pumpWidget(page(false, ConnectionPhase.connectingH3));
      final animation = tester
          .widgetList<AnimatedBuilder>(
            find.descendant(
              of: find.byType(ConnectionRing),
              matching: find.byType(AnimatedBuilder),
            ),
          )
          .map((widget) => widget.animation)
          .whereType<AnimationController>()
          .first;
      expect(animation.isAnimating, isTrue);
      await tester.pumpWidget(page(true, ConnectionPhase.connectingH3));
      expect(animation.isAnimating, isFalse);
      expect(animation.value, 1);
      await tester.pumpWidget(page(false, ConnectionPhase.connectingH3));
      expect(animation.isAnimating, isTrue);
      await tester.pumpWidget(page(false, ConnectionPhase.connected));
      expect(animation.isAnimating, isTrue);
      await tester.pumpWidget(page(true, ConnectionPhase.connected));
      expect(animation.isAnimating, isFalse);
      expect(animation.value, 1);
      await tester.pumpWidget(page(false, ConnectionPhase.connected));
      expect(animation.isAnimating, isFalse);
      await tester.pumpWidget(const SizedBox());
    },
  );

  testWidgets('Home details contain only IPs and configured interface names', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(1280, 900));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final app = AppController(FakeEngineClient())
      ..localePreference = LocalePreference.english;
    addTearDown(app.dispose);
    app.sharedNetwork = app.sharedNetwork.copyWith(
      frontends: const FrontendSettings(
        tunnel: false,
        socks5: false,
        http: false,
      ),
    );
    app.snapshot = const EngineSnapshot(
      phase: ConnectionPhase.connected,
      transport: 'HTTP/3',
      addressFamily: 'IPv4',
      exit: ExitInfo(
        country: 'Singapore',
        countryCode: 'SG',
        ipv4: '198.51.100.10',
        ipv6: '2001:db8::10',
      ),
      frontends: [
        FrontendRuntimeStatus(
          kind: FrontendKind.http,
          phase: FrontendPhase.active,
          listeners: ['127.0.0.1:8080'],
        ),
        FrontendRuntimeStatus(
          kind: FrontendKind.socks5,
          phase: FrontendPhase.active,
          listeners: ['127.0.0.1:1080'],
        ),
        FrontendRuntimeStatus(
          kind: FrontendKind.systemProxy,
          phase: FrontendPhase.active,
        ),
      ],
    );
    Widget page() => workflowHost(app, home: HomeScreen(controller: app));
    await tester.pumpWidget(page());
    await tester.pumpAndSettle();
    await tester.tap(find.text(app.strings.get('connection_details')));
    await tester.pumpAndSettle();
    final details = find.byKey(const ValueKey('home-connection-detail-values'));
    Finder detailText(String value) =>
        find.descendant(of: details, matching: find.text(value));
    expect(detailText('198.51.100.10'), findsOneWidget);
    expect(detailText('2001:db8::10'), findsOneWidget);
    expect(detailText('Exit IP:'), findsOneWidget);
    expect(detailText('IPv4'), findsNothing);
    expect(detailText('IPv6'), findsNothing);
    for (final key in [
      'protocol',
      'address_family',
      'duration',
      'location',
      'outputs',
    ]) {
      expect(detailText(app.strings.get(key)), findsNothing);
    }
    expect(detailText('Singapore'), findsNothing);
    expect(detailText('ip.sb'), findsNothing);
    expect(detailText('HTTP/3'), findsNothing);
    expect(
      find.descendant(of: details, matching: find.byType(InlineStatus)),
      findsNothing,
    );
    final names = [
      app.strings.tunnelOutputLabel(Theme.of(tester.element(details)).platform),
      app.strings.get('home_system_proxy'),
      'HTTP',
      'SOCKS5',
    ];
    expect(find.byKey(const ValueKey('home-enabled-interfaces')), findsNothing);
    // Runtime can lag saved preferences. Only configured interfaces belong
    // in this list; it does not claim that any output is running.
    for (final name in names) {
      expect(detailText(name), findsNothing);
    }
    expect(find.text(app.strings.get('channel_only_warning')), findsNothing);
    app.sharedNetwork = app.sharedNetwork.copyWith(
      frontends: const FrontendSettings(tunnel: true, socks5: true, http: true),
      proxy: app.sharedNetwork.proxy.copyWith(systemProxy: true),
      geoDirectCountries: const ['CN'],
    );
    await tester.pumpWidget(page());
    await tester.pumpAndSettle();
    final enabled = find.byKey(const ValueKey('home-enabled-interfaces'));
    expect(enabled, findsOneWidget);
    expect(tester.widget<Text>(enabled).data, 'Enabled: ${names.join(', ')}');
    expect(
      find.descendant(of: details, matching: find.byType(Icon)),
      findsNothing,
    );
    expect(
      tester.getTopLeft(find.text('Exit IP:')).dx,
      tester.getTopLeft(enabled).dx,
    );
    expect(
      detailText(app.strings.get('geo_chip').replaceAll('{current}', 'CN')),
      findsNothing,
    );
    for (final exit in const [
      ExitInfo(ipv4: '198.51.100.10'),
      ExitInfo(ipv6: '2001:db8::10'),
      ExitInfo(ipv4: '198.51.100.10', ipv6: '  '),
    ]) {
      app.snapshot = EngineSnapshot(
        phase: ConnectionPhase.connected,
        exit: exit,
      );
      await tester.pumpWidget(page());
      await tester.pumpAndSettle();
      expect(
        find.descendant(of: details, matching: find.byType(MonoValue)),
        findsOneWidget,
      );
      expect(
        find.descendant(of: details, matching: find.byType(EmptyValue)),
        findsNothing,
      );
    }
    app.sharedNetwork = app.sharedNetwork.copyWith(
      frontends: const FrontendSettings(
        tunnel: false,
        socks5: true,
        http: false,
      ),
      proxy: app.sharedNetwork.proxy.copyWith(systemProxy: false),
    );
    app.snapshot = const EngineSnapshot(
      exit: ExitInfo(ipv4: '198.51.100.10', ipv6: '2001:db8::10'),
    );
    await tester.pumpWidget(page());
    await tester.pumpAndSettle();
    expect(detailText('Enabled: SOCKS5'), findsOneWidget);
    for (final name in names.take(3)) {
      expect(detailText(name), findsNothing);
    }
    expect(detailText('198.51.100.10'), findsNothing);
    expect(detailText('2001:db8::10'), findsNothing);
    expect(
      find.descendant(of: details, matching: find.byType(EmptyValue)),
      findsOneWidget,
    );
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('desktop Location keeps its icon and shows flag before country', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(1020, 728));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final app = AppController(FakeEngineClient())
      ..localePreference = LocalePreference.english;
    addTearDown(app.dispose);
    for (final country in ['United States', null, '  ']) {
      app.snapshot = EngineSnapshot(
        phase: ConnectionPhase.connected,
        transport: 'HTTP/3',
        addressFamily: 'IPv4',
        connectedAt: DateTime.now().subtract(const Duration(minutes: 2)),
        exit: ExitInfo(city: 'New York', country: country, countryCode: 'US'),
      );
      await tester.pumpWidget(workflowHost(app));
      await tester.pumpAndSettle();
      final location = find.byKey(const ValueKey('home-exit-location'));
      final icon = find.descendant(
        of: location,
        matching: find.byIcon(LucideIcons.mapPin),
      );
      final flag = find.descendant(
        of: location,
        matching: find.byType(CountryFlag),
      );
      final value = find.descendant(
        of: location,
        matching: find.text(
          country?.trim().isNotEmpty == true
              ? country!
              : app.strings.get('not_available'),
        ),
      );
      expect(icon, findsOneWidget);
      expect(flag, findsOneWidget);
      expect(value, findsOneWidget);
      expect(find.textContaining('New York'), findsNothing);
      expect(tester.getRect(icon).right, lessThan(tester.getRect(flag).left));
      expect(tester.getRect(flag).right, lessThan(tester.getRect(value).left));
      expect(
        tester.getTopLeft(find.text('Duration')).dy,
        tester.getTopLeft(find.text('IP version')).dy,
      );
      expect(tester.takeException(), isNull);
    }
    await tester.pumpWidget(const SizedBox());
  });
}
