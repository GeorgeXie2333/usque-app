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

// L4 experimental copy is keyed by AppStrings catalog id. Missing ids fall
// back to English. Companion locale maps live in features_*.dart.
const kL4En = <String, String>{
  'l4_quic_not_ready': 'Preparing the L4 connection',
  'l4_unsupported_packets': 'Unsupported or malformed packets rejected',
  'l4_budget_rejections': 'Resource admissions rejected',
  'l4_not_applicable': 'Not applicable (L4)',
  'l4_mode': 'L4 (experimental)',
  'l4_transport_hint':
      'TCP only. Apps that need UDP may not work. Auto excludes L4.',
  'l4_explanation':
      'L4 carries TCP traffic through HTTP/3 and works with VPN, SOCKS5 and HTTP proxies. DNS requests from the VPN are converted to TCP. Apps needing other UDP traffic, remote ping, IP fragments or extension headers may not work. Select L4 manually; Automatic does not choose it.',
  'l4_unsupported':
      'L4 is unavailable in this version of Usque. Check for updates in Settings.',
  'l4_sni_identity':
      'Set automatically by your account. Your server name for other connection modes is kept.',
  'l4_edge_requires_l4':
      'Edge-resolved DNS requires L4. Select another proxy DNS mode before switching to Auto, H3 or H2.',
  'proxy_dns_edge_resolved': 'Cloudflare edge (L4 only; no local lookup)',
  'l4_verified': 'L4 has accepted an app connection',
  'l4_unverified': 'Server connected; no app connection verified yet',
  'l4_status_unknown': 'App connection status unavailable',
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
  'l4_quic_not_ready': '正在准备 L4 连接',
  'l4_unsupported_packets': '已拒绝的不支持或畸形数据包',
  'l4_budget_rejections': '资源准入拒绝次数',
  'l4_not_applicable': '不适用（L4）',
  'l4_mode': 'L4（实验性）',
  'l4_transport_hint': '仅支持 TCP，需要 UDP 的应用可能无法使用。自动模式不包含 L4。',
  'l4_explanation':
      'L4 通过 HTTP/3 转发 TCP 流量，可用于 VPN、SOCKS5 和 HTTP 代理。VPN 的 DNS 查询会自动改用 TCP。需要其他 UDP 流量、远端 Ping、IP 分片或扩展头的应用可能无法使用。请手动选择 L4，“自动”不会选用此模式。',
  'l4_unsupported': '此版本的 Usque 无法使用 L4，请在“设置”中检查更新。',
  'l4_sni_identity': '由账号自动设置，无需修改。其他连接模式的服务器名称会保留。',
  'l4_edge_requires_l4': '边缘解析 DNS 仅适用于 L4。切换 Auto、H3 或 H2 前，请先选择其他代理 DNS 模式。',
  'proxy_dns_edge_resolved': 'Cloudflare 边缘解析（仅 L4，不在本地解析）',
  'l4_verified': 'L4 已成功建立过应用连接',
  'l4_unverified': '已连接服务器，尚未确认能否建立应用连接',
  'l4_status_unknown': '暂时无法确认应用连接状态',
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

const Map<String, Map<String, String>> kL4Catalogs =
    <String, Map<String, String>>{
      'en': kL4En,
      'zh_CN': kL4ZhCn,
      'zh_HK': kL4ZhHk,
      'zh_TW': kL4ZhTw,
      'ja': kL4Ja,
      'ko': kL4Ko,
      'es': kL4Es,
      'pt': kL4Pt,
      'fr': kL4Fr,
      'nl': kL4Nl,
      'tr': kL4Tr,
      'ru': kL4Ru,
      'fa': kL4Fa,
      'ar': kL4Ar,
      'de': kL4De,
      'id': kL4Id,
      'it': kL4It,
      'pl': kL4Pl,
      'th': kL4Th,
      'uk': kL4Uk,
      'vi': kL4Vi,
    };
