import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/widgets/warp_dns_editor.dart';

void main() {
  testWidgets(
    'TV D-pad traverses DNS inputs without requesting the keyboard',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1280, 900);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      final before = FocusNode();
      final after = FocusNode();
      addTearDown(before.dispose);
      addTearDown(after.dispose);
      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          builder: (context, child) => MediaQuery(
            data: MediaQuery.of(
              context,
            ).copyWith(navigationMode: NavigationMode.directional),
            child: child!,
          ),
          home: Scaffold(
            body: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  TextButton(
                    focusNode: before,
                    onPressed: () {},
                    child: const Text('Before'),
                  ),
                  Form(
                    child: WarpDnsEditor(
                      value: const WarpDnsSettings(
                        mode: WarpDnsMode.doh,
                        serverName: 'dns.example',
                        dohPath: '/dns-query',
                        port: 443,
                        bootstrapIps: ['192.0.2.1'],
                      ),
                      enabled: true,
                      strings: AppStrings(LocalePreference.english),
                      onChanged: (_) {},
                    ),
                  ),
                  TextButton(
                    focusNode: after,
                    onPressed: () {},
                    child: const Text('After'),
                  ),
                ],
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      before.requestFocus();
      await tester.pump();
      tester.testTextInput.log.clear();
      for (var i = 0; i < 4; i++) {
        await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
        await tester.pumpAndSettle();
        expect(tester.testTextInput.isVisible, isFalse);
      }
      expect(after.hasFocus, isTrue);
      expect(
        tester.testTextInput.log.where(
          (call) => call.method == 'TextInput.show',
        ),
        isEmpty,
      );
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );
}
