import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import '../models/app_models.dart';
import '../models/endpoint_subscriptions.dart';
import 'l10n/ads.dart';
import 'l10n/catalogs.dart';
import 'l10n/chain.dart';
import 'l10n/diagnostics.dart';
import 'l10n/endpoint_presets.dart';
import 'l10n/endpoint_subscriptions.dart';
import 'l10n/l4.dart';
import 'l10n/linux_desktop.dart';
import 'l10n/macos_desktop.dart';
import 'l10n/network_quality.dart';
import 'l10n/network_settings.dart';
import 'l10n/onboarding.dart';
import 'l10n/proton.dart';
import 'l10n/scenarios.dart';
import 'l10n/tls_fingerprint.dart';
import 'l10n/ui_workflow.dart';
import 'l10n/update_check.dart';
import 'l10n/ux_refinements.dart';
import 'l10n/vpngate.dart';
import 'l10n/windows_recovery.dart';

class AppStrings {
  AppStrings(LocalePreference preference, {Locale? systemLocale})
    : catalogId = resolveCatalogId(
        preference,
        systemLocale ?? PlatformDispatcher.instance.locale,
      );

  final String catalogId;

  /// Use the same resolved language for Flutter's built-in controls and our
  /// catalogs. In particular, zh-Hant and zh-MO must not fall back to zh-CN.
  Locale get locale {
    if (catalogId == 'pt') return const Locale('pt', 'BR');
    final parts = catalogId.split('_');
    return Locale(parts.first, parts.length > 1 ? parts[1] : null);
  }

  /// Localizes only the original default account's unchanged display name.
  /// Stored names and user-created accounts are kept verbatim.
  String accountName(UsqueProfile account) =>
      account.id == UsqueProfile.defaultProfileId && account.name == 'Default'
      ? get('account_default_name')
      : account.name;

  String get vpnGateUnsupported =>
      get('l4_unsupported').replaceAll('L4', 'VPN Gate');

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

  String get(String key, {TargetPlatform? platform}) {
    final text = _raw(key);
    return text.contains('{tunnel}')
        ? text.replaceAll(
            '{tunnel}',
            tunnelOutputLabel(platform ?? defaultTargetPlatform),
          )
        : text;
  }

  String _raw(String key) {
    final mac = kMacDesktopCatalogs[catalogId] ?? kMacDesktopEn;
    if (mac.containsKey(key)) return mac[key]!;
    final scenes = kScenarioCatalogs[catalogId] ?? kScenarioEn;
    if (scenes.containsKey(key)) return scenes[key]!;
    final presets =
        kEndpointPresetCatalogs[catalogId] ?? kEndpointPresetCatalogs['en']!;
    if (presets.containsKey(key)) return presets[key]!;
    final linux =
        kLinuxDesktopCatalogs[catalogId] ?? kLinuxDesktopCatalogs['en']!;
    if (linux.containsKey(key)) return linux[key]!;
    final tls =
        kTlsFingerprintCatalogs[catalogId] ?? kTlsFingerprintCatalogs['en']!;
    if (tls.containsKey(key)) return tls[key]!;
    final refinements =
        kUxRefinementCatalogs[catalogId] ?? kUxRefinementCatalogs['en']!;
    if (refinements.containsKey(key)) return refinements[key]!;
    final ads = kAdsCatalogs[catalogId] ?? kAdsCatalogs['en']!;
    if (ads.containsKey(key)) return ads[key]!;
    final updates =
        kUpdateCheckCatalogs[catalogId] ?? kUpdateCheckCatalogs['en']!;
    if (updates.containsKey(key)) return updates[key]!;
    final onboarding = kOnboardingCatalogs[catalogId] ?? kOnboardingEn;
    if (onboarding.containsKey(key)) return onboarding[key]!;
    final diagnostics =
        kDiagnosticsCatalogs[catalogId] ?? kDiagnosticsCatalogs['en']!;
    if (diagnostics.containsKey(key)) return diagnostics[key]!;
    final gate = kVpnGateCatalogs[catalogId] ?? kVpnGateEn;
    if (gate.containsKey(key)) return gate[key]!;
    final l4 = kL4Catalogs[catalogId] ?? kL4En;
    if (l4.containsKey(key)) return l4[key]!;
    final settings = kNetworkSettingsCatalogs[catalogId] ?? kNetworkSettingsEn;
    if (settings.containsKey(key)) return settings[key]!;
    final workflow = kUiWorkflowCatalogs[catalogId] ?? kUiWorkflowEn;
    if (workflow.containsKey(key)) return workflow[key]!;
    final quality = kNetworkQualityCatalogs[catalogId] ?? kNetworkQualityEn;
    if (quality.containsKey(key)) return quality[key]!;
    final subs =
        kEndpointSubscriptionCatalogs[catalogId] ??
        kEndpointSubscriptionCatalogs['en']!;
    if (subs.containsKey(key)) return subs[key]!;
    final values = kCatalogs[catalogId] ?? kEnCatalog;
    return values[key] ?? kEnCatalog[key] ?? key;
  }

  String tunnelOutputLabel(TargetPlatform platform) =>
      platform == TargetPlatform.android
      ? _raw('vpn_mode')
      : _raw('tunnel_output');

  String subscriptionTierLabel(EndpointSubscriptionTier tier) => switch (tier) {
    EndpointSubscriptionTier.free => get('sub_tier_free'),
    EndpointSubscriptionTier.plus => get('sub_tier_plus'),
    EndpointSubscriptionTier.zeroTrust => get('sub_tier_zt'),
  };

  String subscriptionErrorLabel(String code) => switch (code) {
    'SUBSCRIPTION_INVALID_NAME' => get('sub_err_invalid_name'),
    'SUBSCRIPTION_INVALID_URL' => get('sub_err_invalid_url'),
    'SUBSCRIPTION_EMPTY' => get('sub_err_empty'),
    'SUBSCRIPTION_INVALID_BODY' => get('sub_err_invalid_body'),
    'SUBSCRIPTION_LIMIT' => get('sub_err_limit'),
    'SUBSCRIPTION_STALE_REVISION' => get('sub_err_stale'),
    'SUBSCRIPTION_NETWORK' => get('sub_err_network'),
    'SUBSCRIPTION_TIMEOUT' => get('sub_err_timeout'),
    'SUBSCRIPTION_TOO_LARGE' => get('sub_err_too_large'),
    'SUBSCRIPTION_HTTP_ERROR' => get('sub_err_http'),
    'SUBSCRIPTION_TIER_MISMATCH' => get('sub_err_unavailable'),
    'SUBSCRIPTION_VERIFY_ACCOUNT' => get('sub_verify_account'),
    'SUBSCRIPTION_PROBE_CONNECTED_TUN_UNSUPPORTED' => get(
      'sub_probe_tun_unsupported',
    ),
    'SUBSCRIPTION_DOMAIN_UNSUPPORTED' => get('sub_domain_unsupported'),
    // Downloads and probes never leave the tunnel or Kill Switch; in this
    // state they are unavailable rather than failing on the network.
    'SUBSCRIPTION_PROTECTED_PATH_UNAVAILABLE' => get('sub_err_unavailable'),
    _ => get('sub_err_unavailable'),
  };

  /// Feature-table keys whose English value may be reused.
  ///
  /// Protocol names, product names, placeholder-only templates, and loanwords
  /// that the main catalog also spells in English.
  @visibleForTesting
  static const Set<String> kFeatureEnglishAllowlist = <String>{
    'nq_doh',
    'nq_dot',
    'home_kill_switch',
    'dns',
    'error_field',
    'password',
    'endpoint_preset_fi',
    'endpoint_preset_de',
    'endpoint_preset_us',
    // French spells it the same.
    'onboarding_notifications_title',
    // French and German also use “Version”.
    'macos_service_version',
    // The SI millisecond symbol is kept in Latin-script locales.
    'sub_rtt_ms',
  };

  @visibleForTesting
  static bool get debugCatalogsAreComplete {
    if (!_featureTablesComplete(kScenarioCatalogs, kScenarioEn) ||
        !_featureTablesComplete(
          kEndpointPresetCatalogs,
          kEndpointPresetCatalogs['en']!,
        ) ||
        !_featureTablesComplete(kAdsCatalogs, kAdsCatalogs['en']!) ||
        !_featureTablesComplete(
          kUxRefinementCatalogs,
          kUxRefinementCatalogs['en']!,
        ) ||
        !_featureTablesComplete(
          kTlsFingerprintCatalogs,
          kTlsFingerprintCatalogs['en']!,
        ) ||
        !_featureTablesComplete(
          kDiagnosticsCatalogs,
          kDiagnosticsCatalogs['en']!,
        ) ||
        !_featureTablesComplete(kUiWorkflowCatalogs, kUiWorkflowEn) ||
        !_featureTablesComplete(
          kUpdateCheckCatalogs,
          kUpdateCheckCatalogs['en']!,
        ) ||
        !_featureTablesComplete(kOnboardingCatalogs, kOnboardingEn) ||
        !_featureTablesComplete(kProtonCatalogs, kProtonEn) ||
        !_featureTablesComplete(
          kLinuxDesktopCatalogs,
          kLinuxDesktopCatalogs['en']!,
        ) ||
        !_featureTablesComplete(kMacDesktopCatalogs, kMacDesktopEn) ||
        !_featureTablesComplete(kWindowsRecoveryCatalogs, kWindowsRecoveryEn) ||
        !_featureTablesComplete(kNetworkQualityCatalogs, kNetworkQualityEn) ||
        !_featureTablesComplete(kL4Catalogs, kL4En) ||
        !_featureTablesComplete(kVpnGateCatalogs, kVpnGateEn) ||
        !_featureTablesComplete(kNetworkSettingsCatalogs, kNetworkSettingsEn) ||
        !_featureTablesComplete(kChainCatalogs, kChainEn) ||
        !_featureTablesComplete(
          kEndpointSubscriptionCatalogs,
          kEndpointSubscriptionCatalogs['en']!,
        )) {
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

    scan(kDiagnosticsCatalogs, kDiagnosticsCatalogs['en']!);
    scan(kAdsCatalogs, kAdsCatalogs['en']!);
    scan(kLinuxDesktopCatalogs, kLinuxDesktopCatalogs['en']!);
    scan(kMacDesktopCatalogs, kMacDesktopEn);
    scan(kUiWorkflowCatalogs, kUiWorkflowEn);
    scan(kUpdateCheckCatalogs, kUpdateCheckCatalogs['en']!);
    scan(kOnboardingCatalogs, kOnboardingEn);
    scan(kNetworkQualityCatalogs, kNetworkQualityEn);
    scan(kWindowsRecoveryCatalogs, kWindowsRecoveryEn);
    scan(kL4Catalogs, kL4En);
    scan(kVpnGateCatalogs, kVpnGateEn);
    scan(kNetworkSettingsCatalogs, kNetworkSettingsEn);
    scan(kChainCatalogs, kChainEn);
    scan(kTlsFingerprintCatalogs, kTlsFingerprintCatalogs['en']!);
    scan(kEndpointPresetCatalogs, kEndpointPresetCatalogs['en']!);
    scan(kUxRefinementCatalogs, kUxRefinementCatalogs['en']!);
    scan(kEndpointSubscriptionCatalogs, kEndpointSubscriptionCatalogs['en']!);
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
    if (!_placeholdersPreserved(kScenarioEn, kScenarioCatalogs.values) ||
        !_placeholdersPreserved(kEnCatalog, kCatalogs.values) ||
        !_placeholdersPreserved(
          kDiagnosticsCatalogs['en']!,
          kDiagnosticsCatalogs.values,
        ) ||
        !_placeholdersPreserved(kUiWorkflowEn, kUiWorkflowCatalogs.values) ||
        !_placeholdersPreserved(
          kUpdateCheckCatalogs['en']!,
          kUpdateCheckCatalogs.values,
        ) ||
        !_placeholdersPreserved(kOnboardingEn, kOnboardingCatalogs.values) ||
        !_placeholdersPreserved(kMacDesktopEn, kMacDesktopCatalogs.values) ||
        !_placeholdersPreserved(kProtonEn, kProtonCatalogs.values) ||
        !_placeholdersPreserved(
          kNetworkQualityEn,
          kNetworkQualityCatalogs.values,
        ) ||
        !_placeholdersPreserved(
          kWindowsRecoveryEn,
          kWindowsRecoveryCatalogs.values,
        ) ||
        !_placeholdersPreserved(kL4En, kL4Catalogs.values) ||
        !_placeholdersPreserved(kVpnGateEn, kVpnGateCatalogs.values) ||
        !_placeholdersPreserved(
          kNetworkSettingsEn,
          kNetworkSettingsCatalogs.values,
        ) ||
        !_placeholdersPreserved(kChainEn, kChainCatalogs.values) ||
        !_placeholdersPreserved(
          kUxRefinementCatalogs['en']!,
          kUxRefinementCatalogs.values,
        ) ||
        !_placeholdersPreserved(
          kEndpointSubscriptionCatalogs['en']!,
          kEndpointSubscriptionCatalogs.values,
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
