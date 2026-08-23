import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import '../models/app_models.dart';
import 'l10n/catalogs.dart';

class AppStrings {
  AppStrings(LocalePreference preference, {Locale? systemLocale})
    : catalogId = resolveCatalogId(
        preference,
        systemLocale ?? PlatformDispatcher.instance.locale,
      );

  final String catalogId;

  String get languageCode =>
      catalogId.startsWith('zh') ? 'zh' : catalogId.split('_').first;

  String get(String key) {
    final values = kCatalogs[catalogId] ?? kEnCatalog;
    return values[key] ?? kEnCatalog[key] ?? key;
  }

  @visibleForTesting
  static bool get debugCatalogsAreComplete {
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
