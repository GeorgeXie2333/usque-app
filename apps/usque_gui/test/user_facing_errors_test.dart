import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/user_facing_errors.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/proxy_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/direct_dns_editor.dart';

import 'app_test.dart' show FakeEngineClient;

const _privateError =
    'decode failed: C:/Users/private/token 192.0.2.1 secret=123';

class _FailingConnection extends FakeEngineClient {
  _FailingConnection([
    this.error = const EngineException(
      'ENGINE_IPC_INVALID_RESPONSE',
      _privateError,
    ),
  ]);

  final Object error;

  @override
  Future<EngineSnapshot> retry() async => throw error;
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('credential persistence and application failures remain distinct', () {
    for (final locale in [
      LocalePreference.english,
      LocalePreference.simplifiedChinese,
    ]) {
      final strings = AppStrings(locale);
      expect(
        userFacingFailure(strings, code: 'PROXY_AUTH_APPLY_FAILED'),
        strings.get('settings_failed'),
      );
      expect(
        userFacingFailure(strings, code: 'PROXY_AUTH_SAVE_FAILED'),
        strings.get('settings_save_failed'),
      );
      expect(
        userFacingFailure(strings, code: 'PROXY_AUTH_UNSUPPORTED'),
        strings.get('settings_unsupported'),
      );
    }
  });

  test('replies and asynchronous failures never expose backend text', () async {
    for (final locale in [
      LocalePreference.english,
      LocalePreference.simplifiedChinese,
    ]) {
      final app = AppController(_FailingConnection())
        ..localePreference = locale;
      addTearDown(app.dispose);
      await app.retry();
      expect(app.lastError, app.strings.get('engine_unavailable'));
      app.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'FUTURE_PRIVATE_ERROR',
        warning: _privateError,
      );
      expect(app.lastError, app.strings.get('operation_failed'));
      expect(app.lastError, isNot(contains('192.0.2.1')));
      expect(app.lastError, isNot(contains('secret')));
      expect(app.lastError, isNot(contains('FUTURE_PRIVATE_ERROR')));
    }
  });

  test(
    'unknown failures and timeouts have translated, actionable messages',
    () {
      for (final locale in LocalePreference.values.where(
        (l) => l != LocalePreference.system,
      )) {
        final strings = AppStrings(locale);
        final unknown = userFacingError(strings, StateError(_privateError));
        expect(unknown, strings.get('operation_failed'));
        expect(unknown, isNot('operation_failed'));
        expect(unknown, isNot(contains(_privateError)));
        expect(
          userFacingError(strings, TimeoutException(_privateError)),
          strings.get('operation_timeout'),
        );
        if (locale != LocalePreference.english) {
          expect(
            unknown,
            isNot(AppStrings(LocalePreference.english).get('operation_failed')),
          );
        }
      }
    },
  );

  test('a transitional failure keeps timeout and recovery context', () async {
    for (final error in [
      TimeoutException(_privateError),
      const EngineException(
        'WINDOWS_RECOVERY_EXHAUSTED',
        'Wintun: $_privateError',
      ),
    ]) {
      final app = AppController(_FailingConnection(error))
        ..localePreference = LocalePreference.simplifiedChinese
        ..snapshot = const EngineSnapshot(phase: ConnectionPhase.preparing);
      addTearDown(app.dispose);
      await app.retry();
      expect(app.snapshot.phase, ConnectionPhase.error);
      expect(app.lastError, userFacingError(app.strings, error));
      expect(app.lastError, isNot(contains(_privateError)));
    }
  });

  test('credential errors identify the field and retain UTF-8 byte limits', () {
    expect(proxyAuthError('', ''), isNull);
    expect(proxyAuthError('', 'password'), (key: 'required', username: true));
    expect(proxyAuthError('user', ''), (key: 'required', username: false));
    expect(proxyAuthError('user:name', 'password')?.key, 'username_colon');
    expect(proxyAuthError('user\u0000name', 'password')?.key, 'username_null');
    expect(proxyAuthError('名' * 85, '密' * 85), isNull);
    expect(proxyAuthError('名' * 86, 'password'), (
      key: 'input_too_long_bytes',
      username: true,
    ));
    expect(proxyAuthError('user', '密' * 86), (
      key: 'input_too_long_bytes',
      username: false,
    ));
  });

  test(
    'DNS errors distinguish syntax, duplicate, count and prohibited addresses',
    () {
      expect(directDnsBootstrapError(''), 'required');
      expect(directDnsBootstrapError('dns.example'), 'invalid_address');
      expect(
        directDnsBootstrapError('::1 0:0:0:0:0:0:0:1'),
        'dns_duplicate_address',
      );
      for (final address in [
        '0.0.0.0',
        '224.0.0.1',
        '255.255.255.255',
        '::',
        'ff02::1',
        'fe80::1',
      ]) {
        expect(directDnsBootstrapError(address), 'dns_address_not_allowed');
      }
      expect(
        directDnsBootstrapError(
          List.generate(9, (i) => '192.0.2.${i + 1}').join('\n'),
        ),
        'nq_dns_invalid_bootstrap',
      );
      expect(directDnsBootstrapError('192.0.2.1\n2001:db8::1'), isNull);
    },
  );
}
