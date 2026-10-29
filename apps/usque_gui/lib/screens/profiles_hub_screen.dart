import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/chain_strings.dart';
import '../core/proton_strings.dart';
import '../models/app_models.dart';
import '../models/scenarios.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/proton_vpn_panel.dart' show loadCachedProtonServers;
import '../widgets/usque_dialog.dart';
import 'profiles_screen.dart' show AccountsScreen;
import 'scenario_editor_screen.dart';

class ProfilesScreen extends StatefulWidget {
  const ProfilesScreen({
    required this.controller,
    this.gateObservation = false,
    super.key,
  });
  final bool gateObservation;
  final AppController controller;
  @override
  State<ProfilesScreen> createState() => _ProfilesScreenState();
}

class _ProfilesScreenState extends State<ProfilesScreen> {
  AppController get app => widget.controller;
  Map<String, String> _exitNames = {};
  String? _labelsKey;
  int _labelGeneration = 0;

  void _refreshExitNames() {
    final scenes =
        app.scenarios.catalog?.scenarios ?? const <ConnectionScenario>[];
    final refs =
        scenes
            .map((scene) => jsonEncode(scene.settings.values['chain_exit']))
            .toList()
          ..sort();
    final key = '${app.chainLibraryRevision}:${refs.join('|')}';
    if (_labelsKey == key || scenes.isEmpty) return;
    _labelsKey = key;
    final generation = ++_labelGeneration;
    unawaited(() async {
      final names = await scenarioExitNames(
        app,
        settings: scenes.map((scene) => scene.settings.values),
      );
      if (mounted && generation == _labelGeneration) {
        setState(() => _exitNames = names);
      }
    }());
  }

  @override
  void initState() {
    super.initState();
    app.addListener(_changed);
    if (app.scenarios.supported) {
      unawaited(app.scenarios.refresh());
      _refreshExitNames();
    }
  }

  void _changed() {
    if (!mounted ||
        widget.gateObservation &&
            (app.section != AppSection.profiles || !app.uiVisible)) {
      return;
    }
    setState(() {});
    _refreshExitNames();
  }

  @override
  void dispose() {
    app.removeListener(_changed);
    super.dispose();
  }

  Future<void> _edit({
    ConnectionScenario? scene,
    bool duplicate = false,
  }) async {
    await Navigator.of(context).push<void>(
      MaterialPageRoute(
        builder: (_) => ScenarioEditorScreen(
          controller: app,
          scene: scene,
          duplicate: duplicate,
        ),
      ),
    );
  }

  Future<void> _remove(ConnectionScenario scene) async {
    final strings = app.strings;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: LucideIcons.trash2,
        title: strings.get('scenario_delete_title'),
        subtitle: scene.name,
        content: Text(strings.get('scenario_delete_body')),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(strings.get('delete')),
          ),
        ],
      ),
    );
    if (confirmed == true) await app.scenarios.delete(scene);
  }

  Future<void> _update(ConnectionScenario scene) async {
    final catalog = app.scenarios.catalog;
    final current = catalog?.currentSettings;
    if (current == null || catalog?.currentAccountId == null) return;
    final binding = scene.boundAccountId == null
        ? null
        : catalog!.currentAccountId;
    final strings = app.strings;
    String account(String? id) =>
        app.profiles.where((p) => p.id == id).firstOrNull?.name ??
        strings.get('scenario_reference_missing');
    final fields = current.values.keys
        .where(
          (key) =>
              jsonEncode(current.values[key]) !=
              jsonEncode(scene.settings.values[key]),
        )
        .toList();
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: LucideIcons.save,
        title: strings.get('scenario_update_current'),
        subtitle: scene.name,
        content: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                binding == null
                    ? strings.get('scenario_follow_account')
                    : '${strings.get('scenario_bound_account')}: ${account(scene.boundAccountId)} → ${account(binding)}',
              ),
              for (final field in fields)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: Text(
                    '${scenarioFieldLabel(strings, field)}: ${scenarioValueLabel(strings, scene.settings.values, field)} → ${scenarioValueLabel(strings, current.values, field)}',
                  ),
                ),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(strings.get('scenario_save')),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    await app.scenarios.save(
      previous: scene,
      name: scene.name,
      boundAccountId: binding,
      settings: current,
    );
  }

  Widget _scene(ConnectionScenario scene) {
    final strings = app.strings;
    final state = app.scenarios.catalog;
    final selected = state?.selectedId == scene.id;
    final profile = scene.settings.toProfile(app.activeProfile);
    final account = app.profiles
        .where((a) => a.id == scene.boundAccountId)
        .firstOrNull;
    final summary = [
      if (profile.frontends.tunnel)
        strings.tunnelOutputLabel(Theme.of(context).platform),
      if (profile.frontends.socks5) 'SOCKS5',
      if (profile.frontends.http) 'HTTP',
      profile.chainEnabled
          ? scenarioValueLabel(
              strings,
              scene.settings.values,
              profile.chainSource == ChainSource.vpnGate
                  ? 'vpn_gate'
                  : 'chain_exit',
              exitNames: _exitNames,
            )
          : 'WARP',
    ].join(' · ');
    final actions = Wrap(
      spacing: 4,
      runSpacing: 4,
      children: [
        TextButton(
          onPressed: app.scenarios.busy ? null : () => _edit(scene: scene),
          child: Text(strings.get('edit')),
        ),
        TextButton(
          key: ValueKey('scenario-use-${scene.id}'),
          onPressed: app.scenarios.busy
              ? null
              : () => previewAndUseScenario(context, app, scene),
          child: Text(strings.get('scenario_use')),
        ),
        PopupMenuButton<String>(
          enabled: !app.scenarios.busy,
          onSelected: (value) {
            switch (value) {
              case 'duplicate':
                unawaited(_edit(scene: scene, duplicate: true));
              case 'update':
                unawaited(_update(scene));
              case 'delete':
                unawaited(_remove(scene));
            }
          },
          itemBuilder: (_) => [
            PopupMenuItem(
              value: 'duplicate',
              child: Text(strings.get('scenario_duplicate')),
            ),
            PopupMenuItem(
              value: 'update',
              child: Text(strings.get('scenario_update_current')),
            ),
            PopupMenuItem(value: 'delete', child: Text(strings.get('delete'))),
          ],
        ),
      ],
    );
    final details = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Wrap(
          spacing: 8,
          runSpacing: 4,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            Text(scene.name, style: Theme.of(context).textTheme.titleMedium),
            if (selected)
              InlineStatus(
                label: strings.get(scenarioStateKey(app)),
                tone: state?.modified == true
                    ? StatusTone.warning
                    : StatusTone.brand,
              ),
            if (selected && state?.templateUpdated == true)
              InlineStatus(
                label: strings.get('scenario_template_updated'),
                tone: StatusTone.warning,
              ),
          ],
        ),
        const SizedBox(height: 6),
        Text(summary),
        HintText(
          scene.boundAccountId == null
              ? strings.get('scenario_follow_account')
              : '${strings.get('scenario_bound_account')}: ${account?.name ?? strings.get('scenario_reference_missing')}',
        ),
        if (selected &&
            scene.boundAccountId != null &&
            scene.boundAccountId != state?.currentAccountId)
          HintText(strings.get('scenario_binding_modified')),
      ],
    );
    return Material(
      color: selected
          ? Theme.of(context).colorScheme.primaryContainer.withValues(alpha: .3)
          : Colors.transparent,
      borderRadius: BorderRadius.circular(12),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 16),
        child: LayoutBuilder(
          builder: (context, constraints) {
            if (constraints.maxWidth < 560 ||
                MediaQuery.textScalerOf(context).scale(14) > 21) {
              return Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  details,
                  const SizedBox(height: 8),
                  Align(
                    alignment: AlignmentDirectional.centerEnd,
                    child: actions,
                  ),
                ],
              );
            }
            return Row(
              children: [
                Expanded(child: details),
                const SizedBox(width: 12),
                actions,
              ],
            );
          },
        ),
      ),
    );
  }

  Widget _summary() {
    final strings = app.strings, state = app.scenarios.catalog;
    final accountId = state?.currentAccountId ?? app.activeProfileId;
    final account = app.profiles.where((a) => a.id == accountId).firstOrNull;
    final appliedAccountId = state?.appliedAccountId;
    return Wrap(
      spacing: 20,
      runSpacing: 4,
      children: [
        TextButton(
          onPressed: () => app.selectProfilesTab(ProfilesTab.accounts),
          child: Text(
            '${strings.get('scenario_current_account')}: ${account?.name ?? strings.get('not_available')}',
          ),
        ),
        TextButton(
          onPressed: () => app.selectProfilesTab(ProfilesTab.scenes),
          child: Text(
            '${strings.get('scenario_current')}: ${state?.selected?.name ?? strings.get('scenario_custom')}',
          ),
        ),
        if (state?.sessionId != null || app.snapshot.isConnected)
          Text(
            '${strings.chain('current')}: ${app.profiles.where((a) => a.id == appliedAccountId).firstOrNull?.name ?? strings.get('not_available')} · ${state?.appliedSceneConfirmed == true ? state?.scenarios.where((scene) => scene.id == state.appliedScenarioId).firstOrNull?.name ?? strings.get('scenario_custom') : strings.get('scenario_unknown')}',
          ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final strings = app.strings, scenes = app.scenarios, state = scenes.catalog;
    final selected = scenes.supported ? app.profilesTab : ProfilesTab.accounts;
    return PageFrame(
      title: strings.get('profiles'),
      subtitle: strings.get('profiles_subtitle'),
      contentWidth: 880,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (scenes.supported) _summary(),
          const SizedBox(height: 8),
          SegmentedButton<ProfilesTab>(
            segments: [
              ButtonSegment(
                value: ProfilesTab.scenes,
                label: Text(strings.get('scenario_scenes')),
                enabled: scenes.supported,
              ),
              ButtonSegment(
                value: ProfilesTab.accounts,
                label: Text(strings.get('scenario_accounts')),
              ),
            ],
            selected: {selected},
            onSelectionChanged: (values) => app.selectProfilesTab(values.first),
          ),
          const SizedBox(height: 20),
          if (!scenes.supported)
            Padding(
              padding: const EdgeInsets.only(bottom: 16),
              child: HintText(strings.get('scenario_unsupported')),
            ),
          if (state?.currentSettingsError != null)
            WarningBanner(
              title: null,
              message: strings.get(
                scenarioErrorKey(state!.currentSettingsError),
              ),
            ),
          if (selected == ProfilesTab.accounts)
            AccountsScreen(controller: app, embedded: true)
          else ...[
            if (scenes.errorCode != null ||
                scenes.unconfirmed ||
                state?.applyStatus == 'failed')
              WarningBanner(
                title: null,
                message: strings.get(
                  scenes.unconfirmed
                      ? 'scenario_unknown'
                      : state?.applyStatus == 'failed'
                      ? 'scenario_failed'
                      : scenarioErrorKey(scenes.errorCode),
                ),
              ),
            Wrap(
              alignment: WrapAlignment.end,
              spacing: 8,
              runSpacing: 8,
              children: [
                IconButton(
                  tooltip: strings.get('settings_refresh'),
                  onPressed: scenes.busy ? null : scenes.refresh,
                  icon: const Icon(LucideIcons.refreshCw),
                ),
                if (state?.canRestore == true)
                  TextButton(
                    onPressed: scenes.busy
                        ? null
                        : () => previewAndUseScenario(context, app, null),
                    child: Text(strings.get('scenario_restore')),
                  ),
                FilledButton.icon(
                  key: const ValueKey('scenario-create'),
                  onPressed: scenes.busy || state?.currentSettings == null
                      ? null
                      : () => _edit(),
                  icon: const Icon(LucideIcons.plus),
                  label: Text(strings.get('scenario_create')),
                ),
              ],
            ),
            const SizedBox(height: 16),
            if (state == null)
              const Center(child: CircularProgressIndicator())
            else if (state.scenarios.isEmpty)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 32),
                child: Column(
                  children: [
                    Text(
                      strings.get('scenario_empty'),
                      style: Theme.of(context).textTheme.titleLarge,
                    ),
                    const SizedBox(height: 8),
                    Text(strings.get('scenario_empty_hint')),
                  ],
                ),
              )
            else
              ContentList(children: state.scenarios.map(_scene).toList()),
            if (scenes.busy) const LinearProgressIndicator(),
          ],
        ],
      ),
    );
  }
}

String scenarioStateKey(AppController app) {
  final state = app.scenarios.catalog;
  if (app.scenarios.unconfirmed) return 'scenario_unknown';
  if (state?.modified == true) return 'scenario_modified';
  if (state?.applyStatus == 'failed') return 'scenario_failed';
  if (state?.appliedMatches == true) return 'scenario_applied';
  if (state?.applyStatus == 'applying' || state?.applyStatus == 'deferred') {
    return 'scenario_pending';
  }
  if ((state?.applyStatus == 'unknown' || app.scenarios.queryUnconfirmed) &&
      app.snapshot.isConnected) {
    return 'scenario_unknown';
  }
  return 'scenario_selected';
}

final _scenePreviewLocks = Expando<bool>();
Future<bool> previewAndUseScenario(
  BuildContext context,
  AppController app,
  ConnectionScenario? scene,
) async {
  if (_scenePreviewLocks[app] == true) return false;
  _scenePreviewLocks[app] = true;
  try {
    return await _previewAndUseScenario(context, app, scene);
  } finally {
    _scenePreviewLocks[app] = false;
  }
}

Future<bool> _previewAndUseScenario(
  BuildContext context,
  AppController app,
  ConnectionScenario? scene,
) async {
  final route = ModalRoute.of(context);
  bool canPresent() =>
      app.section == AppSection.profiles && (route?.isCurrent ?? true);
  final preview = scene == null
      ? await app.scenarios.previewRestore()
      : await app.scenarios.preview(scene);
  if (preview == null || !context.mounted || !canPresent()) return false;
  final strings = app.strings;
  final exitNames = await scenarioExitNames(
    app,
    settings: [
      preview.appliedSettings ?? preview.beforeSettings,
      preview.targetSettings,
    ],
  );
  if (!context.mounted || !canPresent()) return false;
  final managedFields = {
    ...preview.changedFields,
    ...preview.actualChangedFields.where(_managedScenarioFields.contains),
  };
  final globalFields = preview.actualChangedFields
      .where((field) => !_managedScenarioFields.contains(field))
      .toSet();
  String account(String? id) =>
      app.profiles.where((a) => a.id == id).firstOrNull?.name ??
      strings.get('scenario_reference_missing');
  final confirmed = await showDialog<bool>(
    context: context,
    builder: (context) => UsqueDialog(
      icon: LucideIcons.layers3,
      title: strings.get(
        preview.restore ? 'scenario_restore' : 'scenario_preview',
      ),
      subtitle: scene?.name,
      content: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              preview.accountChanged
                  ? '${strings.get('scenario_account_change')}: ${account(preview.previousAccountId)} → ${account(preview.accountId)}'
                  : strings.get('scenario_keep_account'),
            ),
            const SizedBox(height: 16),
            if (managedFields.isEmpty && globalFields.isEmpty)
              Text(strings.get('scenario_no_changes')),
            for (final field in managedFields)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 5),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(scenarioFieldLabel(strings, field)),
                    Text(
                      '${scenarioValueLabel(strings, preview.appliedSettings ?? preview.beforeSettings, field, exitNames: exitNames)} → ${scenarioValueLabel(strings, preview.targetSettings, field, exitNames: exitNames)}',
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ],
                ),
              ),
            if (scenarioMap(preview.targetSettings['chain_exit'])['source'] ==
                    'proton_vpn' &&
                scenarioMap(
                      (preview.appliedSettings ??
                          preview.beforeSettings)['chain_exit'],
                    )['proton_server_id'] !=
                    scenarioMap(
                      preview.targetSettings['chain_exit'],
                    )['proton_server_id'])
              Text(
                '${strings.proton('selected_server')} · ${strings.get('scenario_modified')}',
              ),
            if (globalFields.isNotEmpty) ...[
              const SizedBox(height: 16),
              ContentHeading(title: strings.get('advanced')),
              for (final field in globalFields)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 6),
                  child: Text(scenarioFieldLabel(strings, field)),
                ),
            ],
            const SizedBox(height: 16),
            HintText(
              strings.get(
                preview.applyKind == 'select'
                    ? 'scenario_disconnected_hint'
                    : preview.needsReconnect == null
                    ? 'scenario_unknown'
                    : preview.needsReconnect == true
                    ? 'scenario_will_reconnect'
                    : 'scenario_new_flows',
              ),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context, false),
          child: Text(strings.get('cancel')),
        ),
        FilledButton(
          key: const ValueKey('scenario-confirm-use'),
          onPressed: () => Navigator.pop(context, true),
          child: Text(strings.get('scenario_confirm_use')),
        ),
      ],
    ),
  );
  return confirmed == true ? app.scenarios.activate(preview) : false;
}

String scenarioFieldLabel(AppStrings strings, String field) =>
    strings.get(switch (field) {
      'account_id' || 'active_profile_id' => 'scenario_account_change',
      'frontends' => 'scenario_output',
      'frontends.tunnel' => 'tunnel_output',
      'frontends.socks5' => 'socks_listener',
      'frontends.http' => 'http_listener',
      'transport' => 'transport',
      'data_plane' => 'mode',
      'tls_fingerprint' => 'tls_fingerprint',
      'congestion_control' => 'nq_congestion',
      'endpoint.ipv4' => 'endpoint_ipv4',
      'endpoint.ipv6' => 'endpoint_ipv6',
      'endpoint.port' => 'port',
      'endpoint.sni' => 'sni',
      'endpoint.selection' => 'endpoint_selection',
      'endpoint.preset_region' => 'endpoint_section',
      'ip_policy' => 'ip_policy',
      'mtu' => 'mtu',
      'allow_lan' => 'allow_lan',
      'disable_quic' => 'disable_quic',
      'kill_switch' => 'kill_switch',
      'auto_connect' => 'auto_connect',
      'split_exclusions' || 'bypass_domains' => 'routing_protection',
      'proxy.socks5_listeners' => 'socks_listener',
      'proxy.http_listeners' => 'http_listener',
      'proxy.udp_idle_timeout_seconds' => 'scenario_connection',
      'routing' => 'routing_protection',
      'geo_direct_countries' => 'bypass_countries',
      'direct_dns' => 'nq_direct_dns',
      'warp_dns' => 'warp_dns_type',
      'dns_mode' ||
      'dns_servers' ||
      'proxy_dns_mode' ||
      'proxy_dns_servers' ||
      'proxy.dns_mode' ||
      'proxy.dns_servers' => 'dns',
      'proxy_system_proxy' || 'proxy.system_proxy' => 'home_system_proxy',
      'vpn_gate' || 'chain_exit' => 'scenario_exit',
      _ => 'scenario_connection',
    });

const _managedScenarioFields = {
  'frontends',
  'frontends.tunnel',
  'frontends.socks5',
  'frontends.http',
  'transport',
  'ip_policy',
  'allow_lan',
  'routing',
  'geo_direct_countries',
  'direct_dns',
  'warp_dns',
  'dns_mode',
  'dns_servers',
  'proxy_system_proxy',
  'proxy_dns_mode',
  'proxy_dns_servers',
  'proxy.system_proxy',
  'proxy.dns_mode',
  'proxy.dns_servers',
  'vpn_gate',
  'chain_exit',
};

final _protonNameCaches = Expando<_ScenarioProtonNames>();

class _ScenarioProtonNames {
  final requests = <String, Future<Map<String, String>>>{};
  Future<Map<String, String>> get(
    AppController app,
    String profileId,
    String revision,
  ) {
    final key = '$profileId:$revision:${app.chainLibraryRevision}';
    final existing = requests[key];
    if (existing != null) return existing;
    while (requests.length >= 32) {
      requests.remove(requests.keys.first);
    }
    final result = () async {
      final nodes = await loadCachedProtonServers(
        app,
        profileId,
        isCurrent: () => app.uiVisible,
      );
      return <String, String>{
        for (final node in nodes)
          if (node['id'] is String && node['name'] is String)
            'proton:$profileId:${node['id']}': node['name'] as String,
      };
    }();
    requests[key] = result;
    unawaited(
      result.then<void>(
        (value) {
          if (value.isEmpty) requests.remove(key);
        },
        onError: (Object _) {
          requests.remove(key);
        },
      ),
    );
    return result;
  }
}

Future<Map<String, String>> scenarioExitNames(
  AppController app, {
  Iterable<Map<String, Object?>> settings = const [],
}) async {
  final names = <String, String>{};
  try {
    final result = await app.chainProfile({'action': 'list'});
    if (result.error == null) {
      names.addEntries(
        result.profiles.map((entry) => MapEntry(entry.id, entry.name)),
      );
    }
  } on Object {
    // An unavailable local library does not change the selected exit or routes.
  }
  final cache = _protonNameCaches[app] ??= _ScenarioProtonNames();
  final profiles = <String, String>{};
  for (final value in settings) {
    final exit = scenarioMap(value['chain_exit']);
    if (exit['source'] == 'proton_vpn' &&
        exit['profile_id'] is String &&
        exit['revision'] is String &&
        exit['proton_server_id'] is String &&
        (exit['proton_server_id'] as String).isNotEmpty) {
      profiles[exit['profile_id'] as String] = exit['revision'] as String;
    }
  }
  for (final profile in profiles.entries) {
    try {
      names.addAll(await cache.get(app, profile.key, profile.value));
    } on Object {
      /* Keep a visible unavailable label; never show a raw server ID. */
    }
  }
  return names;
}

String scenarioErrorKey(String? code) => switch (code) {
  'SCENARIO_ACCOUNT_UNAVAILABLE' ||
  'SCENARIO_DEPENDENCY_UNAVAILABLE' ||
  'SCENARIO_ACCOUNT_MISSING' => 'scenario_reference_missing',
  'SCENARIO_VPN_PERMISSION_REQUIRED' => 'scenario_vpn_permission',
  'SCENARIO_RULES_UNAVAILABLE' => 'scenario_rules_missing',
  'SCENARIO_UNCONFIRMED' ||
  'SCENARIO_COMMIT_UNCONFIRMED' ||
  'SCENARIO_RESULT_UNKNOWN' => 'scenario_unknown',
  'SCENARIO_PLATFORM_UNSUPPORTED' => 'scenario_platform_unsupported',
  'SCENARIO_LIMIT' => 'scenario_limit',
  'SCENARIOS_UNSUPPORTED' => 'scenario_unsupported',
  _ => 'scenario_save_failed',
};

String scenarioValueLabel(
  AppStrings strings,
  Map<String, Object?> settings,
  String field, {
  Map<String, String> exitNames = const {},
}) {
  Object? value;
  if (field.startsWith('proxy.')) {
    value = settings[field.replaceFirst('proxy.', 'proxy_')];
  } else if (field.startsWith('frontends.')) {
    value = scenarioMap(settings['frontends'])[field.split('.').last];
  } else {
    value = settings[field];
  }
  if (value == null) return strings.get('not_available');
  if (value is bool) return strings.get(value ? 'on' : 'off');
  if (value is List) return value.isEmpty ? '—' : value.join(', ');
  if (value is Map) {
    if (field == 'routing') {
      return '${strings.get('scenario_rules_count').replaceAll('{count}', '${(value['rules'] as List? ?? []).length}')} · ${strings.get('routing_ads')}: ${strings.get(value['ads_enabled'] == true ? 'on' : 'off')}';
    }
    if (field == 'chain_exit' || field == 'vpn_gate') {
      if (value['enabled'] != true) return strings.get('off');
      if (field == 'vpn_gate') return 'VPN Gate';
      final source = value['source'] as String?;
      final label =
          ChainSource.values
              .where((item) => item.wire == source)
              .firstOrNull
              ?.label ??
          strings.get('not_available');
      final name = exitNames[value['profile_id']];
      final base = name == null ? label : '$label · $name';
      final serverId = value['proton_server_id'];
      if (source == 'proton_vpn' && serverId is String) {
        final serverName = serverId.isEmpty
            ? strings.proton('automatic_server')
            : exitNames['proton:${value['profile_id']}:$serverId'] ??
                  strings.get('not_available');
        return '$base · $serverName';
      }
      return base;
    }
    if (value.containsKey('mode')) {
      return [
        value['mode'],
        value['server_name'],
      ].whereType<String>().where((v) => v.isNotEmpty).join(' · ');
    }
    return strings.get('scenario_output');
  }
  return switch (value) {
    'auto' => strings.get('automatic'),
    'prefer_ipv4' => strings.get('prefer_ipv4'),
    'prefer_ipv6' => strings.get('prefer_ipv6'),
    'ipv4_only' => strings.get('ipv4_only'),
    'ipv6_only' => strings.get('ipv6_only'),
    'tunnel' || 'remote' => strings.get('proxy_dns_remote'),
    'local_configured' => strings.get('proxy_dns_configured'),
    'system' => strings.get('proxy_dns_system'),
    'edge_resolved' => strings.get('proxy_dns_edge_resolved'),
    'http3' => strings.get('http3'),
    'http2' => strings.get('http2'),
    _ => value.toString(),
  };
}

class ScenarioHomeLink extends StatelessWidget {
  const ScenarioHomeLink({required this.controller, super.key});
  final AppController controller;
  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: controller.scenarios,
    builder: (context, _) {
      if (!controller.scenarios.supported) return const SizedBox.shrink();
      final strings = controller.strings;
      return Align(
        alignment: AlignmentDirectional.centerStart,
        child: TextButton.icon(
          key: const ValueKey('home-scene-link'),
          onPressed: () => controller.openProfiles(ProfilesTab.scenes),
          icon: const Icon(LucideIcons.layers3, size: 18),
          label: Text(
            '${strings.get('scenario_current')}: ${controller.scenarios.catalog?.selected?.name ?? strings.get('scenario_custom')}',
          ),
        ),
      );
    },
  );
}
