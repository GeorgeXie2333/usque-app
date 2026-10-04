import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/dev/preview_app.dart';
import 'package:usque/dev/preview_engine.dart';
import 'package:usque/dev/preview_update_downloader.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/onboarding_screen.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/services/engine_client_factory.dart';

void main() {
  test('disconnect cancels a pending simulated connection', () async {
    final engine = PreviewEngine();
    addTearDown(engine.dispose);
    final pending = engine.connect(UsqueProfile.defaultProfile());
    expect((await engine.snapshot()).phase, ConnectionPhase.connectingH3);
    await engine.disconnect();
    expect((await pending).phase, ConnectionPhase.disconnected);
  });

  test('disposal stops traffic and prevents late reconnection', () async {
    final engine = PreviewEngine();
    addTearDown(engine.dispose);
    final frames = <EngineSnapshotEvent>[];
    final subscription = engine.snapshotEvents.listen(frames.add);
    engine.showPhase(ConnectionPhase.connected);
    await Future<void>.delayed(const Duration(milliseconds: 1100));
    expect((await engine.snapshot()).downloadedBytes, greaterThan(0));
    final pending = engine.connect(UsqueProfile.defaultProfile());
    engine.dispose();
    expect((await pending).isConnected, isFalse);
    final count = frames.length;
    await Future<void>.delayed(const Duration(milliseconds: 1100));
    expect(frames.length, count);
    await subscription.cancel();
  });

  test('update cleanup cannot inspect or delete files', () async {
    final engine = PreviewEngine();
    final directory = await Directory.systemTemp.createTemp(
      'usque-preview-test-',
    );
    final existing = File('${directory.path}/usque-v1.msi');
    try {
      await existing.writeAsString('sentinel');
      final downloader = PreviewUpdateDownloader(engine);
      await downloader.cleanupStale(directory: directory);
      await downloader.discard(existing.path);
      expect(await existing.readAsString(), 'sentinel');
      await expectLater(
        engine.getUpdateCacheDirectory(),
        throwsA(isA<EngineException>()),
      );
    } finally {
      engine.dispose();
      await directory.delete(recursive: true);
    }
  });

  test('Linux production entry never constructs a native engine', () {
    if (Platform.isLinux) {
      expect(createDefaultEngineClient, throwsUnsupportedError);
    }
  });

  testWidgets('preview resets accounts and preferences before onboarding', (
    tester,
  ) async {
    await tester.pumpWidget(const PreviewApp());
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    await tester.pump(const Duration(milliseconds: 100));
    expect(find.text('UI preview · simulated data · no VPN'), findsOneWidget);
    expect(find.byType(ShellScreen), findsOneWidget);
    final controller = tester
        .widget<ShellScreen>(find.byType(ShellScreen))
        .controller;
    controller.renameProfile(controller.activeProfileId, 'Changed account');
    await controller.flushProfileWrites();
    expect(controller.activeProfile.name, 'Changed account');
    final preferences = await SharedPreferences.getInstance();
    await preferences.setString('theme', 'dark');
    await tester.tap(find.text('Restart onboarding'));
    for (var frame = 0; frame < 5; frame++) {
      await tester.pump(const Duration(milliseconds: 100));
    }
    expect(find.byType(OnboardingScreen), findsOneWidget);
    expect(preferences.getString('theme'), isNull);
    await tester.tap(find.text('Reset preview'));
    for (var frame = 0; frame < 5; frame++) {
      await tester.pump(const Duration(milliseconds: 100));
    }
    expect(find.byType(ShellScreen), findsOneWidget);
    expect(
      tester
          .widget<ShellScreen>(find.byType(ShellScreen))
          .controller
          .activeProfile
          .name,
      'Preview account',
    );
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump(const Duration(seconds: 1));
    expect(tester.takeException(), isNull);
  });

  testWidgets('preview controls fit a narrow window at large text', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(360, 800);
    tester.view.devicePixelRatio = 1;
    tester.platformDispatcher.textScaleFactorTestValue = 2;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
    await tester.pumpWidget(const PreviewApp());
    await tester.pump(const Duration(milliseconds: 100));
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('preview toolbar follows the app language and RTL direction', (
    tester,
  ) async {
    await tester.pumpWidget(const PreviewApp());
    for (var frame = 0; frame < 5; frame++) {
      await tester.pump(const Duration(milliseconds: 100));
    }
    final controller = tester
        .widget<ShellScreen>(find.byType(ShellScreen))
        .controller;
    for (final locale in [
      LocalePreference.simplifiedChinese,
      LocalePreference.arabic,
    ]) {
      await controller.setLocale(locale);
      await tester.pumpAndSettle();
      final strings = AppStrings(locale);
      expect(find.text(strings.get('preview_banner')), findsOneWidget);
      expect(find.text(strings.get('preview_reset')), findsOneWidget);
      expect(
        find.text(strings.get('preview_restart_onboarding')),
        findsOneWidget,
      );
      expect(find.text('UI preview · simulated data · no VPN'), findsNothing);
      expect(
        Directionality.of(
          tester.element(find.byKey(const ValueKey('preview-reset'))),
        ),
        locale == LocalePreference.arabic
            ? TextDirection.rtl
            : TextDirection.ltr,
      );
    }
    await tester.tap(find.byKey(const ValueKey('preview-restart-onboarding')));
    for (var frame = 0; frame < 5; frame++) {
      await tester.pump(const Duration(milliseconds: 100));
    }
    expect(find.byType(OnboardingScreen), findsOneWidget);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump(const Duration(seconds: 1));
    expect(tester.takeException(), isNull);
  });
}
