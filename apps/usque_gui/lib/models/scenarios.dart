import 'dart:convert';

import 'package:flutter/foundation.dart';

import 'app_models.dart';

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
/// profile: listener authentication and account credentials are excluded.
@immutable
class ScenarioSettings {
  static const managedFields = {
    'endpoint',
    'transport',
    'ip_policy',
    'vpn_gate',
    'chain_exit',
  };
  ScenarioSettings(Map<String, Object?> value)
    : values = Map<String, Object?>.unmodifiable(_settings(value));

  static Map<String, Object?> _settings(Map<String, Object?> value) {
    final endpoint = scenarioMap(value['endpoint']);
    const endpointFields = {'selection', 'ipv4', 'ipv6', 'port', 'sni'};
    if (value.length != managedFields.length ||
        !managedFields.containsAll(value.keys) ||
        !endpoint.keys.toSet().containsAll(endpointFields) ||
        !{...endpointFields, 'preset_region'}.containsAll(endpoint.keys) ||
        value['transport'] is! String ||
        value['ip_policy'] is! String ||
        value['vpn_gate'] is! Map ||
        value['chain_exit'] != null && value['chain_exit'] is! Map) {
      throw const FormatException('Invalid scene settings shape');
    }
    return scenarioMap(jsonDecode(jsonEncode(value)));
  }

  factory ScenarioSettings.fromProfile(UsqueProfile profile) =>
      ScenarioSettings({
        'endpoint': {
          'selection': profile.endpointSelection.name,
          'preset_region': profile.endpointPresetRegion?.name,
          'ipv4': profile.endpointIpv4,
          'ipv6': profile.endpointIpv6,
          'port': profile.endpointPort,
          'sni': profile.sni,
        },
        'transport': profile.transport == TransportPolicy.automatic
            ? 'auto'
            : profile.transport.name,
        'ip_policy': profile.ipPolicy == IpPolicy.automatic
            ? 'auto'
            : _snake(profile.ipPolicy.name),
        'vpn_gate': profile.vpnGate.toMap(),
        'chain_exit': profile.chainExit?.toMap(),
      });

  final Map<String, Object?> values;
  Map<String, Object?> toMap() => scenarioMap(jsonDecode(jsonEncode(values)));

  UsqueProfile toProfile(UsqueProfile base) {
    final endpoint = scenarioMap(values['endpoint']);
    return base.copyWith(
      endpointSelection: EndpointSelection.values.byName(
        endpoint['selection'] as String,
      ),
      endpointPresetRegion: endpoint['preset_region'] == null
          ? null
          : EndpointPresetRegion.values.byName(
              endpoint['preset_region'] as String,
            ),
      clearEndpointPresetRegion: endpoint['preset_region'] == null,
      endpointIpv4: endpoint['ipv4'] as String,
      endpointIpv6: endpoint['ipv6'] as String,
      endpointPort: endpoint['port'] as int,
      sni: endpoint['sni'] as String,
      transport: TransportPolicy.values.firstWhere(
        (v) =>
            v.name ==
            (values['transport'] == 'auto' ? 'automatic' : values['transport']),
      ),
      ipPolicy: IpPolicy.values.firstWhere(
        (v) =>
            v.name ==
            (values['ip_policy'] == 'auto'
                ? 'automatic'
                : _camel(values['ip_policy'] as String)),
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
       runtimeObserved = value['runtime_observed'] != false,
       appliedAccountId = value['runtime_observed'] == false
           ? null
           : value['applied_account_id'] as String?,
       appliedScenarioId = value['runtime_observed'] == false
           ? null
           : value['applied_scenario_id'] as String?,
       appliedSceneConfirmed =
           value['runtime_observed'] != false &&
           value['applied_scene_confirmed'] == true,
       sourceEpoch = value['runtime_observed'] == false
           ? ''
           : value['source_epoch'] as String,
       sessionId = value['runtime_observed'] == false
           ? null
           : value['session_id'] as String?,
       observationSessionId = _observationSession(value),
       state = value['runtime_observed'] == false
           ? _savedDrift(value['state'])
           : scenarioMap(value['state']),
       canRestore =
           value['runtime_observed'] != false && value['can_restore'] == true,
       lastOperationId = value['runtime_observed'] == false
           ? null
           : value['last_operation_id'] as String?;
  final List<ConnectionScenario> scenarios;
  final String? revision;

  /// A saved-configuration read carries only drift computed from saved
  /// values. It never carries an application result.
  static Map<String, Object?> _savedDrift(Object? value) {
    final state = scenarioMap(value);
    return Map.unmodifiable({
      'apply_status': 'unknown',
      for (final key in const [
        'modified',
        'template_updated',
        'bound_account_missing',
      ])
        if (state[key] == true) key: true,
    });
  }

  static String? _observationSession(Map<String, Object?> value) {
    if (value['runtime_observed'] == false) return null;
    if (!value.containsKey('observation_session_id')) {
      throw const FormatException('Missing scene observation identity');
    }
    return value['observation_session_id'] as String?;
  }

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
  final bool runtimeObserved;
  final String sourceEpoch;
  final String? appliedScenarioId, sessionId;

  /// Native settings observation identity. Android keeps its generation even
  /// when the public scene session is absent after disconnecting.
  final String? observationSessionId;
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
      scenarioMap(values['target_settings']);

  /// Saved global output changes stop VPN protection during this switch.
  bool get killSwitchOutputRemoved =>
      values['kill_switch_output_removed'] == true;
  Map<String, Object?>? get appliedSettings => values['applied_settings'] is Map
      ? scenarioMap(values['applied_settings'])
      : null;
  bool get restore => values['restore'] == true;

  /// Returning to Custom: no scene, with Usque's default scene settings.
  bool get reset => values['reset'] == true;
  List<String> get changedFields =>
      (values['changed_fields'] as List? ?? const []).cast<String>();
}
