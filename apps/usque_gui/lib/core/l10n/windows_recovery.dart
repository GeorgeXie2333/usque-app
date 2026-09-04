const Map<String, String> kWindowsRecoveryEn = <String, String>{
  'WINDOWS_RECOVERY_FAILED':
      'The previous VPN network state could not be fully restored. No new VPN connection was started. Retry the connection or inspect local diagnostics.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Windows network recovery is taking longer than expected. No new VPN connection was started. Wait for recovery to finish before retrying.',
  'WINDOWS_RECOVERY_CONFLICT':
      'The network state changed or is still in use by another session. Automatic recovery was stopped to protect the active connection.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'This Windows Agent does not support safe automatic recovery. Update the application and Agent together, then retry.',
};

const Map<String, String> kWindowsRecoveryZhCn = <String, String>{
  'WINDOWS_RECOVERY_FAILED': '未能完整恢复上次 VPN 的网络状态，尚未建立新 VPN 连接。请重试连接，或查看本地诊断。',
  'WINDOWS_RECOVERY_TIMEOUT': 'Windows 网络状态恢复耗时较长，尚未建立新 VPN 连接。请等待恢复完成后再重试。',
  'WINDOWS_RECOVERY_CONFLICT': '网络状态已变化，或仍被其他会话使用。为保护现有连接，已停止自动恢复。',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      '当前 Windows Agent 不支持安全的自动恢复。请同时更新应用和 Agent 后重试。',
};
