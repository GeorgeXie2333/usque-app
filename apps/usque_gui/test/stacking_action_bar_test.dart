import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/widgets/stacking_action_bar.dart';

Widget _bar({
  required double width,
  required List<double> children,
  bool leading = true,
  TextDirection direction = TextDirection.ltr,
}) => Directionality(
  textDirection: direction,
  child: Align(
    alignment: Alignment.topLeft,
    child: SizedBox(
      width: width,
      child: StackingActionBar(
        leading: leading,
        children: <Widget>[
          for (final (index, childWidth) in children.indexed)
            SizedBox(key: ValueKey<int>(index), width: childWidth, height: 48),
        ],
      ),
    ),
  ),
);

Rect _rect(WidgetTester tester, int index) =>
    tester.getRect(find.byKey(ValueKey<int>(index)));

bool _stacked(WidgetTester tester) => tester
    .renderObject<RenderStackingActionBar>(find.byType(StackingActionBar))
    .stacked;

void main() {
  testWidgets('a row keeps Back at the start and the primary at the end', (
    tester,
  ) async {
    await tester.pumpWidget(_bar(width: 400, children: [100, 160]));
    expect(_stacked(tester), isFalse);
    expect(_rect(tester, 0), const Rect.fromLTWH(0, 0, 100, 48));
    expect(_rect(tester, 1), const Rect.fromLTWH(240, 0, 160, 48));
    expect(tester.getSize(find.byType(StackingActionBar)), const Size(400, 48));
  });

  testWidgets('a right-to-left row mirrors the edges', (tester) async {
    await tester.pumpWidget(
      _bar(width: 400, children: [100, 160], direction: TextDirection.rtl),
    );
    expect(_rect(tester, 0), const Rect.fromLTWH(300, 0, 100, 48));
    expect(_rect(tester, 1), const Rect.fromLTWH(0, 0, 160, 48));
  });

  testWidgets('without Back the actions share the end edge', (tester) async {
    await tester.pumpWidget(
      _bar(width: 400, children: [160, 80], leading: false),
    );
    expect(_rect(tester, 0), const Rect.fromLTWH(148, 0, 160, 48));
    expect(_rect(tester, 1), const Rect.fromLTWH(320, 0, 80, 48));
  });

  testWidgets('three actions spread across the row like OverflowBar', (
    tester,
  ) async {
    await tester.pumpWidget(_bar(width: 400, children: [80, 120, 80]));
    expect(_rect(tester, 0).left, 0);
    expect(_rect(tester, 1).left, 140);
    expect(_rect(tester, 2).right, 400);
  });

  testWidgets(
    'actions that do not fit stack at full width, primary first and Back last',
    (tester) async {
      await tester.pumpWidget(_bar(width: 300, children: [100, 120, 90]));
      expect(_stacked(tester), isTrue);
      expect(_rect(tester, 1), const Rect.fromLTWH(0, 0, 300, 48));
      expect(_rect(tester, 2), const Rect.fromLTWH(0, 60, 300, 48));
      expect(_rect(tester, 0), const Rect.fromLTWH(0, 120, 300, 48));
      expect(
        tester.getSize(find.byType(StackingActionBar)),
        const Size(300, 168),
      );
    },
  );

  testWidgets('the arrangement follows the available width', (tester) async {
    await tester.pumpWidget(_bar(width: 272, children: [100, 160]));
    expect(_stacked(tester), isFalse, reason: 'exactly fits with the gap');
    await tester.pumpWidget(_bar(width: 271, children: [100, 160]));
    expect(_stacked(tester), isTrue);
    await tester.pumpWidget(_bar(width: 400, children: [100, 160]));
    expect(_stacked(tester), isFalse);
    expect(tester.takeException(), isNull);
  });
}
