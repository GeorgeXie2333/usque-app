import 'package:flutter_test/flutter_test.dart';
import 'package:usque/state/window_frame.dart';

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
}
