import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/state/window_frame.dart';
import 'package:usque/widgets/window_titlebar.dart';

void main() {
  const channel = MethodChannel('io.github.georgexie2333.usque/window_frame');
  tearDown(WindowFrame.instance.debugReset);

  for (final active in [true, false]) {
    testWidgets('native push supersedes pending frame read (active: $active)', (
      tester,
    ) async {
      final reply = Completer<Map<Object?, Object?>>();
      final messenger = tester.binding.defaultBinaryMessenger;
      messenger.setMockMethodCallHandler(channel, (_) => reply.future);
      addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
      final frame = WindowFrame.instance;
      frame.enable();
      await tester.pump();
      tester.binding.channelBuffers.push(
        channel.name,
        const StandardMethodCodec().encodeMethodCall(
          MethodCall('windowFrameChanged', {
            'active': active,
            'maximized': false,
          }),
        ),
        (_) {},
      );
      await tester.pump();
      reply.complete({'active': !active, 'maximized': true});
      await tester.pump();
      expect(frame.active, active);
      expect(frame.maximized, isFalse);
    });
  }

  testWidgets('newer frame refresh wins when replies arrive out of order', (
    tester,
  ) async {
    final replies = <Completer<Map<Object?, Object?>>>[];
    final messenger = tester.binding.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(channel, (_) {
      final reply = Completer<Map<Object?, Object?>>();
      replies.add(reply);
      return reply.future;
    });
    addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
    final frame = WindowFrame.instance;
    frame.enable();
    await tester.pump();
    final refresh = frame.refresh();
    await tester.pump();
    replies.last.complete({'active': false, 'maximized': true});
    await refresh;
    replies.first.complete({'active': true, 'maximized': false});
    await tester.pump();
    expect(frame.active, isFalse);
    expect(frame.maximized, isTrue);
  });

  testWidgets('frame reset invalidates a pending native read', (tester) async {
    final reply = Completer<Map<Object?, Object?>>();
    final messenger = tester.binding.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(channel, (_) => reply.future);
    addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
    final frame = WindowFrame.instance;
    frame.enable();
    await tester.pump();
    frame.debugReset();
    frame.debugEnable();
    reply.complete({'active': false, 'maximized': true});
    await tester.pump();
    expect(frame.active, isTrue);
    expect(frame.maximized, isFalse);
  });

  test('apply updates maximized, active, and caption hover', () {
    final WindowFrame frame = WindowFrame.instance;
    var notifications = 0;
    void listener() => notifications += 1;
    frame.addListener(listener);
    addTearDown(() => frame.removeListener(listener));

    frame.debugEnable();
    expect(frame.enabled, isTrue);
    expect(frame.maximized, isFalse);
    expect(frame.active, isTrue);
    expect(frame.captionHover, CaptionHover.none);

    frame.apply(
      maximized: true,
      active: false,
      captionHover: CaptionHover.close,
    );
    expect(frame.maximized, isTrue);
    expect(frame.active, isFalse);
    expect(frame.captionHover, CaptionHover.close);
    expect(notifications, greaterThan(0));

    final int after = notifications;
    frame.apply(
      maximized: true,
      active: false,
      captionHover: CaptionHover.close,
    );
    expect(notifications, after);
  });

  test('debugReset restores the disabled singleton', () {
    WindowFrame.instance.debugEnable(
      maximized: true,
      active: false,
      captionHover: CaptionHover.max,
    );
    WindowFrame.instance.debugReset();
    expect(WindowFrame.instance.enabled, isFalse);
    expect(WindowFrame.instance.maximized, isFalse);
    expect(WindowFrame.instance.active, isTrue);
    expect(WindowFrame.instance.captionHover, CaptionHover.none);
  });

  testWidgets('close hover paints the Windows red caption fill', (
    tester,
  ) async {
    WindowFrame.instance.debugEnable(captionHover: CaptionHover.close);
    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: WindowTitleBar(
          strings: AppStrings(LocalePreference.english),
          phase: ConnectionPhase.disconnected,
        ),
      ),
    );
    await tester.pump();

    final Container close = tester.widget<Container>(
      find.descendant(
        of: find.bySemanticsLabel('Close'),
        matching: find.byType(Container),
      ),
    );
    expect((close.decoration! as BoxDecoration).color, const Color(0xFFC42B1C));
  });
  testWidgets(
    'macOS caption leaves traffic lights to AppKit',
    (tester) async {
      WindowFrame.instance.debugEnable();
      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: WindowFrameScaffold(
            strings: AppStrings(LocalePreference.english),
            phase: ConnectionPhase.disconnected,
            child: const SizedBox.expand(key: ValueKey('client')),
          ),
        ),
      );
      expect(find.text('Usque'), findsOneWidget);
      expect(find.bySemanticsLabel('Close'), findsNothing);
      expect(find.bySemanticsLabel('Minimize'), findsNothing);
      expect(
        tester.getSize(find.byType(WindowTitleBar)).height,
        kWindowTitleBarHeight,
      );
      expect(
        tester.getTopLeft(find.byKey(const ValueKey('client'))).dy,
        kWindowTitleBarHeight,
      );
      expect(tester.getTopLeft(find.text('Usque')).dx, greaterThan(90));
      expect(tester.takeException(), isNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );
}
