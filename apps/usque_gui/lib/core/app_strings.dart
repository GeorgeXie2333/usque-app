import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import '../models/app_models.dart';
import 'l10n/catalogs.dart';
import 'l10n/network_quality.dart';
import 'l10n/ui_workflow.dart';
import 'l10n/windows_recovery.dart';

class AppStrings {
  AppStrings(LocalePreference preference, {Locale? systemLocale})
    : catalogId = resolveCatalogId(
        preference,
        systemLocale ?? PlatformDispatcher.instance.locale,
      );

  final String catalogId;

  String? windowsRecoveryError(String? code) =>
      (catalogId == 'zh_CN' ? kWindowsRecoveryZhCn : kWindowsRecoveryEn)[code];

  String get languageCode =>
      catalogId.startsWith('zh') ? 'zh' : catalogId.split('_').first;

  String get(String key) {
    final workflow = catalogId == 'zh_CN' ? kUiWorkflowZhCn : kUiWorkflowEn;
    if (workflow.containsKey(key)) return workflow[key]!;
    final quality = catalogId == 'zh_CN'
        ? kNetworkQualityZhCn
        : kNetworkQualityEn;
    if (quality.containsKey(key)) return quality[key]!;
    final values = kCatalogs[catalogId] ?? kEnCatalog;
    return values[key] ?? kEnCatalog[key] ?? key;
  }

  String tunnelOutputLabel(TargetPlatform platform) =>
      platform == TargetPlatform.android
      ? get('vpn_mode')
      : get('tunnel_output');

  @visibleForTesting
  static bool get debugCatalogsAreComplete {
    if (!setEquals(kUiWorkflowEn.keys.toSet(), kUiWorkflowZhCn.keys.toSet()) ||
        kUiWorkflowEn.values.any((value) => value.trim().isEmpty) ||
        kUiWorkflowZhCn.values.any((value) => value.trim().isEmpty)) {
      return false;
    }
    if (!setEquals(
          kWindowsRecoveryEn.keys.toSet(),
          kWindowsRecoveryZhCn.keys.toSet(),
        ) ||
        kWindowsRecoveryEn.values.any((value) => value.trim().isEmpty) ||
        kWindowsRecoveryZhCn.values.any((value) => value.trim().isEmpty)) {
      return false;
    }
    if (!setEquals(
          kNetworkQualityEn.keys.toSet(),
          kNetworkQualityZhCn.keys.toSet(),
        ) ||
        kNetworkQualityEn.values.any((value) => value.trim().isEmpty) ||
        kNetworkQualityZhCn.values.any((value) => value.trim().isEmpty)) {
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

  @visibleForTesting
  static bool get debugPlaceholdersArePreserved {
    for (final key in kEnCatalog.keys) {
      final english = kEnCatalog[key]!;
      final required = kPlaceholderTokens
          .where(english.contains)
          .toList(growable: false);
      if (required.isEmpty) {
        continue;
      }
      for (final catalog in kCatalogs.values) {
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
