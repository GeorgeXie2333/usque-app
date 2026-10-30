import 'dart:io';

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/country_strings.dart';
import 'package:usque/core/iso_countries.dart';
import 'package:usque/core/l10n/ads.dart';
import 'package:usque/core/l10n/catalogs.dart';
import 'package:usque/core/l10n/chain.dart';
import 'package:usque/core/l10n/countries.dart';
import 'package:usque/core/l10n/diagnostics.dart';
import 'package:usque/core/l10n/endpoint_presets.dart';
import 'package:usque/core/l10n/l4.dart';
import 'package:usque/core/l10n/linux_desktop.dart';
import 'package:usque/core/l10n/macos_desktop.dart';
import 'package:usque/core/l10n/network_quality.dart';
import 'package:usque/core/l10n/network_settings.dart';
import 'package:usque/core/l10n/onboarding.dart';
import 'package:usque/core/l10n/proton.dart';
import 'package:usque/core/l10n/scenarios.dart';
import 'package:usque/core/l10n/tls_fingerprint.dart';
import 'package:usque/core/l10n/ui_workflow.dart';
import 'package:usque/core/l10n/update_check.dart';
import 'package:usque/core/l10n/ux_refinements.dart';
import 'package:usque/core/l10n/vpngate.dart';
import 'package:usque/core/l10n/windows_recovery.dart';
import 'package:usque/core/proton_strings.dart';
import 'package:usque/core/warp_strings.dart';
import 'package:usque/models/app_models.dart';

const _tables = {
  'main': kCatalogs,
  'ads': kAdsCatalogs,
  'chain': kChainCatalogs,
  'diagnostics': kDiagnosticsCatalogs,
  'endpoint presets': kEndpointPresetCatalogs,
  'L4': kL4Catalogs,
  'Linux': kLinuxDesktopCatalogs,
  'network quality': kNetworkQualityCatalogs,
  'network settings': kNetworkSettingsCatalogs,
  'onboarding': kOnboardingCatalogs,
  'Proton': kProtonCatalogs,
  'scenarios': kScenarioCatalogs,
  'TLS fingerprint': kTlsFingerprintCatalogs,
  'workflow': kUiWorkflowCatalogs,
  'update': kUpdateCheckCatalogs,
  'UX refinements': kUxRefinementCatalogs,
  'VPN Gate': kVpnGateCatalogs,
  'Windows recovery': kWindowsRecoveryCatalogs,
};

// Compare all tokens, including ones not in the older fixed token list.
List<String> _placeholders(String text) => RegExp(
  r'\{[a-zA-Z_]+\}',
).allMatches(text).map((match) => match[0]!).toList()..sort();

AppStrings _strings(String id) {
  final parts = id.split('_');
  return AppStrings(
    LocalePreference.system,
    systemLocale: Locale(parts.first, parts.length > 1 ? parts[1] : null),
  );
}

void main() {
  test('macOS Chinese catalogs resolve every key without English fallback', () {
    expect(kMacDesktopCatalogs.keys.toSet(), {'en', 'zh_CN', 'zh_HK', 'zh_TW'});
    for (final catalog in kMacDesktopCatalogs.entries) {
      expect(catalog.value.keys.toSet(), kMacDesktopEn.keys.toSet());
      for (final entry in catalog.value.entries) {
        final label = '${catalog.key}.${entry.key}';
        expect(entry.value.trim(), isNotEmpty, reason: label);
        expect(
          _placeholders(entry.value),
          _placeholders(kMacDesktopEn[entry.key]!),
        );
        expect(
          _strings(catalog.key).get(entry.key),
          entry.value,
          reason: label,
        );
        if (catalog.key.startsWith('zh_')) {
          expect(entry.value, isNot(kMacDesktopEn[entry.key]), reason: label);
        }
      }
    }
  });

  test('all 249 country names resolve in each of the 21 locales', () {
    final codes = kIsoCountries.map((country) => country.code).toSet();
    expect(codes, hasLength(249));
    expect(kCountryCatalogs.keys.toSet(), kCatalogs.keys.toSet());
    for (final locale in kCountryCatalogs.entries) {
      expect(locale.value.keys.toSet(), codes, reason: locale.key);
      for (final country in locale.value.entries) {
        expect(country.value.trim(), isNotEmpty);
        expect(
          _strings(locale.key).countryName(country.key),
          country.value,
          reason: '${locale.key}.${country.key}',
        );
      }
    }
    expect(_strings('zh_CN').countryName(' us '), '美国');
    expect(_strings('zh_HK').countryName('NZ'), '新西蘭');
    expect(_strings('zh_TW').countryName('NZ'), '紐西蘭');
    expect(_strings('ar').countryName('US'), 'الولايات المتحدة');
    expect(
      _strings('de').countryName('ZZ', fallback: 'Custom region'),
      'Custom region',
    );
    expect(_strings('de').countryName('ZZ'), 'ZZ');
    expect(
      _strings('de').countryName(null),
      _strings('de').get('gate_unknown_country'),
    );
  });

  for (final module in _tables.entries) {
    test('${module.key}: all 21 locales have every key and placeholder', () {
      expect(module.value.keys.toSet(), kCatalogs.keys.toSet());
      expect(module.value, hasLength(21));
      final english = module.value['en']!;
      for (final locale in module.value.entries) {
        expect(
          locale.value.keys.toSet(),
          english.keys.toSet(),
          reason: '${module.key}.${locale.key}',
        );
        for (final entry in locale.value.entries) {
          final label = '${module.key}.${locale.key}.${entry.key}';
          expect(entry.value.trim(), isNotEmpty, reason: label);
          expect(
            _placeholders(entry.value),
            _placeholders(english[entry.key]!),
            reason: label,
          );
        }
      }
    });
  }

  test(
    'Proton resolves its own locale without English or Simplified fallback',
    () {
      // These words are also spelled this way in the named target languages.
      const sharedWords = {
        'de': {'selected_server'},
        'id': {'selected_server'},
        'it': {'selected_server', 'password'},
        'nl': {'selected_server'},
      };
      for (final locale in kProtonCatalogs.entries) {
        final strings = _strings(locale.key);
        expect(strings.catalogId, locale.key);
        for (final entry in locale.value.entries) {
          final label = '${locale.key}.${entry.key}';
          expect(strings.proton(entry.key), entry.value, reason: label);
          if (locale.key != 'en' &&
              entry.key != 'title' &&
              !(sharedWords[locale.key]?.contains(entry.key) ?? false)) {
            expect(entry.value, isNot(kProtonEn[entry.key]), reason: label);
          }
        }
        expect(
          protonConnectionFailure(strings, 'PROTON_AUTHENTICATION_FAILED'),
          locale.value['connection_authentication'],
        );
        expect(
          protonGateFailure(strings, 'proton_max_sessions'),
          locale.value['connection_sessions'],
        );
      }
      expect(_strings('zh_CN').proton('automatic_hint'), '连接时选择最低延迟节点。');
      expect(_strings('zh_HK').proton('login'), '登入 Proton');
      expect(_strings('zh_TW').proton('login'), '登入 Proton');
      expect(_strings('zh_HK').proton('account'), 'Proton 帳戶');
      expect(_strings('zh_TW').proton('account'), 'Proton 帳號');
      for (final locale in kProtonCatalogs.entries) {
        expect(
          _strings(locale.key).proton('unknown_future_key'),
          locale.value['failed'],
        );
      }
    },
  );

  test('WARP and adapter cleanup copy covers all locales', () {
    const keys = [
      'generate',
      'completed',
      'cancelled',
      'failed',
      'unavailable',
      'identity_required',
      'underlay_failed',
      'registration_failed',
    ];
    expect(kWarpCatalogs.keys.toSet(), kCatalogs.keys.toSet());
    expect(kWindowsAdapterCleanupCatalogs.keys.toSet(), kCatalogs.keys.toSet());
    for (final id in kCatalogs.keys) {
      expect(kWarpCatalogs[id], hasLength(keys.length));
      expect(kWindowsAdapterCleanupCatalogs[id]!.trim(), isNotEmpty);
      for (var index = 0; index < keys.length; index++) {
        final value = kWarpCatalogs[id]![index];
        expect(value.trim(), isNotEmpty);
        expect(_strings(id).warp(keys[index]), value);
        expect(
          _placeholders(value),
          _placeholders(kWarpCatalogs['en']![index]),
        );
      }
    }
  });

  test(
    'loading, license and quality labels are translated in every locale',
    () {
      for (final id in kCatalogs.keys.where((id) => id != 'en')) {
        for (final key in [
          'app_loading',
          'warp_license_key',
          'use_license_key',
          'nq_bytes',
          'nq_stream_window',
        ]) {
          final value = _strings(id).get(key);
          expect(value, isNot(key), reason: '$id.$key');
          expect(value, isNot(_strings('en').get(key)), reason: '$id.$key');
        }
      }
      expect(
        _strings('vi').get('diag_check_transport_endpoint_pin'),
        'Ghim điểm cuối',
      );
      expect(_strings('vi').get('endpoint_ipv4'), 'Điểm cuối IPv4');
      expect(_strings('vi').get('endpoint_ipv6'), 'Điểm cuối IPv6');
    },
  );

  test(
    'Android translatable strings and format arguments cover all 21 locales',
    () {
      final root = Directory('android/app/src/main/res');
      Map<String, String> read(String directory) => {
        for (final match
            in RegExp(r'<string\b([^>]*)>([\s\S]*?)</string>').allMatches(
              File('${root.path}/$directory/strings.xml').readAsStringSync(),
            ))
          if (!match[1]!.contains('translatable="false"'))
            RegExp(r'name="([^"]+)"').firstMatch(match[1]!)![1]!: match[2]!,
      };
      final english = read('values');
      for (final id in kCatalogs.keys) {
        final directory = switch (id) {
          'en' => 'values',
          'id' => 'values-in',
          'zh_CN' => 'values-zh-rCN',
          'zh_HK' => 'values-zh-rHK',
          'zh_TW' => 'values-zh-rTW',
          _ => 'values-$id',
        };
        final localized = read(directory);
        expect(localized.keys.toSet(), english.keys.toSet(), reason: directory);
        for (final entry in localized.entries) {
          expect(
            entry.value.trim(),
            isNotEmpty,
            reason: '$directory.${entry.key}',
          );
          List<String> arguments(String text) => RegExp(
            r'%\d+\$[sd]',
          ).allMatches(text).map((match) => match[0]!).toList()..sort();
          expect(
            arguments(entry.value),
            arguments(english[entry.key]!),
            reason: '$directory.${entry.key}',
          );
        }
      }
    },
  );
}
