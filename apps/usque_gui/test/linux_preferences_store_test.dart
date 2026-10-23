import 'dart:async';
import 'dart:convert';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences_platform_interface/types.dart';
import 'package:usque/services/linux_preferences_store.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('io.github.georgexie2333.usque/engine');
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  tearDown(() => messenger.setMockMethodCallHandler(channel, null));

  test(
    'blocked native storage leaves timers running and preserves unknown keys',
    () async {
      final release = Completer<void>();
      final reading = Completer<void>();
      var raw = '{"flutter.onboarding_complete":true,"foreign":{"keep":1}}';
      var writes = 0;
      messenger.setMockMethodCallHandler(channel, (call) async {
        if (call.method == 'readLinuxPreferences') {
          reading.complete();
          await release.future;
          return raw;
        }
        writes++;
        raw = call.arguments as String;
        return true;
      });
      final store = LinuxPreferencesStore();
      var ticks = 0;
      final timer = Timer.periodic(
        const Duration(milliseconds: 10),
        (_) => ticks++,
      );
      try {
        final load = store.getAll();
        await reading.future;
        await Future<void>.delayed(const Duration(milliseconds: 100));
        expect(ticks, greaterThan(1));
        release.complete();
        expect(await load, {'flutter.onboarding_complete': true});
        expect(await store.remove('flutter.profiles_v1'), true);
        expect(writes, 0);
        await store.setValue('String', 'flutter.theme', 'dark');
        await store.clear();
        expect(jsonDecode(raw), {
          'foreign': {'keep': 1},
        });
        expect(writes, 2);
      } finally {
        timer.cancel();
      }
    },
  );

  test(
    'failed saves do not change storage cache and later writes remain ordered',
    () async {
      var fail = true;
      var raw = '{"flutter.theme":"light"}';
      messenger.setMockMethodCallHandler(channel, (call) async {
        if (call.method == 'readLinuxPreferences') return raw;
        if (fail) return false;
        raw = call.arguments as String;
        return true;
      });
      final store = LinuxPreferencesStore();
      expect(await store.setValue('String', 'flutter.theme', 'dark'), false);
      expect((await store.getAll())['flutter.theme'], 'light');
      fail = false;
      final a = store.setValue('String', 'flutter.theme', 'dark');
      final b = store.clear();
      final c = store.setValue('String', 'flutter.locale', 'english');
      await Future.wait([a, b, c]);
      expect(jsonDecode(raw), {'flutter.locale': 'english'});
    },
  );

  test(
    'invalid stored JSON fails without overwrite and can be retried',
    () async {
      var raw = '{broken';
      var writes = 0;
      messenger.setMockMethodCallHandler(channel, (call) async {
        if (call.method == 'readLinuxPreferences') return raw;
        writes++;
        return true;
      });
      final store = LinuxPreferencesStore();
      await expectLater(store.getAll(), throwsFormatException);
      expect(writes, 0);
      raw = '{"flutter.theme":"dark","flutter.locale":"english"}';
      expect(
        await store.getAllWithParameters(
          GetAllParameters(
            filter: PreferencesFilter(
              prefix: 'flutter.',
              allowList: {'flutter.theme'},
            ),
          ),
        ),
        {'flutter.theme': 'dark'},
      );
    },
  );
}
