import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/widgets/usque_logo.dart';

void main() {
  testWidgets('logo follows explicit and system themes without changing size', (
    tester,
  ) async {
    tester.platformDispatcher.platformBrightnessTestValue = Brightness.light;
    addTearDown(tester.platformDispatcher.clearPlatformBrightnessTestValue);

    Future<void> host(ThemeMode mode) async {
      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          darkTheme: UsqueTheme.dark(),
          themeMode: mode,
          home: const Scaffold(body: UsqueLogo(size: 17)),
        ),
      );
      await tester.pumpAndSettle();
    }

    void expectAsset(String asset) {
      final image = tester.widget<Image>(find.byType(Image));
      expect((image.image as AssetImage).assetName, asset);
      expect(tester.getSize(find.byType(Image)), const Size(17, 17));
      expect(image.excludeFromSemantics, isTrue);
    }

    await host(ThemeMode.light);
    expectAsset(UsqueLogo.lightAsset);
    await host(ThemeMode.dark);
    expectAsset(UsqueLogo.darkAsset);
    await host(ThemeMode.system);
    expectAsset(UsqueLogo.lightAsset);
    tester.platformDispatcher.platformBrightnessTestValue = Brightness.dark;
    await tester.pumpAndSettle();
    expectAsset(UsqueLogo.darkAsset);
  });

  testWidgets('standalone logo supplies exactly one accessible label', (
    tester,
  ) async {
    final semantics = tester.ensureSemantics();
    try {
      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: const UsqueLogo(size: 40, semanticLabel: 'Usque'),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.bySemanticsLabel('Usque'), findsOneWidget);
    } finally {
      semantics.dispose();
    }
  });
}
