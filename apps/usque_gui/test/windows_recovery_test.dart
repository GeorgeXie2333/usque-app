import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/l10n/windows_recovery.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';

import 'app_test.dart' show FakeEngineClient;

class RecoveryErrorEngine extends FakeEngineClient {
  RecoveryErrorEngine(this.code);

  final String code;

  @override
  Future<EngineSnapshot> retry() async =>
      throw EngineException(code, 'Unlocalized technical recovery details');
}

class PendingRecoveryEngine extends FakeEngineClient {
  @override
  Future<EngineSnapshot> retry() async =>
      const EngineSnapshot(phase: ConnectionPhase.reconnecting);
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test(
    'recovery catalogs are complete and other locales fall back to English',
    () {
      expect(
        kWindowsRecoveryEn.keys.toSet(),
        kWindowsRecoveryZhCn.keys.toSet(),
      );
      expect(AppStrings.debugCatalogsAreComplete, isTrue);
      for (final code in kWindowsRecoveryEn.keys) {
        expect(
          AppStrings(LocalePreference.english).windowsRecoveryError(code),
          kWindowsRecoveryEn[code],
        );
        expect(
          AppStrings(
            LocalePreference.simplifiedChinese,
          ).windowsRecoveryError(code),
          kWindowsRecoveryZhCn[code],
        );
        expect(
          AppStrings(LocalePreference.japanese).windowsRecoveryError(code),
          kWindowsRecoveryEn[code],
        );
      }
      expect(
        AppStrings(LocalePreference.english).windowsRecoveryError('OTHER'),
        isNull,
      );
    },
  );

  test(
    'pending automatic recovery stays transitional without an error',
    () async {
      final controller = AppController(PendingRecoveryEngine());
      addTearDown(controller.dispose);
      controller.snapshot = const EngineSnapshot(phase: ConnectionPhase.error);
      await controller.retry();
      expect(controller.snapshot.phase, ConnectionPhase.reconnecting);
      expect(controller.lastError, isNull);
      expect(controller.busy, isFalse);
    },
  );

  for (final locale in [
    LocalePreference.english,
    LocalePreference.simplifiedChinese,
  ]) {
    for (final code in kWindowsRecoveryEn.keys) {
      test(
        'recovery reply and subsequent snapshot stay localized: $locale $code',
        () async {
          final controller = AppController(RecoveryErrorEngine(code))
            ..localePreference = locale;
          addTearDown(controller.dispose);
          await controller.retry();
          final expected = AppStrings(locale).windowsRecoveryError(code);
          expect(controller.lastError, expected);
          expect(controller.busy, isFalse);
          controller.snapshot = EngineSnapshot(
            phase: ConnectionPhase.error,
            errorCode: code,
            warning: 'Unlocalized snapshot error',
          );
          expect(controller.lastError, expected);
        },
      );
    }
  }
}
