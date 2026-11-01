import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/semantics.dart';
import 'package:flutter/services.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/tv_input_platform.dart';
import 'package:usque/widgets/tv_input_scope.dart';
import 'package:usque/widgets/tv_text_input.dart';
import 'package:usque/widgets/unsaved_changes_guard.dart';
import 'package:usque/widgets/usque_dialog.dart';

class FakeTvPlatform extends TvInputPlatform {
  final begun = <String>[];
  final ended = <String>[];
  Completer<void>? pending;
  Completer<bool?>? environmentResult;
  bool? environment = true;
  @override
  Future<bool?> getEnvironment() async =>
      environmentResult == null ? environment : await environmentResult!.future;
  @override
  Future<void> beginEditing(String sessionId) async {
    begun.add(sessionId);
    await pending?.future;
  }

  @override
  Future<void> endEditing(String sessionId) async => ended.add(sessionId);
  @override
  void dispose() {}
}

Future<void> host(
  WidgetTester tester,
  Widget child, {
  TvInputController? controller,
}) async {
  final input = controller ?? TvInputController(isTv: true);
  addTearDown(input.dispose);
  await tester.pumpWidget(
    TvInputHost(
      controller: input,
      child: MaterialApp(
        theme: UsqueTheme.light(),
        home: Scaffold(body: child),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets(
    'native TV classification completes before autofocus is mounted',
    (tester) async {
      final platform = FakeTvPlatform()..environmentResult = Completer<bool?>();
      final input = TvInputController(platform: platform);
      addTearDown(input.dispose);
      await tester.pumpWidget(
        TvInputHost(
          controller: input,
          child: MaterialApp(
            home: Scaffold(body: tvTextField(autofocus: true)),
          ),
        ),
      );
      expect(find.byType(TextField), findsNothing);
      platform.environmentResult!.complete(true);
      await tester.pumpAndSettle();
      expect(find.byType(TextField), findsOneWidget);
      expect(tester.testTextInput.isVisible, isFalse);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  test('all production Material text inputs use the shared TV adapter', () {
    final raw = RegExp(r'\bText(?:Form)?Field\s*\(');
    final missed = Directory('lib')
        .listSync(recursive: true)
        .whereType<File>()
        .where(
          (file) =>
              file.path.endsWith('.dart') &&
              !file.path.endsWith('tv_text_input.dart'),
        )
        .where((file) => raw.hasMatch(file.readAsStringSync()))
        .map((file) => file.path)
        .toList();
    expect(missed, isEmpty);
  });

  for (final key in [
    LogicalKeyboardKey.select,
    LogicalKeyboardKey.enter,
    LogicalKeyboardKey.numpadEnter,
    LogicalKeyboardKey.gameButtonA,
  ]) {
    testWidgets(
      'explicit ${key.keyLabel} activation does not submit on repeats',
      (tester) async {
        final focus = FocusNode();
        addTearDown(focus.dispose);
        var submissions = 0;
        await host(
          tester,
          tvTextField(
            focusNode: focus,
            autofocus: true,
            onSubmitted: (_) => submissions++,
          ),
        );
        expect(focus.hasFocus, isTrue);
        expect(tester.testTextInput.isVisible, isFalse);
        await tester.sendKeyDownEvent(key);
        await tester.pumpAndSettle();
        expect(tester.testTextInput.isVisible, isTrue);
        await tester.sendKeyRepeatEvent(key);
        await tester.pump();
        await tester.sendKeyUpEvent(key);
        await tester.pump();
        expect(submissions, 0);
        await tester.testTextInput.receiveAction(TextInputAction.done);
        await tester.pumpAndSettle();
        expect(submissions, 1);
        expect(tester.testTextInput.isVisible, isFalse);
        expect(focus.hasFocus, isTrue);
        await tester.pumpWidget(const SizedBox.shrink());
      },
      variant: TargetPlatformVariant.only(TargetPlatform.android),
    );
  }

  testWidgets(
    'native Back keeps a dirty draft and ignores stale session callbacks',
    (tester) async {
      final platform = FakeTvPlatform();
      final input = TvInputController(isTv: true, platform: platform);
      final text = TextEditingController(text: 'before');
      final focus = FocusNode();
      addTearDown(text.dispose);
      addTearDown(focus.dispose);
      await host(
        tester,
        tvTextField(controller: text, focusNode: focus),
        controller: input,
      );
      await tester.tap(find.byType(TextField));
      await tester.pumpAndSettle();
      final old = platform.begun.single;
      tester.testTextInput.enterText('draft');
      await tester.pump();
      platform.onEditingEnded!(old, 'back');
      await tester.pumpAndSettle();
      expect(text.text, 'draft');
      expect(focus.hasFocus, isTrue);
      expect(tester.testTextInput.isVisible, isFalse);
      await tester.sendKeyEvent(LogicalKeyboardKey.select);
      await tester.pumpAndSettle();
      platform.onEditingEnded!(old, 'imeHidden');
      await tester.pump();
      expect(tester.testTextInput.isVisible, isTrue);
      input.didChangeAppLifecycleState(AppLifecycleState.paused);
      await tester.pumpAndSettle();
      expect(tester.testTextInput.isVisible, isFalse);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  for (final dirty in [false, true]) {
    testWidgets(
      'route Back ends editing before clean/dirty guard dirty=$dirty',
      (tester) async {
        final text = TextEditingController(text: 'draft');
        addTearDown(text.dispose);
        final input = TvInputController(isTv: true);
        await host(
          tester,
          Builder(
            builder: (context) => TextButton(
              onPressed: () => Navigator.of(context).push(
                MaterialPageRoute<void>(
                  builder: (_) => UnsavedChangesGuard(
                    strings: AppStrings(LocalePreference.english),
                    dirty: dirty,
                    saving: false,
                    child: Scaffold(body: tvTextField(controller: text)),
                  ),
                ),
              ),
              child: const Text('Open'),
            ),
          ),
          controller: input,
        );
        await tester.tap(find.text('Open'));
        await tester.pumpAndSettle();
        await tester.tap(find.byType(TextField));
        await tester.pumpAndSettle();
        final context = tester.element(find.byType(TextField));
        // Exercise the route's actual PopEntries, independently of the early
        // WidgetsBinding observer and native key interception.
        await Navigator.of(context).maybePop();
        await tester.pumpAndSettle();
        expect(find.byType(TextField), findsOneWidget);
        expect(find.byType(UsqueDialog), findsNothing);
        expect(text.text, 'draft');
        expect(tester.testTextInput.isVisible, isFalse);
        await Navigator.of(context).maybePop();
        await tester.pumpAndSettle();
        expect(find.byType(UsqueDialog), dirty ? findsOneWidget : findsNothing);
        expect(find.byType(TextField), dirty ? findsOneWidget : findsNothing);
        await tester.pumpWidget(const SizedBox.shrink());
      },
      variant: TargetPlatformVariant.only(TargetPlatform.android),
    );
  }

  testWidgets(
    'opening a dialog revokes the old page input session',
    (tester) async {
      final platform = FakeTvPlatform();
      await host(
        tester,
        Builder(
          builder: (context) => Column(
            children: [
              tvTextField(autofocus: true),
              TextButton(
                onPressed: () => showDialog<void>(
                  context: context,
                  builder: (_) =>
                      const AlertDialog(content: Text('Other dialog')),
                ),
                child: const Text('Dialog'),
              ),
            ],
          ),
        ),
        controller: TvInputController(isTv: true, platform: platform),
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.select);
      await tester.pumpAndSettle();
      await tester.tap(find.text('Dialog'));
      await tester.pumpAndSettle();
      expect(platform.ended, contains(platform.begun.single));
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.text('Other dialog'), findsNothing);
      expect(tester.testTextInput.isVisible, isFalse);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets(
    'cancelled pending activation cannot reopen a disposed editor',
    (tester) async {
      final platform = FakeTvPlatform()..pending = Completer<void>();
      final input = TvInputController(isTv: true, platform: platform);
      await host(tester, tvTextField(autofocus: true), controller: input);
      await tester.sendKeyEvent(LogicalKeyboardKey.select);
      await tester.pump();
      expect(tester.testTextInput.isVisible, isFalse);
      await tester.pumpWidget(const SizedBox.shrink());
      platform.pending!.complete();
      await tester.pumpAndSettle();
      expect(tester.testTextInput.isVisible, isFalse);
      expect(platform.ended, contains(platform.begun.single));
      expect(tester.takeException(), isNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets(
    'accessibility activation edits without exposing password contents',
    (tester) async {
      final semantics = tester.ensureSemantics();
      final password = TextEditingController(text: 'synthetic-secret');
      addTearDown(password.dispose);
      await host(
        tester,
        tvTextField(
          controller: password,
          obscureText: true,
          decoration: const InputDecoration(labelText: 'Password'),
        ),
      );
      final node = tester.getSemantics(find.byType(TextField));
      expect(node.hintOverrides?.onTapHint, 'Edit');
      expect(
        node.getSemanticsData().value,
        isNot(contains('synthetic-secret')),
      );
      tester
          .element(find.byType(TextField))
          .findRenderObject()!
          .owner!
          .semanticsOwner!
          .performAction(node.id, SemanticsAction.tap);
      await tester.pumpAndSettle();
      expect(tester.testTextInput.isVisible, isTrue);
      expect(
        tester
            .getSemantics(find.byType(TextField))
            .getSemanticsData()
            .flagsCollection
            .isReadOnly,
        isFalse,
      );
      semantics.dispose();
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  for (final target in [
    TargetPlatform.android,
    TargetPlatform.windows,
    TargetPlatform.linux,
  ]) {
    testWidgets(
      'ordinary $target field keeps immediate editing',
      (tester) async {
        await host(
          tester,
          tvTextField(),
          controller: TvInputController(isTv: false),
        );
        await tester.tap(find.byType(TextField));
        await tester.pumpAndSettle();
        expect(tester.testTextInput.isVisible, isTrue);
        expect(
          tester.widget<TextField>(find.byType(TextField)).readOnly,
          isFalse,
        );
        await tester.pumpWidget(const SizedBox.shrink());
      },
      variant: TargetPlatformVariant.only(target),
    );
  }

  testWidgets(
    'Next, numeric formatting and validation preserve the chosen focus',
    (tester) async {
      final first = FocusNode();
      final second = FocusNode();
      final value = TextEditingController();
      final form = GlobalKey<FormState>();
      addTearDown(first.dispose);
      addTearDown(second.dispose);
      addTearDown(value.dispose);
      await host(
        tester,
        Form(
          key: form,
          child: Column(
            children: [
              tvTextFormField(
                controller: value,
                focusNode: first,
                autofocus: true,
                textInputAction: TextInputAction.next,
                inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                validator: (text) => text == '42' ? null : 'Use 42',
              ),
              tvTextField(focusNode: second, minLines: 2, maxLines: 4),
            ],
          ),
        ),
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.select);
      await tester.pumpAndSettle();
      tester.testTextInput.enterText('4a');
      await tester.pump();
      expect(value.text, '4');
      await tester.testTextInput.receiveAction(TextInputAction.next);
      await tester.pumpAndSettle();
      expect(second.hasFocus, isTrue);
      expect(tester.testTextInput.isVisible, isFalse);
      expect(form.currentState!.validate(), isFalse);
      first.requestFocus();
      await tester.pumpAndSettle();
      expect(tester.testTextInput.isVisible, isFalse);
      await tester.sendKeyEvent(LogicalKeyboardKey.select);
      await tester.pumpAndSettle();
      tester.testTextInput.enterText('42');
      await tester.pump();
      expect(form.currentState!.validate(), isTrue);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets(
    'read-only fields remain navigable and disabling an editor releases it',
    (tester) async {
      var enabled = true;
      late StateSetter rebuild;
      final locked = FocusNode();
      final editable = FocusNode();
      addTearDown(locked.dispose);
      addTearDown(editable.dispose);
      await host(
        tester,
        StatefulBuilder(
          builder: (context, setState) {
            rebuild = setState;
            return Column(
              children: [
                tvTextField(focusNode: locked, readOnly: true, autofocus: true),
                tvTextField(focusNode: editable, enabled: enabled),
              ],
            );
          },
        ),
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.select);
      await tester.pumpAndSettle();
      expect(tester.testTextInput.isVisible, isFalse);
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
      await tester.pumpAndSettle();
      expect(editable.hasFocus, isTrue);
      await tester.sendKeyEvent(LogicalKeyboardKey.select);
      await tester.pumpAndSettle();
      expect(tester.testTextInput.isVisible, isTrue);
      rebuild(() => enabled = false);
      await tester.pumpAndSettle();
      expect(tester.testTextInput.isVisible, isFalse);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  for (final directional in [false, true]) {
    testWidgets(
      'missing native environment falls back only to directional Android: $directional',
      (tester) async {
        final platform = FakeTvPlatform()..environment = null;
        final input = TvInputController(platform: platform);
        addTearDown(input.dispose);
        tester.view.physicalSize = const Size(1920, 1080);
        addTearDown(tester.view.resetPhysicalSize);
        await tester.pumpWidget(
          TvInputHost(
            controller: input,
            child: MaterialApp(
              builder: (context, child) => MediaQuery(
                data: MediaQuery.of(context).copyWith(
                  navigationMode: directional
                      ? NavigationMode.directional
                      : NavigationMode.traditional,
                ),
                child: child!,
              ),
              home: Scaffold(body: tvTextField(autofocus: true)),
            ),
          ),
        );
        await tester.pumpAndSettle();
        expect(tester.testTextInput.isVisible, !directional);
        await tester.pumpWidget(const SizedBox.shrink());
      },
      variant: TargetPlatformVariant.only(TargetPlatform.android),
    );
  }

  for (final locale in [const Locale('en'), const Locale('zh', 'CN')]) {
    for (final dark in [false, true]) {
      testWidgets(
        'TV focus and edit semantics at 200 percent $locale dark=$dark',
        (tester) async {
          final input = TvInputController(isTv: true);
          final focus = FocusNode();
          final semantics = tester.ensureSemantics();
          addTearDown(input.dispose);
          addTearDown(focus.dispose);
          tester.view.physicalSize = const Size(1280, 720);
          tester.view.devicePixelRatio = 1;
          addTearDown(tester.view.resetPhysicalSize);
          addTearDown(tester.view.resetDevicePixelRatio);
          await tester.pumpWidget(
            TvInputHost(
              controller: input,
              child: MaterialApp(
                locale: locale,
                supportedLocales: [locale],
                localizationsDelegates: const [
                  GlobalMaterialLocalizations.delegate,
                  GlobalWidgetsLocalizations.delegate,
                  GlobalCupertinoLocalizations.delegate,
                ],
                theme: dark ? UsqueTheme.dark() : UsqueTheme.light(),
                builder: (context, child) => MediaQuery(
                  data: MediaQuery.of(
                    context,
                  ).copyWith(textScaler: const TextScaler.linear(2)),
                  child: child!,
                ),
                home: Scaffold(
                  body: SingleChildScrollView(
                    child: tvTextField(
                      focusNode: focus,
                      decoration: const InputDecoration(labelText: 'DNS'),
                    ),
                  ),
                ),
              ),
            ),
          );
          await tester.pumpAndSettle();
          final bounds = tester.getRect(find.byType(TextField));
          focus.requestFocus();
          await tester.pumpAndSettle();
          expect(tester.getRect(find.byType(TextField)), bounds);
          expect(tester.testTextInput.isVisible, isFalse);
          expect(
            tester
                .getSemantics(find.byType(TextField))
                .hintOverrides
                ?.onTapHint,
            AppStrings(
              LocalePreference.system,
              systemLocale: locale,
            ).get('edit'),
          );
          await tester.sendKeyEvent(LogicalKeyboardKey.select);
          await tester.pumpAndSettle();
          expect(tester.testTextInput.isVisible, isTrue);
          await tester.binding.handlePopRoute();
          await tester.pumpAndSettle();
          expect(tester.testTextInput.isVisible, isFalse);
          expect(focus.hasFocus, isTrue);
          expect(tester.takeException(), isNull);
          semantics.dispose();
          await tester.pumpWidget(const SizedBox.shrink());
        },
        variant: TargetPlatformVariant.only(TargetPlatform.android),
      );
    }
  }
}
