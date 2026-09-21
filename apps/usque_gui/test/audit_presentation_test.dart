import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/home_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/animated_index_stack.dart';
import 'package:usque/widgets/connection_ring.dart';
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

  testWidgets(
    'observed outputs remain visible after desired outputs are disabled',
    (tester) async {
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
      expect(
        find.text('HTTP · ${app.strings.get('output_running')}'),
        findsOneWidget,
      );
      expect(
        find.text('SOCKS5 · ${app.strings.get('output_running')}'),
        findsOneWidget,
      );
      expect(
        find.text(
          '${app.strings.get('system_proxy')} · ${app.strings.get('output_running')}',
        ),
        findsOneWidget,
      );
      expect(find.text(app.strings.get('channel_only_warning')), findsNothing);
      app.snapshot = const EngineSnapshot();
      await tester.pumpWidget(page());
      await tester.pumpAndSettle();
      expect(
        find.text('HTTP · ${app.strings.get('output_running')}'),
        findsNothing,
      );
      expect(
        find.text(app.strings.get('channel_only_warning')),
        findsOneWidget,
      );
      await tester.pumpWidget(const SizedBox());
    },
  );
}
