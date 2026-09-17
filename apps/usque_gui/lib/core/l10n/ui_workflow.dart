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

// Shared workflow copy is keyed by AppStrings catalog id. Missing ids fall
// back to English. Companion locale maps live in features_*.dart.
const Map<String, String> kUiWorkflowEn = <String, String>{
  'cc_label': 'HTTP/3 congestion control',
  'cc_help': 'Applies on your next manual connection.',
  'cc_upgrade': 'Update Usque in Settings to use this option.',
  'cc_h2': 'This option only affects HTTP/3 connections.',
  'cc_saved': 'Saved',
  'cc_pending': 'Pending next manual connection.',
  'save_changes': 'Apply changes',
  'saving_changes': 'Applying changes…',
  'unsaved_changes': 'Unapplied changes',
  'changes_applied': 'Changes applied',
  'changes_apply_hint': 'Edits take effect only after you apply them.',
  'changes_failed':
      'Could not apply changes. Review the saved values and try again.',
  'form_errors': 'Check the highlighted fields before applying changes.',
  'discard_changes_title': 'Discard unapplied changes?',
  'discard_changes_body':
      'Your edits have not been applied. Keep editing to save them, or discard them to leave.',
  'keep_editing': 'Keep editing',
  'discard_changes': 'Discard changes',
  'invalid_port': 'Enter a port from 1 to 65535.',
  'listener_exposure': 'Listener addresses allow LAN access',
  'invalid_ipv4': 'Enter a valid IPv4 address, for example 127.0.0.1.',
  'invalid_ipv6': 'Enter a valid IPv6 address, for example ::1.',
  'output_running': 'Running',
  'output_waiting': 'Enabled · not running',
  'output_disabled': 'Disabled',
  'output_starting': 'Starting',
  'output_stopping': 'Stopping',
  'output_reconnecting': 'Reconnecting',
  'output_degraded': 'Limited',
  'output_error': 'Error',
  'output_unknown': 'Status unavailable',
  'shared_network_scope': 'Network settings are shared by all accounts.',
  'connection_details': 'Connection details',
  'home_overview': 'Connection overview',
  'home_exit_region': 'Exit region',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': 'Traffic',
  'home_traffic_window': 'Last 60 seconds',
  'home_traffic_idle': 'Traffic appears after connecting',
  'home_traffic_waiting': 'Waiting for traffic data',
  'home_traffic_unavailable': 'Traffic history unavailable',
  'home_traffic_stale': 'Traffic updates delayed',
  'home_outputs_next': 'Available after connecting',
  'home_outputs_retry': 'VPN and proxies for the next connection',
  'connection_protection_group': 'Connection & protection',
  'proxy_routing_group': 'Proxy & routing',
  'application_group': 'Application',
  'proxy_settings_link': 'Listener addresses, ports, authentication and DNS.',
  'local_proxy_settings': 'Local proxy settings',
  'proxy_switches_hint':
      'Switch changes are saved automatically. Apply listener and DNS edits with Apply changes.',
  'proxy_auth_separate':
      'Use Save username and password below to apply these credentials.',
  'reset_draft_hint':
      'Defaults will be loaded into this form. Apply changes to make them take effect.',
};

const Map<String, String> kUiWorkflowZhCn = <String, String>{
  'cc_label': 'HTTP/3 拥塞控制算法',
  'cc_help': '下次手动连接生效。',
  'cc_upgrade': '请在“设置”中更新 Usque 后使用此选项。',
  'cc_h2': '此选项仅影响 HTTP/3 连接。',
  'cc_saved': '已保存',
  'cc_pending': '待下次手动连接生效。',
  'save_changes': '应用修改',
  'saving_changes': '正在应用修改…',
  'unsaved_changes': '有未应用的修改',
  'changes_applied': '修改已生效',
  'changes_apply_hint': '编辑完成后，点击“应用修改”才会生效。',
  'changes_failed': '未能应用修改，请检查已保存的值后重试。',
  'form_errors': '请先修正标出的字段，再应用修改。',
  'discard_changes_title': '放弃未应用的修改？',
  'discard_changes_body': '这些修改尚未生效。继续编辑可保留修改，放弃后将离开此页。',
  'keep_editing': '继续编辑',
  'discard_changes': '放弃修改',
  'invalid_port': '请输入 1–65535 之间的端口。',
  'listener_exposure': '监听地址允许局域网访问',
  'invalid_ipv4': '请输入有效的 IPv4 地址，例如 127.0.0.1。',
  'invalid_ipv6': '请输入有效的 IPv6 地址，例如 ::1。',
  'output_running': '运行中',
  'output_waiting': '已启用 · 尚未运行',
  'output_disabled': '未启用',
  'output_starting': '正在启动',
  'output_stopping': '正在停止',
  'output_reconnecting': '正在重连',
  'output_degraded': '部分受限',
  'output_error': '异常',
  'output_unknown': '状态不可用',
  'shared_network_scope': '网络设置由所有账号共用。',
  'connection_details': '连接详情',
  'home_overview': '连接概览',
  'home_exit_region': '出口地区',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': '实时流量',
  'home_traffic_window': '最近 60 秒',
  'home_traffic_idle': '连接后显示流量',
  'home_traffic_waiting': '等待流量数据',
  'home_traffic_unavailable': '暂无流量记录',
  'home_traffic_stale': '流量数据更新延迟',
  'home_outputs_next': '连接后将启用',
  'home_outputs_retry': '下次连接将启用的 VPN 和代理',
  'connection_protection_group': '连接与保护',
  'proxy_routing_group': '代理与分流',
  'application_group': '应用',
  'proxy_settings_link': '监听地址、端口、认证与 DNS。',
  'local_proxy_settings': '本地代理设置',
  'proxy_switches_hint': '开关更改会自动保存；监听地址、端口与 DNS 的修改需点击“应用修改”。',
  'proxy_auth_separate': '请点击下方“保存用户名和密码”来应用此处的修改。',
  'reset_draft_hint': '默认值将填入表单，点击“应用修改”后才会生效。',
};

const Map<String, Map<String, String>> kUiWorkflowCatalogs =
    <String, Map<String, String>>{
      'en': kUiWorkflowEn,
      'zh_CN': kUiWorkflowZhCn,
      'zh_HK': kUiWorkflowZhHk,
      'zh_TW': kUiWorkflowZhTw,
      'ja': kUiWorkflowJa,
      'ko': kUiWorkflowKo,
      'es': kUiWorkflowEs,
      'pt': kUiWorkflowPt,
      'fr': kUiWorkflowFr,
      'nl': kUiWorkflowNl,
      'tr': kUiWorkflowTr,
      'ru': kUiWorkflowRu,
      'fa': kUiWorkflowFa,
      'ar': kUiWorkflowAr,
      'de': kUiWorkflowDe,
      'id': kUiWorkflowId,
      'it': kUiWorkflowIt,
      'pl': kUiWorkflowPl,
      'th': kUiWorkflowTh,
      'uk': kUiWorkflowUk,
      'vi': kUiWorkflowVi,
    };
