import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import '../models/app_models.dart';
import 'l10n/catalogs.dart';
import 'l10n/l4.dart';
import 'l10n/network_quality.dart';
import 'l10n/network_settings.dart';
import 'l10n/ui_workflow.dart';
import 'l10n/windows_recovery.dart';

class AppStrings {
  AppStrings(LocalePreference preference, {Locale? systemLocale})
    : catalogId = resolveCatalogId(
        preference,
        systemLocale ?? PlatformDispatcher.instance.locale,
      );

  final String catalogId;

  String? windowsRecoveryError(String? code, {String? details}) {
    final message =
        (kWindowsRecoveryCatalogs[catalogId] ?? kWindowsRecoveryEn)[code];
    if (message == null || !(details?.contains('Wintun') ?? false)) {
      return message;
    }
    // Show only localized step context, never raw Agent diagnostics or paths.
    final adapter =
        kWindowsAdapterCleanupCatalogs[catalogId] ?? kWindowsAdapterCleanupEn;
    return '$message\n$adapter';
  }

  String get languageCode =>
      catalogId.startsWith('zh') ? 'zh' : catalogId.split('_').first;

  String get(String key) {
    final l4 = kL4Catalogs[catalogId] ?? kL4En;
    if (l4.containsKey(key)) return l4[key]!;
    final settings = kNetworkSettingsCatalogs[catalogId] ?? kNetworkSettingsEn;
    if (settings.containsKey(key)) return settings[key]!;
    final workflow = kUiWorkflowCatalogs[catalogId] ?? kUiWorkflowEn;
    if (workflow.containsKey(key)) return workflow[key]!;
    final quality = kNetworkQualityCatalogs[catalogId] ?? kNetworkQualityEn;
    if (quality.containsKey(key)) return quality[key]!;
    final values = kCatalogs[catalogId] ?? kEnCatalog;
    return values[key] ?? kEnCatalog[key] ?? key;
  }

  String tunnelOutputLabel(TargetPlatform platform) =>
      platform == TargetPlatform.android
      ? get('vpn_mode')
      : get('tunnel_output');

  /// Feature-table keys whose English value may be reused (protocol/product).
  @visibleForTesting
  static const Set<String> kFeatureEnglishAllowlist = <String>{
    'nq_doh',
    'nq_dot',
    'nq_bytes',
    'nq_stream_window',
    'home_kill_switch',
  };

  @visibleForTesting
  static bool get debugCatalogsAreComplete {
    if (!_featureTablesComplete(kUiWorkflowCatalogs, kUiWorkflowEn) ||
        !_featureTablesComplete(kWindowsRecoveryCatalogs, kWindowsRecoveryEn) ||
        !_featureTablesComplete(kNetworkQualityCatalogs, kNetworkQualityEn) ||
        !_featureTablesComplete(kL4Catalogs, kL4En) ||
        !_featureTablesComplete(kNetworkSettingsCatalogs, kNetworkSettingsEn)) {
      return false;
    }
    if (!setEquals(
          kWindowsAdapterCleanupCatalogs.keys.toSet(),
          kCatalogs.keys.toSet(),
        ) ||
        kWindowsAdapterCleanupCatalogs.values.any(
          (value) => value.trim().isEmpty,
        )) {
      return false;
    }
    final englishKeys = kEnCatalog.keys.toSet();
    if (englishKeys.isEmpty || kCatalogs.isEmpty) {
      return false;
    }
    for (final catalog in kCatalogs.values) {
      if (!setEquals(catalog.keys.toSet(), englishKeys)) {
        return false;
      }
      if (catalog.values.any((value) => value.trim().isEmpty)) {
        return false;
      }
    }
    return true;
  }

  /// Catalog entries whose value still matches English for [keys].
  ///
  /// Returns `catalogId.key` labels so a failure names the leftover.
  @visibleForTesting
  static List<String> debugUntranslatedKeys(Iterable<String> keys) {
    final leftovers = <String>[];
    for (final catalogEntry in kCatalogs.entries) {
      if (catalogEntry.key == 'en') {
        continue;
      }
      for (final key in keys) {
        final english = kEnCatalog[key];
        final value = catalogEntry.value[key];
        if (english == null || value == null) {
          continue;
        }
        if (value == english) {
          leftovers.add('${catalogEntry.key}.$key');
        }
      }
    }
    leftovers.sort();
    return leftovers;
  }

  /// Feature-table entries whose value still matches English.
  ///
  /// Returns `catalogId.key` labels, plus `catalogId.adapter_cleanup`.
  @visibleForTesting
  static List<String> debugUntranslatedFeatureKeys() {
    final leftovers = <String>[];
    void scan(
      Map<String, Map<String, String>> tables,
      Map<String, String> english,
    ) {
      for (final catalogEntry in tables.entries) {
        if (catalogEntry.key == 'en') {
          continue;
        }
        for (final key in english.keys) {
          if (kFeatureEnglishAllowlist.contains(key)) {
            continue;
          }
          final value = catalogEntry.value[key];
          final expected = english[key];
          if (expected == null || value == null) {
            continue;
          }
          if (value == expected) {
            leftovers.add('${catalogEntry.key}.$key');
          }
        }
      }
    }

    scan(kUiWorkflowCatalogs, kUiWorkflowEn);
    scan(kNetworkQualityCatalogs, kNetworkQualityEn);
    scan(kWindowsRecoveryCatalogs, kWindowsRecoveryEn);
    scan(kL4Catalogs, kL4En);
    scan(kNetworkSettingsCatalogs, kNetworkSettingsEn);
    for (final catalogEntry in kWindowsAdapterCleanupCatalogs.entries) {
      if (catalogEntry.key == 'en') {
        continue;
      }
      if (catalogEntry.value == kWindowsAdapterCleanupEn) {
        leftovers.add('${catalogEntry.key}.adapter_cleanup');
      }
    }
    leftovers.sort();
    return leftovers;
  }

  @visibleForTesting
  static bool get debugPlaceholdersArePreserved {
    if (!_placeholdersPreserved(kEnCatalog, kCatalogs.values) ||
        !_placeholdersPreserved(kUiWorkflowEn, kUiWorkflowCatalogs.values) ||
        !_placeholdersPreserved(
          kNetworkQualityEn,
          kNetworkQualityCatalogs.values,
        ) ||
        !_placeholdersPreserved(
          kWindowsRecoveryEn,
          kWindowsRecoveryCatalogs.values,
        ) ||
        !_placeholdersPreserved(kL4En, kL4Catalogs.values) ||
        !_placeholdersPreserved(
          kNetworkSettingsEn,
          kNetworkSettingsCatalogs.values,
        )) {
      return false;
    }
    return true;
  }

  static bool _featureTablesComplete(
    Map<String, Map<String, String>> tables,
    Map<String, String> english,
  ) {
    if (!setEquals(tables.keys.toSet(), kCatalogs.keys.toSet())) {
      return false;
    }
    final englishKeys = english.keys.toSet();
    if (englishKeys.isEmpty) {
      return false;
    }
    for (final table in tables.values) {
      if (!setEquals(table.keys.toSet(), englishKeys)) {
        return false;
      }
      if (table.values.any((value) => value.trim().isEmpty)) {
        return false;
      }
    }
    return true;
  }

  static bool _placeholdersPreserved(
    Map<String, String> english,
    Iterable<Map<String, String>> catalogs,
  ) {
    for (final key in english.keys) {
      final required = kPlaceholderTokens
          .where(english[key]!.contains)
          .toList(growable: false);
      if (required.isEmpty) {
        continue;
      }
      for (final catalog in catalogs) {
        final value = catalog[key] ?? '';
        if (required.any((token) => !value.contains(token))) {
          return false;
        }
      }
    }
    return true;
  }

  @visibleForTesting
  static String resolveCatalogId(LocalePreference preference, Locale locale) {
    if (preference != LocalePreference.system) {
      return _catalogIdForPreference(preference);
    }
    return _catalogIdForSystemLocale(locale);
  }

  static String _catalogIdForPreference(LocalePreference preference) {
    return switch (preference) {
      LocalePreference.system => 'en',
      LocalePreference.english => 'en',
      LocalePreference.simplifiedChinese => 'zh_CN',
      LocalePreference.traditionalChineseHongKong => 'zh_HK',
      LocalePreference.traditionalChineseTaiwan => 'zh_TW',
      LocalePreference.japanese => 'ja',
      LocalePreference.korean => 'ko',
      LocalePreference.spanish => 'es',
      LocalePreference.portuguese => 'pt',
      LocalePreference.french => 'fr',
      LocalePreference.dutch => 'nl',
      LocalePreference.turkish => 'tr',
      LocalePreference.russian => 'ru',
      LocalePreference.persian => 'fa',
      LocalePreference.arabic => 'ar',
      LocalePreference.german => 'de',
      LocalePreference.indonesian => 'id',
      LocalePreference.italian => 'it',
      LocalePreference.polish => 'pl',
      LocalePreference.thai => 'th',
      LocalePreference.ukrainian => 'uk',
      LocalePreference.vietnamese => 'vi',
    };
  }

  static String _catalogIdForSystemLocale(Locale locale) {
    final language = locale.languageCode.toLowerCase();
    final country = (locale.countryCode ?? '').toUpperCase();
    final script = (locale.scriptCode ?? '').toLowerCase();
    if (language == 'zh') {
      if (country == 'HK' || country == 'MO') {
        return 'zh_HK';
      }
      if (country == 'TW') {
        return 'zh_TW';
      }
      if (script == 'hant') {
        return 'zh_TW';
      }
      return 'zh_CN';
    }
    return switch (language) {
      'ja' => 'ja',
      'ko' => 'ko',
      'es' => 'es',
      'pt' => 'pt',
      'fr' => 'fr',
      'nl' => 'nl',
      'tr' => 'tr',
      'ru' => 'ru',
      'fa' => 'fa',
      'ar' => 'ar',
      'de' => 'de',
      'id' => 'id',
      'in' => 'id',
      'it' => 'it',
      'pl' => 'pl',
      'th' => 'th',
      'uk' => 'uk',
      'vi' => 'vi',
      _ => 'en',
    };
  }
}
