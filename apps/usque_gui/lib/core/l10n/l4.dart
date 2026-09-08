// L4 experimental strings; other locales use the explicit English fallback.
const kL4En = <String, String>{
  'l4_quic_not_ready': 'Waiting for a ready QUIC session',
  'l4_unsupported_packets': 'Unsupported or malformed packets rejected',
  'l4_budget_rejections': 'Resource admissions rejected',
  'l4_not_applicable': 'Not applicable (L4)',
  'l4_mode': 'L4 (experimental)',
  'l4_explanation':
      'TCP-only over HTTP/3. Supports VPN/TUN, SOCKS5 and HTTP; TUN DNS is converted to TCP. Auto never selects L4. Other UDP, remote ping, IP fragments and extension headers are unsupported; some apps may not work.',
  'l4_unsupported':
      'This engine has not declared complete L4 support. L4 cannot be enabled.',
  'l4_sni_identity':
      'Read-only: derived from the loaded account identity. Your CONNECT-IP SNI is preserved.',
  'l4_edge_requires_l4':
      'Edge-resolved DNS requires L4. Select another proxy DNS mode before switching to Auto, H3 or H2.',
  'proxy_dns_edge_resolved': 'Cloudflare edge (L4 only; no local lookup)',
  'l4_verified': 'L4 CONNECT verified',
  'l4_unverified': 'QUIC ready; L4 CONNECT not yet verified',
  'l4_status_unknown': 'L4 verification status unknown',
  'l4_sessions': 'Sessions / draining',
  'l4_flows': 'Active / waiting streams',
  'l4_connect': 'CONNECT successes / failures / timeouts',
  'l4_buffers': 'Application buffer budget used (bytes)',
  'l4_backpressure': 'Send / receive backpressure',
  'l4_tun_flows': 'TUN TCP / half-open',
  'l4_udp': 'UDP packets rejected',
  'l4_dns': 'DNS conversions / failures / timeouts',
  'l4_migration': 'Streams preserved by migration / ended by rebuild',
  'l4_na':
      'CONNECT-IP address control, DATAGRAM queues, inner payload MTU and UDP timeout: not applicable in L4.',
};

const kL4ZhCn = <String, String>{
  'l4_quic_not_ready': '等待 QUIC 会话就绪',
  'l4_unsupported_packets': '已拒绝的不支持或畸形数据包',
  'l4_budget_rejections': '资源准入拒绝次数',
  'l4_not_applicable': '不适用（L4）',
  'l4_mode': 'L4（实验性）',
  'l4_explanation':
      '基于 HTTP/3 的 TCP-only 模式，支持 VPN/TUN、SOCKS5 和 HTTP；TUN DNS 自动转换为 TCP。Auto 不包含 L4。其他 UDP、远端 Ping、IP 分片与扩展头不受支持，部分应用可能无法使用。',
  'l4_unsupported': '当前引擎尚未声明完整的 L4 能力，不能启用 L4。',
  'l4_sni_identity': '只读：根据已加载的账户身份派生。原有 CONNECT-IP SNI 将保留。',
  'l4_edge_requires_l4': '边缘解析 DNS 仅适用于 L4。切换 Auto、H3 或 H2 前，请先选择其他代理 DNS 模式。',
  'proxy_dns_edge_resolved': 'Cloudflare 边缘解析（仅 L4，不在本地解析）',
  'l4_verified': '已验证 L4 CONNECT',
  'l4_unverified': 'QUIC 已就绪；L4 CONNECT 尚未验证',
  'l4_status_unknown': 'L4 验证状态未知',
  'l4_sessions': '会话数／排空会话',
  'l4_flows': '活跃／等待流',
  'l4_connect': 'CONNECT 成功／失败／超时',
  'l4_buffers': '应用缓冲预算用量（字节）',
  'l4_backpressure': '发送／接收背压',
  'l4_tun_flows': 'TUN TCP／半开连接',
  'l4_udp': '已拒绝的 UDP 包',
  'l4_dns': 'DNS 转换成功／失败／超时',
  'l4_migration': '迁移保留流／重建终止流',
  'l4_na': 'CONNECT-IP 地址控制、DATAGRAM 队列、内层有效载荷 MTU 和 UDP 超时：L4 下不适用。',
};
