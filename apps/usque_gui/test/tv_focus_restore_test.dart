import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/widgets/tv_input_scope.dart';
import 'package:usque/widgets/tv_text_input.dart';

import 'tv_text_input_test.dart' show FakeTvPlatform, host;

void main() {
  testWidgets(
    'directional keys keep their text-editing behavior while editing',
    (tester) async {
      final field = FocusNode();
      final button = FocusNode();
      final text = TextEditingController();
      addTearDown(field.dispose);
      addTearDown(button.dispose);
      addTearDown(text.dispose);
      await host(
        tester,
        Column(
          children: [
            tvTextField(focusNode: field, controller: text, maxLines: 3),
            TextButton(
              focusNode: button,
              onPressed: () {},
              child: const Text('Next'),
            ),
          ],
        ),
      );
      await tester.tap(find.byType(TextField));
      await tester.pumpAndSettle();
      tester.testTextInput.enterText('one\ntwo');
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowUp);
      await tester.pump();
      expect(field.hasFocus, isTrue);
      expect(button.hasFocus, isFalse);
      expect(text.text, 'one\ntwo');
      expect(text.selection.baseOffset, lessThan(text.text.length));
      expect(tester.testTextInput.isVisible, isTrue);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  for (final delayedAcknowledgement in [false, true]) {
    testWidgets(
      'old Back restoration cannot steal a newer editor: pending=$delayedAcknowledgement',
      (tester) async {
        final platform = FakeTvPlatform();
        final input = TvInputController(isTv: true, platform: platform);
        final first = FocusNode();
        final second = FocusNode();
        final draft = TextEditingController(text: 'First draft');
        addTearDown(first.dispose);
        addTearDown(second.dispose);
        addTearDown(draft.dispose);
        await host(
          tester,
          Column(
            children: [
              tvTextField(controller: draft, focusNode: first),
              tvTextField(focusNode: second),
            ],
          ),
          controller: input,
        );
        await tester.tap(find.byType(TextField).first);
        await tester.pumpAndSettle();
        final oldSession = platform.begun.single;
        platform.onEditingEnded!(oldSession, 'back');
        if (delayedAcknowledgement) platform.pending = Completer<void>();
        // Do not pump: the second activation precedes the old restore callback.
        await tester.tap(find.byType(TextField).last);
        await tester.pumpAndSettle();
        final focusBeforeAcknowledgement = second.hasFocus;
        if (delayedAcknowledgement) {
          platform.pending!.complete();
          await tester.pumpAndSettle();
        }
        expect(focusBeforeAcknowledgement, isTrue);
        expect(second.hasFocus, isTrue);
        expect(first.hasFocus, isFalse);
        expect(tester.testTextInput.isVisible, isTrue);
        expect(draft.text, 'First draft');
        expect(platform.begun, hasLength(2));
        expect(platform.ended, isNot(contains(platform.begun.last)));
        await tester.pumpWidget(const SizedBox.shrink());
      },
      variant: TargetPlatformVariant.only(TargetPlatform.android),
    );
  }

  testWidgets(
    'Back followed by D-pad before a rebuild preserves the new focus choice',
    (tester) async {
      final platform = FakeTvPlatform();
      final input = TvInputController(isTv: true, platform: platform);
      final field = FocusNode();
      final button = FocusNode();
      addTearDown(field.dispose);
      addTearDown(button.dispose);
      await host(
        tester,
        Column(
          children: [
            tvTextField(focusNode: field),
            TextButton(
              focusNode: button,
              onPressed: () {},
              child: const Text('Next'),
            ),
          ],
        ),
        controller: input,
      );
      await tester.tap(find.byType(TextField));
      await tester.pumpAndSettle();
      platform.onEditingEnded!(platform.begun.single, 'back');
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
      await tester.pumpAndSettle();
      expect(button.hasFocus, isTrue);
      expect(field.hasFocus, isFalse);
      expect(tester.testTextInput.isVisible, isFalse);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets(
    'a pending non-editor focus request wins over restoration in the same frame',
    (tester) async {
      final platform = FakeTvPlatform();
      final input = TvInputController(isTv: true, platform: platform);
      final field = FocusNode();
      final button = FocusNode();
      addTearDown(field.dispose);
      addTearDown(button.dispose);
      await host(
        tester,
        Column(
          children: [
            tvTextField(focusNode: field),
            TextButton(
              focusNode: button,
              onPressed: () {},
              child: const Text('Next'),
            ),
          ],
        ),
        controller: input,
      );
      await tester.tap(find.byType(TextField));
      await tester.pumpAndSettle();
      // A sibling can request focus during the frame, before the old restore.
      WidgetsBinding.instance.addPostFrameCallback(
        (_) => button.requestFocus(),
      );
      platform.onEditingEnded!(platform.begun.single, 'back');
      await tester.pumpAndSettle();
      expect(button.hasFocus, isTrue);
      expect(field.hasFocus, isFalse);
      expect(platform.begun, hasLength(1));
      expect(tester.testTextInput.isVisible, isFalse);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );
}
