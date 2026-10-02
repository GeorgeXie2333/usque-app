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
  'l4_budget_rejections': 'Connections rejected for lack of resources',
  'l4_not_applicable': 'Not applicable (L4)',
  'l4_mode': 'L4 (experimental)',
  'l4_transport_hint':
      'TCP only. Apps that need UDP may not work. Auto excludes L4.',
  'l4_explanation':
      'L4 carries TCP traffic through HTTP/3 and works with VPN, SOCKS5 and HTTP proxies. DNS requests from the VPN are converted to TCP. Apps needing other UDP traffic, remote ping, IP fragments or extension headers may not work.',
  'l4_unsupported':
      'L4 is unavailable in this version of Usque. Check for updates in Settings.',
  'l4_sni_identity':
      'Set automatically by your account. Your server name for other connection modes is kept.',
  'l4_edge_requires_l4':
      'This connection cannot resolve names at the proxy server. Choose another DNS option.',
  'proxy_dns_edge_resolved': 'Resolve at the proxy server',
  'l4_verified': 'L4 has accepted an app connection',
  'l4_unverified': 'Server connected; no app connection verified yet',
  'l4_status_unknown': 'App connection status unavailable',
  'l4_sessions': 'Sessions / draining',
  'l4_flows': 'Active / waiting streams',
  'l4_connect': 'CONNECT successes / failures / timeouts',
  'l4_buffers': 'Buffer usage (bytes)',
  'l4_backpressure': 'Send / receive backpressure',
  'l4_tun_flows': 'TUN TCP / half-open',
  'l4_udp': 'UDP packets rejected',
  'l4_dns': 'DNS conversions / failures / timeouts',
  'l4_migration': 'Streams preserved by migration / ended by rebuild',
  'l4_na':
      'Address assignment, datagram queue, MTU and UDP timeout metrics do not apply in L4 mode.',
};

const kL4ZhCn = <String, String>{
  'l4_quic_not_ready': '正在准备 L4 连接',
  'l4_unsupported_packets': '已拒绝的不支持或畸形数据包',
  'l4_budget_rejections': '因资源不足拒绝的连接',
  'l4_not_applicable': '不适用（L4）',
  'l4_mode': 'L4（实验性）',
  'l4_transport_hint': '仅支持 TCP，需要 UDP 的应用可能无法使用。自动模式不包含 L4。',
  'l4_explanation':
      'L4 通过 HTTP/3 转发 TCP 流量，可用于 VPN、SOCKS5 和 HTTP 代理。VPN 的 DNS 查询会自动改用 TCP。需要其他 UDP 流量、远端 Ping、IP 分片或扩展头的应用可能无法使用。',
  'l4_unsupported': '此版本的 Usque 无法使用 L4，请在“设置”中检查更新。',
  'l4_sni_identity': '由账号自动设置，无需修改。其他连接模式的服务器名称会保留。',
  'l4_edge_requires_l4': '当前连接不支持由代理服务器解析，请选择其他 DNS 方式。',
  'proxy_dns_edge_resolved': '由代理服务器解析',
  'l4_verified': 'L4 已成功建立过应用连接',
  'l4_unverified': '已连接服务器，尚未确认能否建立应用连接',
  'l4_status_unknown': '暂时无法确认应用连接状态',
  'l4_sessions': '会话数／排空会话',
  'l4_flows': '活跃／等待流',
  'l4_connect': 'CONNECT 成功／失败／超时',
  'l4_buffers': '缓冲区占用（字节）',
  'l4_backpressure': '发送／接收背压',
  'l4_tun_flows': 'TUN TCP／半开连接',
  'l4_udp': '已拒绝的 UDP 包',
  'l4_dns': 'DNS 转换成功／失败／超时',
  'l4_migration': '迁移保留流／重建终止流',
  'l4_na': 'L4 模式不提供地址分配、数据报队列、MTU 和 UDP 超时指标。',
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
