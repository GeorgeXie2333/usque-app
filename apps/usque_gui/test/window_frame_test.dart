import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/state/window_frame.dart';
import 'package:usque/widgets/window_titlebar.dart';

void main() {
  tearDown(WindowFrame.instance.debugReset);

  test('apply updates maximized, active, and caption hover', () {
    final WindowFrame frame = WindowFrame.instance;
    var notifications = 0;
    frame.addListener(() => notifications += 1);

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
}
