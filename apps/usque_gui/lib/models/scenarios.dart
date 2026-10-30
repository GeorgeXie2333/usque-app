import 'dart:convert';

import 'package:flutter/foundation.dart';

import 'app_models.dart';

enum ProfilesTab { scenes, accounts }

Map<String, Object?> scenarioMap(Object? value) =>
    value is Map ? Map<String, Object?>.from(value) : <String, Object?>{};

String _snake(String value) => value.replaceAllMapped(
  RegExp('[A-Z]'),
  (match) => '_${match[0]!.toLowerCase()}',
);
String _camel(String value) => value.replaceAllMapped(
  RegExp('_([a-z])'),
  (match) => match[1]!.toUpperCase(),
);

/// The explicit non-secret scene field projection. Never serialize a whole
/// profile: listener authentication and account-owned endpoints are global.
@immutable
class ScenarioSettings {
  ScenarioSettings(Map<String, Object?> value)
    : values = Map<String, Object?>.unmodifiable(
        scenarioMap(jsonDecode(jsonEncode(value))),
      );

  factory ScenarioSettings.fromProfile(UsqueProfile profile) =>
      ScenarioSettings({
        'frontends': profile.frontends.toMap(),
        'transport': profile.transport == TransportPolicy.automatic
            ? 'auto'
            : profile.transport.name,
        'ip_policy': profile.ipPolicy == IpPolicy.automatic
            ? 'auto'
            : _snake(profile.ipPolicy.name),
        'allow_lan': profile.allowLan,
        'routing': profile.routing.toMap(),
        'geo_direct_countries': profile.geoDirectCountries,
        'direct_dns': {
          ...profile.directDns.toMap(),
          'mode': _snake(profile.directDns.mode.name),
        },
        'warp_dns': profile.warpDns.toMap(),
        'dns_mode': _snake(profile.dnsMode.name),
        'dns_servers': [
          profile.dnsIpv4,
          profile.dnsIpv6,
        ].where((s) => s.isNotEmpty).toList(),
        'proxy_system_proxy': profile.proxy.systemProxy,
        'proxy_dns_mode': _snake(profile.proxy.dnsMode.name),
        'proxy_dns_servers': [
          profile.proxy.dnsIpv4,
          profile.proxy.dnsIpv6,
        ].where((s) => s.isNotEmpty).toList(),
        'vpn_gate': profile.vpnGate.toMap(),
        'chain_exit': profile.chainExit?.toMap(),
      });

  final Map<String, Object?> values;
  Map<String, Object?> toMap() => scenarioMap(jsonDecode(jsonEncode(values)));

  UsqueProfile toProfile(UsqueProfile base) {
    final dns = (values['dns_servers'] as List? ?? const []).cast<String>();
    final proxyDns = (values['proxy_dns_servers'] as List? ?? const [])
        .cast<String>();
    String address(List<String> ips, bool v6) =>
        ips.where((ip) => ip.contains(':') == v6).firstOrNull ?? '';
    return base.copyWith(
      frontends: FrontendSettings.fromMap(scenarioMap(values['frontends'])),
      transport: TransportPolicy.values.firstWhere(
        (v) =>
            v.name ==
            (values['transport'] == 'auto' ? 'automatic' : values['transport']),
        orElse: () => base.transport,
      ),
      ipPolicy: IpPolicy.values.firstWhere(
        (v) =>
            v.name ==
            (values['ip_policy'] == 'auto'
                ? 'automatic'
                : _camel(values['ip_policy'] as String? ?? 'auto')),
        orElse: () => base.ipPolicy,
      ),
      allowLan: values['allow_lan'] == true,
      routing: RoutingSettings.fromMap(scenarioMap(values['routing'])),
      geoDirectCountries: (values['geo_direct_countries'] as List? ?? const [])
          .cast<String>(),
      directDns: DirectDnsSettings.fromMap({
        ...scenarioMap(values['direct_dns']),
        'mode': _camel(
          scenarioMap(values['direct_dns'])['mode'] as String? ??
              'physical_system',
        ),
      }),
      warpDns: WarpDnsSettings.fromMap(scenarioMap(values['warp_dns'])),
      dnsMode: DnsMode.values.firstWhere(
        (v) => v.name == _camel(values['dns_mode'] as String? ?? 'tunnel'),
        orElse: () => base.dnsMode,
      ),
      dnsIpv4: address(dns, false),
      dnsIpv6: address(dns, true),
      proxy: base.proxy.copyWith(
        systemProxy: values['proxy_system_proxy'] == true,
        dnsMode: ProxyDnsMode.values.firstWhere(
          (v) =>
              v.name == _camel(values['proxy_dns_mode'] as String? ?? 'remote'),
          orElse: () => base.proxy.dnsMode,
        ),
        dnsIpv4: address(proxyDns, false),
        dnsIpv6: address(proxyDns, true),
      ),
      vpnGate: VpnGateSettings.fromMap(scenarioMap(values['vpn_gate'])),
      chainExit: values['chain_exit'] is Map
          ? ChainExitSettings.fromMap(values['chain_exit'] as Map)
          : null,
      clearChainExit: values['chain_exit'] == null,
    );
  }
}

@immutable
class ConnectionScenario {
  const ConnectionScenario({
    required this.id,
    required this.name,
    required this.revision,
    required this.settings,
    this.boundAccountId,
  });
  factory ConnectionScenario.fromMap(Map<String, Object?> map) =>
      ConnectionScenario(
        id: map['id'] as String,
        name: map['name'] as String,
        revision: map['revision'] as String,
        boundAccountId: map['bound_account_id'] as String?,
        settings: ScenarioSettings(scenarioMap(map['settings'])),
      );
  final String id, name, revision;
  final String? boundAccountId;
  final ScenarioSettings settings;
}

@immutable
class ScenarioCatalog {
  /// A status poll can omit unchanged templates; [unchanged] supplies the
  /// previously accepted templates for that same catalogue revision.
  ScenarioCatalog.fromMap(
    Map<String, Object?> value, {
    List<ConnectionScenario>? unchanged,
  }) : scenarios =
           unchanged ??
           List.unmodifiable(
             (value['scenarios'] as List? ?? const []).map(
               (v) => ConnectionScenario.fromMap(scenarioMap(v)),
             ),
           ),
       revision = value['catalogue_revision'] as String?,
       invalidIds = Set.unmodifiable(
         (value['invalid_scenario_ids'] as List? ?? const []).cast<String>(),
       ),
       selection = value['selection'] is Map
           ? scenarioMap(value['selection'])
           : null,
       currentSettings = value['current_settings'] is Map
           ? ScenarioSettings(scenarioMap(value['current_settings']))
           : null,
       currentSettingsError = value['current_settings_error'] as String?,
       currentAccountId = value['current_account_id'] as String?,
       appliedAccountId = value['applied_account_id'] as String?,
       appliedScenarioId = value['applied_scenario_id'] as String?,
       appliedSceneConfirmed = value['applied_scene_confirmed'] == true,
       sessionId = value['session_id'] as String?,
       state = scenarioMap(value['state']),
       canRestore = value['can_restore'] == true,
       lastOperationId = value['last_operation_id'] as String?;
  final List<ConnectionScenario> scenarios;
  final String? revision;

  /// Templates whose saved settings fail the current validation rules.
  final Set<String> invalidIds;
  final Map<String, Object?>? selection;
  final ScenarioSettings? currentSettings;
  final String? currentAccountId,
      appliedAccountId,
      lastOperationId,
      currentSettingsError;
  final Map<String, Object?> state;
  final bool canRestore;
  final String? appliedScenarioId, sessionId;
  final bool appliedSceneConfirmed;
  String? get selectedId => selection?['scenario_id'] as String?;
  ConnectionScenario? get selected =>
      scenarios.where((s) => s.id == selectedId).firstOrNull;
  bool get modified => state['modified'] == true;
  bool get templateUpdated => state['template_updated'] == true;
  bool get boundAccountMissing => state['bound_account_missing'] == true;
  bool? get appliedMatches => state['applied_matches'] as bool?;
  String get applyStatus => state['apply_status'] as String? ?? 'unknown';
}

@immutable
class ScenarioPreview {
  ScenarioPreview.fromMap(Map<String, Object?> map)
    : values = Map.unmodifiable(scenarioMap(map['preview'])),
      catalog = ScenarioCatalog.fromMap(map);
  final Map<String, Object?> values;
  final ScenarioCatalog catalog;
  String get token => values['preview_token'] as String? ?? '';
  String? get scenarioId => values['scenario_id'] as String?;
  String? get scenarioRevision => values['scenario_revision'] as String?;
  String? get accountId => values['account_id'] as String?;
  String? get previousAccountId => values['previous_account_id'] as String?;
  bool? get needsReconnect => values['needs_reconnect'] as bool?;
  String get applyKind => values['apply_kind'] as String? ?? 'unconfirmed';
  List<String> get actualChangedFields =>
      (values['actual_changed_fields'] as List? ?? const []).cast<String>();
  bool get accountChanged => values['account_changed'] == true;
  Map<String, Object?> get beforeSettings =>
      scenarioMap(values['before_settings']);
  Map<String, Object?> get targetSettings =>
      scenarioMap(values['target_settings'] ?? values['settings']);

  /// Kill Switch stays on while this scene turns off the VPN output.
  bool get killSwitchOutputRemoved =>
      values['kill_switch_output_removed'] == true;
  Map<String, Object?>? get appliedSettings => values['applied_settings'] is Map
      ? scenarioMap(values['applied_settings'])
      : null;
  bool get restore => values['restore'] == true;
  List<String> get changedFields =>
      (values['changed_fields'] as List? ?? const []).cast<String>();
}
