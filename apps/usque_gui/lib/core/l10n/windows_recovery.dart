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
    'The virtual network adapter from the previous connection could not be removed, or its removal could not be confirmed. No new VPN connection has started.';
const String kWindowsAdapterCleanupZhCn =
    '上次连接创建的虚拟网卡未能移除，或无法确认已移除。尚未建立新的 VPN 连接。';

const Map<String, String> kWindowsRecoveryEn = <String, String>{
  'WINDOWS_DEVICE_REUSE_UNSUPPORTED':
      'Your Usque components need to be updated together. Open Settings and check for updates. No new VPN connection was started.',
  'WINDOWS_DEVICE_RECOVERY_REQUIRED':
      'The previous VPN connection has not finished cleaning up. Fully exit and reopen Usque, then retry. If it still fails, open Diagnostics.',
  'WINDOWS_RECOVERY_FAILED':
      'The previous VPN network state could not be fully restored. No new VPN connection was started. Retry the connection or inspect local diagnostics.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows could not restore the previous VPN network state after three automatic attempts. Retry when ready, or inspect local diagnostics.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Usque stopped automatic repair because it could not confirm the previous VPN settings were safe to restore. Open Settings and check for updates, then open Diagnostics and export a diagnostic package if the problem persists.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Windows network recovery is taking longer than expected. No new VPN connection was started. Wait for recovery to finish before retrying.',
  'WINDOWS_RECOVERY_CONFLICT':
      'The network state changed or is still in use by another session. Automatic recovery was stopped to protect the active connection.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'This installation cannot automatically restore the previous VPN settings. Open Settings and update Usque before retrying.',
};

const Map<String, String> kWindowsRecoveryZhCn = <String, String>{
  'WINDOWS_DEVICE_REUSE_UNSUPPORTED':
      'Usque 的连接组件需要一并更新。请打开“设置”检查更新。尚未建立新的 VPN 连接。',
  'WINDOWS_DEVICE_RECOVERY_REQUIRED':
      '上次 VPN 连接尚未清理完成。请彻底退出并重新打开 Usque 后重试；若仍失败，请打开“诊断”。',
  'WINDOWS_RECOVERY_FAILED': '未能完整恢复上次 VPN 的网络状态，尚未建立新 VPN 连接。请重试连接，或查看本地诊断。',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows 在三次自动尝试后仍未能恢复上次 VPN 网络状态。请稍后重试，或查看本地诊断。',
  'WINDOWS_RECOVERY_BLOCKED':
      'Usque 无法确认是否能安全恢复上次 VPN 的设置，已停止自动修复。请打开“设置”检查更新；若问题持续，请打开“诊断”并导出诊断包。',
  'WINDOWS_RECOVERY_TIMEOUT': 'Windows 网络状态恢复耗时较长，尚未建立新 VPN 连接。请等待恢复完成后再重试。',
  'WINDOWS_RECOVERY_CONFLICT': '网络状态已变化，或仍被其他会话使用。为保护现有连接，已停止自动恢复。',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      '当前安装的 Usque 无法自动恢复上次 VPN 的设置。请打开“设置”更新 Usque 后重试。',
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
