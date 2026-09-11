import 'features_ar.dart';
import 'features_de.dart';
import 'features_es.dart';
import 'features_fa.dart';
import 'features_fr.dart';
import 'features_id.dart';
import 'features_it.dart';
import 'features_ja.dart';
import 'features_ko.dart';
import 'features_nl.dart';
import 'features_pl.dart';
import 'features_pt.dart';
import 'features_ru.dart';
import 'features_th.dart';
import 'features_tr.dart';
import 'features_uk.dart';
import 'features_vi.dart';
import 'features_zh_hk.dart';
import 'features_zh_tw.dart';

// Network-settings save status is keyed by AppStrings catalog id. Missing ids
// fall back to English. Companion locale maps live in features_*.dart.
const kNetworkSettingsEn = <String, String>{
  'settings_applying': 'Saved, applying',
  'settings_applied': 'Saved and applied',
  'settings_deferred': 'Saved, takes effect on the next manual connection',
  'settings_failed': 'Saved, application failed',
  'settings_unknown': 'Result not yet confirmed',
  'settings_saved': 'Saved',
  'settings_unsupported':
      'Restart or update the Engine to save network settings.',
  'settings_save_failed':
      'Settings could not be saved. Your edits are retained.',
  'settings_reconnect': 'Reconnect',
};

const kNetworkSettingsZh = <String, String>{
  'settings_applying': '已保存，正在应用',
  'settings_applied': '已保存并应用',
  'settings_deferred': '已保存，下次手动连接生效',
  'settings_failed': '已保存，应用失败',
  'settings_unknown': '结果尚未确认',
  'settings_saved': '已保存',
  'settings_unsupported': '请重启或更新引擎后保存网络设置。',
  'settings_save_failed': '设置保存失败，已保留你的修改。',
  'settings_reconnect': '重新连接',
};

const Map<String, Map<String, String>> kNetworkSettingsCatalogs =
    <String, Map<String, String>>{
      'en': kNetworkSettingsEn,
      'zh_CN': kNetworkSettingsZh,
      'zh_HK': kNetworkSettingsZhHk,
      'zh_TW': kNetworkSettingsZhTw,
      'ja': kNetworkSettingsJa,
      'ko': kNetworkSettingsKo,
      'es': kNetworkSettingsEs,
      'pt': kNetworkSettingsPt,
      'fr': kNetworkSettingsFr,
      'nl': kNetworkSettingsNl,
      'tr': kNetworkSettingsTr,
      'ru': kNetworkSettingsRu,
      'fa': kNetworkSettingsFa,
      'ar': kNetworkSettingsAr,
      'de': kNetworkSettingsDe,
      'id': kNetworkSettingsId,
      'it': kNetworkSettingsIt,
      'pl': kNetworkSettingsPl,
      'th': kNetworkSettingsTh,
      'uk': kNetworkSettingsUk,
      'vi': kNetworkSettingsVi,
    };
