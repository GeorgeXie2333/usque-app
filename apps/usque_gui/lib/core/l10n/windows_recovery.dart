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

// Windows recovery copy is keyed by AppStrings catalog id. Missing ids fall
// back to English. Companion locale maps live in features_*.dart.

const String kWindowsAdapterCleanupEn =
    'The previous Wintun adapter could not be removed or its removal could not be verified. No new VPN connection has started.';
const String kWindowsAdapterCleanupZhCn =
    '未能移除旧的 Wintun 适配器，或无法确认已移除。尚未建立新的 VPN 连接。';

const Map<String, String> kWindowsRecoveryEn = <String, String>{
  'WINDOWS_RECOVERY_FAILED':
      'The previous VPN network state could not be fully restored. No new VPN connection was started. Retry the connection or inspect local diagnostics.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows could not restore the previous VPN network state after three automatic attempts. Retry when ready, or inspect local diagnostics.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Automatic repair stopped because the previous Windows network state could not be verified safely. Restart the Agent or update Usque, then inspect local diagnostics.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Windows network recovery is taking longer than expected. No new VPN connection was started. Wait for recovery to finish before retrying.',
  'WINDOWS_RECOVERY_CONFLICT':
      'The network state changed or is still in use by another session. Automatic recovery was stopped to protect the active connection.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'This Windows Agent does not support safe automatic recovery. Update the application and Agent together, then retry.',
};

const Map<String, String> kWindowsRecoveryZhCn = <String, String>{
  'WINDOWS_RECOVERY_FAILED': '未能完整恢复上次 VPN 的网络状态，尚未建立新 VPN 连接。请重试连接，或查看本地诊断。',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows 在三次自动尝试后仍未能恢复上次 VPN 网络状态。请稍后重试，或查看本地诊断。',
  'WINDOWS_RECOVERY_BLOCKED':
      '由于无法安全验证上次 Windows 网络状态，自动修复已停止。请重启 Agent 或更新 Usque，然后查看本地诊断。',
  'WINDOWS_RECOVERY_TIMEOUT': 'Windows 网络状态恢复耗时较长，尚未建立新 VPN 连接。请等待恢复完成后再重试。',
  'WINDOWS_RECOVERY_CONFLICT': '网络状态已变化，或仍被其他会话使用。为保护现有连接，已停止自动恢复。',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      '当前 Windows Agent 不支持安全的自动恢复。请同时更新应用和 Agent 后重试。',
};

const Map<String, Map<String, String>> kWindowsRecoveryCatalogs =
    <String, Map<String, String>>{
      'en': kWindowsRecoveryEn,
      'zh_CN': kWindowsRecoveryZhCn,
      'zh_HK': kWindowsRecoveryZhHk,
      'zh_TW': kWindowsRecoveryZhTw,
      'ja': kWindowsRecoveryJa,
      'ko': kWindowsRecoveryKo,
      'es': kWindowsRecoveryEs,
      'pt': kWindowsRecoveryPt,
      'fr': kWindowsRecoveryFr,
      'nl': kWindowsRecoveryNl,
      'tr': kWindowsRecoveryTr,
      'ru': kWindowsRecoveryRu,
      'fa': kWindowsRecoveryFa,
      'ar': kWindowsRecoveryAr,
      'de': kWindowsRecoveryDe,
      'id': kWindowsRecoveryId,
      'it': kWindowsRecoveryIt,
      'pl': kWindowsRecoveryPl,
      'th': kWindowsRecoveryTh,
      'uk': kWindowsRecoveryUk,
      'vi': kWindowsRecoveryVi,
    };

const Map<String, String> kWindowsAdapterCleanupCatalogs = <String, String>{
  'en': kWindowsAdapterCleanupEn,
  'zh_CN': kWindowsAdapterCleanupZhCn,
  'zh_HK': kWindowsAdapterCleanupZhHk,
  'zh_TW': kWindowsAdapterCleanupZhTw,
  'ja': kWindowsAdapterCleanupJa,
  'ko': kWindowsAdapterCleanupKo,
  'es': kWindowsAdapterCleanupEs,
  'pt': kWindowsAdapterCleanupPt,
  'fr': kWindowsAdapterCleanupFr,
  'nl': kWindowsAdapterCleanupNl,
  'tr': kWindowsAdapterCleanupTr,
  'ru': kWindowsAdapterCleanupRu,
  'fa': kWindowsAdapterCleanupFa,
  'ar': kWindowsAdapterCleanupAr,
  'de': kWindowsAdapterCleanupDe,
  'id': kWindowsAdapterCleanupId,
  'it': kWindowsAdapterCleanupIt,
  'pl': kWindowsAdapterCleanupPl,
  'th': kWindowsAdapterCleanupTh,
  'uk': kWindowsAdapterCleanupUk,
  'vi': kWindowsAdapterCleanupVi,
};
