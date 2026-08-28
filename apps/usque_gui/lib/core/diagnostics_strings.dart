import '../models/diagnostics_models.dart';
import 'app_strings.dart';

String diagnosticStatusLabel(AppStrings strings, DiagnosticCheckStatus status) {
  final zh = strings.languageCode == 'zh';
  return switch (status) {
    DiagnosticCheckStatus.pending => zh ? '等待' : 'Waiting',
    DiagnosticCheckStatus.running => zh ? '检查中' : 'Checking',
    DiagnosticCheckStatus.passed => zh ? '通过' : 'Passed',
    DiagnosticCheckStatus.warning => zh ? '警告' : 'Warning',
    DiagnosticCheckStatus.failed => zh ? '失败' : 'Failed',
    DiagnosticCheckStatus.skipped => zh ? '已跳过' : 'Skipped',
    DiagnosticCheckStatus.cancelled => zh ? '已取消' : 'Cancelled',
  };
}

String diagnosticCategoryLabel(
  AppStrings strings,
  DiagnosticCategory category,
) {
  final zh = strings.languageCode == 'zh';
  return switch (category) {
    DiagnosticCategory.localComponent => zh ? '本地组件' : 'Local components',
    DiagnosticCategory.physicalNetwork => zh ? '物理网络' : 'Physical network',
    DiagnosticCategory.transport => zh ? '传输' : 'Transport',
    DiagnosticCategory.tunnel => zh ? '隧道' : 'Tunnel',
    DiagnosticCategory.protection => zh ? '系统保护' : 'System protection',
    DiagnosticCategory.recovery => zh ? '恢复状态' : 'Recovery',
  };
}

String diagnosticCheckLabel(AppStrings strings, String checkId) {
  final zh = strings.languageCode == 'zh';
  if (zh) {
    return _zhChecks[checkId] ?? _humanize(checkId.split('.').last);
  }
  return _englishChecks[checkId] ?? _humanize(checkId.split('.').last);
}

String diagnosticFailureTitle(AppStrings strings, String code) {
  if (strings.languageCode == 'zh') {
    return _zhFailures[code] ?? _humanize(code);
  }
  return _humanize(code);
}

String diagnosticRemediation(AppStrings strings, String key) {
  final zh = strings.languageCode == 'zh';
  return switch (key) {
    'try_http2' =>
      zh
          ? '可改用 HTTP/2，并继续观察恢复探测。'
          : 'Use HTTP/2 and keep recovery probes enabled.',
    'check_physical_network' =>
      zh
          ? '检查当前网络、DNS 与地址族是否可用。'
          : 'Check the current network, DNS, and address-family availability.',
    'refresh_or_replace_identity' =>
      zh
          ? '刷新或重新创建身份材料后再连接。'
          : 'Refresh or replace the identity before reconnecting.',
    'replace_identity' =>
      zh ? '重新配置有效身份。' : 'Configure a valid identity again.',
    'review_configuration' =>
      zh
          ? '检查当前配置并修正无效值。'
          : 'Review the configuration and correct invalid values.',
    'restore_platform_state' =>
      zh ? '先恢复平台网络状态，再重试。' : 'Restore platform network state before retrying.',
    'resolve_dependency' =>
      zh ? '先处理上游检查失败。' : 'Resolve the failed prerequisite first.',
    'run_deep_diagnostics' =>
      zh
          ? '在适当环境中运行深度诊断。'
          : 'Run deep diagnostics in an appropriate environment.',
    'run_release_leak_gate' =>
      zh
          ? '使用独立观察者运行发布泄漏门禁。'
          : 'Run the release leak gate with an independent observer.',
    'inspect_platform_state' =>
      zh
          ? '通过平台只读检查确认实际状态。'
          : 'Confirm actual state with the platform inspector.',
    'generate_tunnel_traffic' =>
      zh
          ? '产生少量隧道流量后重新检查。'
          : 'Generate a small amount of tunnel traffic, then check again.',
    'export_diagnostics' =>
      zh
          ? '导出脱敏诊断包并联系支持。'
          : 'Export a sanitized diagnostic bundle for support.',
    'retry' => zh ? '稍后重试。' : 'Try again shortly.',
    'none' || '' => zh ? '无需操作。' : 'No action is required.',
    _ =>
      zh
          ? '按照错误码检查相关配置和网络状态。'
          : 'Use the error code to review the related configuration and network state.',
  };
}

String connectionEventLabel(
  AppStrings strings,
  ConnectionTimelineEventType type,
) {
  final zh = strings.languageCode == 'zh';
  return switch (type) {
    ConnectionTimelineEventType.attemptStarted =>
      zh ? '开始连接尝试' : 'Connection attempt started',
    ConnectionTimelineEventType.endpointResolved =>
      zh ? '端点解析完成' : 'Endpoint resolved',
    ConnectionTimelineEventType.socketConnected =>
      zh ? 'Socket 已连接' : 'Socket connected',
    ConnectionTimelineEventType.tlsReady => zh ? 'TLS 就绪' : 'TLS ready',
    ConnectionTimelineEventType.quicReady => zh ? 'QUIC 就绪' : 'QUIC ready',
    ConnectionTimelineEventType.masqueAccepted =>
      zh ? 'MASQUE 已接受' : 'MASQUE accepted',
    ConnectionTimelineEventType.peerSettingsReceived =>
      zh ? '收到对端设置' : 'Peer settings received',
    ConnectionTimelineEventType.addressAssigned =>
      zh ? '地址已分配' : 'Address assigned',
    ConnectionTimelineEventType.tunnelReady => zh ? '隧道就绪' : 'Tunnel ready',
    ConnectionTimelineEventType.firstPacketSent =>
      zh ? '首个上行包已发送' : 'First packet sent',
    ConnectionTimelineEventType.firstPacketReceived =>
      zh ? '首个下行包已接收' : 'First packet received',
    ConnectionTimelineEventType.fallbackStarted =>
      zh ? '开始回退到 H2' : 'Fallback to H2 started',
    ConnectionTimelineEventType.reconnectScheduled =>
      zh ? '已安排重连' : 'Reconnect scheduled',
    ConnectionTimelineEventType.networkChanged =>
      zh ? '物理网络发生变化' : 'Physical network changed',
    ConnectionTimelineEventType.recoveryProbeStarted =>
      zh ? '开始 H3 恢复探测' : 'H3 recovery probe started',
    ConnectionTimelineEventType.recoveryProbeSucceeded =>
      zh ? 'H3 恢复探测成功' : 'H3 recovery probe succeeded',
    ConnectionTimelineEventType.recoveryProbeFailed =>
      zh ? 'H3 恢复探测失败' : 'H3 recovery probe failed',
    ConnectionTimelineEventType.pathPromoted =>
      zh ? '候选路径已切换为活动路径' : 'Candidate path promoted',
    ConnectionTimelineEventType.queueSaturated =>
      zh ? '发送队列已满' : 'Send queue saturated',
    ConnectionTimelineEventType.disconnected => zh ? '连接已断开' : 'Disconnected',
    ConnectionTimelineEventType.failed => zh ? '连接失败' : 'Connection failed',
  };
}

const Map<String, String> _englishChecks = <String, String>{
  'engine.control_channel': 'Engine control channel',
  'engine.event_stream': 'Engine event stream',
  'engine.capabilities': 'API capabilities',
  'engine.configuration': 'Configuration',
  'engine.secure_storage_metadata': 'Identity metadata',
  'frontend.socks_port': 'SOCKS5 listener',
  'frontend.http_port': 'HTTP listener',
  'frontend.system_proxy_state': 'System proxy state',
  'physical.network_present': 'Physical network',
  'physical.ipv4_route': 'Physical IPv4 route',
  'physical.ipv6_route': 'Physical IPv6 route',
  'physical.dns_available': 'Physical DNS',
  'physical.network_generation': 'Network generation',
  'transport.h3_connect': 'HTTP/3 connection',
  'transport.h3_datagram': 'HTTP/3 datagrams',
  'transport.h2_tcp': 'HTTP/2 TCP',
  'transport.h2_tls': 'HTTP/2 TLS',
  'transport.h2_connect': 'HTTP/2 CONNECT-IP',
  'transport.endpoint_pin': 'Endpoint pin',
  'transport.fallback_policy': 'Fallback policy',
  'tunnel.address_assignment': 'Tunnel address assignment',
  'tunnel.routes': 'Tunnel routes',
  'tunnel.dns': 'Tunnel DNS',
  'tunnel.first_packet': 'First packet',
  'tunnel.ipv4_egress': 'IPv4 egress',
  'tunnel.ipv6_egress': 'IPv6 egress',
  'protection.kill_switch': 'Kill Switch state',
  'protection.dns_path': 'DNS path',
  'protection.route_ownership': 'Route ownership',
  'protection.recovery_journal': 'Recovery journal',
};

const Map<String, String> _zhChecks = <String, String>{
  'engine.control_channel': 'Engine 控制通道',
  'engine.event_stream': 'Engine 事件流',
  'engine.capabilities': 'API 能力',
  'engine.configuration': '配置校验',
  'engine.secure_storage_metadata': '身份元数据',
  'frontend.socks_port': 'SOCKS5 监听端口',
  'frontend.http_port': 'HTTP 监听端口',
  'frontend.system_proxy_state': '系统代理状态',
  'physical.network_present': '物理网络',
  'physical.ipv4_route': '物理 IPv4 路由',
  'physical.ipv6_route': '物理 IPv6 路由',
  'physical.dns_available': '物理 DNS',
  'physical.network_generation': '网络代次',
  'transport.h3_connect': 'HTTP/3 连接',
  'transport.h3_datagram': 'HTTP/3 Datagram',
  'transport.h2_tcp': 'HTTP/2 TCP',
  'transport.h2_tls': 'HTTP/2 TLS',
  'transport.h2_connect': 'HTTP/2 CONNECT-IP',
  'transport.endpoint_pin': '端点 Pin',
  'transport.fallback_policy': '回退策略',
  'tunnel.address_assignment': '隧道地址分配',
  'tunnel.routes': '隧道路由',
  'tunnel.dns': '隧道 DNS',
  'tunnel.first_packet': '首个数据包',
  'tunnel.ipv4_egress': 'IPv4 出口',
  'tunnel.ipv6_egress': 'IPv6 出口',
  'protection.kill_switch': 'Kill Switch 状态',
  'protection.dns_path': 'DNS 路径',
  'protection.route_ownership': '路由所有权',
  'protection.recovery_journal': '恢复日志',
};

const Map<String, String> _zhFailures = <String, String>{
  'ENGINE_UNAVAILABLE': 'Engine 不可用',
  'AGENT_UNREACHABLE': 'Agent 无法访问',
  'VPN_SERVICE_UNAVAILABLE': 'VPN Service 不可用',
  'PROXY_PORT_IN_USE': '代理端口被占用',
  'PHYSICAL_IPV4_UNAVAILABLE': '物理 IPv4 不可用',
  'PHYSICAL_IPV6_UNAVAILABLE': '物理 IPv6 不可用',
  'PHYSICAL_DNS_UNAVAILABLE': '物理 DNS 不可用',
  'PHYSICAL_NETWORK_CHANGED': '物理网络已变化',
  'H3_UDP_UNREACHABLE': 'H3 UDP 不可达',
  'H3_HANDSHAKE_TIMEOUT': 'H3 握手超时',
  'H3_PROTOCOL_ERROR': 'H3 协议错误',
  'H3_DATAGRAM_UNAVAILABLE': 'H3 Datagram 不可用',
  'H3_CONNECTION_CLOSED': 'H3 连接已关闭',
  'H2_TCP_CONNECT_FAILED': 'H2 TCP 连接失败',
  'H2_TLS_FAILED': 'H2 TLS 失败',
  'H2_STREAM_CLOSED': 'H2 Stream 已关闭',
  'H2_CONNECT_REJECTED': 'H2 CONNECT 被拒绝',
  'H2_GOAWAY': 'H2 收到 GOAWAY',
  'ALL_TRANSPORTS_FAILED': 'HTTP/3 与 HTTP/2 均连接失败',
  'ENDPOINT_PIN_MISMATCH': '端点 Pin 不匹配',
  'IDENTITY_INVALID': '身份无效',
  'AUTHENTICATION_FAILED': '认证失败',
  'CONFIGURATION_INVALID': '配置无效',
  'CONNECT_IP_REJECTED': 'CONNECT-IP 被拒绝',
  'ADDRESS_ASSIGNMENT_INVALID': '地址分配无效',
  'TUN_ADDRESS_MISSING': '缺少 TUN 地址',
  'SOCKET_PROTECTION_FAILED': 'Socket 保护失败',
  'SOCKET_AFFINITY_INVALID': 'Socket 网络绑定失效',
  'DNS_APPLY_FAILED': 'DNS 应用失败',
  'ROUTE_APPLY_FAILED': '路由应用失败',
  'KILL_SWITCH_APPLY_FAILED': 'Kill Switch 应用失败',
  'KILL_SWITCH_STATE_MISMATCH': 'Kill Switch 状态不一致',
  'SYSTEM_PROXY_STATE_MISMATCH': '系统代理状态不一致',
  'ROUTE_RESTORE_INCOMPLETE': '路由恢复不完整',
  'DNS_RESTORE_INCOMPLETE': 'DNS 恢复不完整',
  'SYSTEM_PROXY_STALE': '系统代理残留',
  'PLATFORM_RECOVERY_PENDING': '平台恢复待处理',
  'PACKET_SEND_FAILED': '数据包发送失败',
  'PACKET_SEND_TIMEOUT': '数据包发送超时',
  'PACKET_RECEIVE_FAILED': '数据包接收失败',
  'PACKET_RECEIVE_STALLED': '数据包接收停滞',
  'SEND_QUEUE_FULL': '发送队列已满',
  'DIAGNOSTIC_ALREADY_RUNNING': '已有诊断正在运行',
  'DIAGNOSTIC_TIMEOUT': '诊断检查超时',
  'DIAGNOSTIC_CANCELLED': '诊断已取消',
  'DIAGNOSTIC_DEPENDENCY_FAILED': '诊断依赖项失败',
  'INTERNAL': '内部错误',
};

String _humanize(String value) {
  final words = value.replaceAll(RegExp(r'[_\-.]+'), ' ').trim().toLowerCase();
  if (words.isEmpty) {
    return value;
  }
  return '${words[0].toUpperCase()}${words.substring(1)}';
}
