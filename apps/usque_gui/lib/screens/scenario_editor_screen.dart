import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/country_strings.dart';
import '../core/iso_countries.dart';
import '../core/proton_strings.dart';
import '../models/app_models.dart';
import '../models/scenarios.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/country_flag.dart';
import '../widgets/direct_dns_editor.dart';
import '../widgets/proton_vpn_panel.dart';
import '../widgets/routing_rules_editor.dart';
import '../widgets/save_changes_bar.dart';
import '../widgets/unsaved_changes_guard.dart';
import '../widgets/usque_dialog.dart';
import '../widgets/warp_dns_editor.dart';
import 'profiles_hub_screen.dart' show previewAndUseScenario, scenarioErrorKey;

class ScenarioEditorScreen extends StatefulWidget {
  const ScenarioEditorScreen({
    required this.controller,
    this.scene,
    this.duplicate = false,
    super.key,
  });
  final AppController controller;
  final ConnectionScenario? scene;
  final bool duplicate;
  @override
  State<ScenarioEditorScreen> createState() => _ScenarioEditorScreenState();
}

class _ScenarioEditorScreenState extends State<ScenarioEditorScreen> {
  AppController get app => widget.controller;
  final _form = GlobalKey<FormState>();
  late final TextEditingController _name, _countries, _dns, _proxyDns;
  late ScenarioSettings _baseline;
  late UsqueProfile _draft;
  String? _binding;
  ConnectionScenario? _savedScene;
  bool _dirty = false, _saving = false, _loading = false;
  String? _error;
  final _changed = <String>{};
  List<ChainProfileSummary> _exits = [];
  List<VpnGateServer> _gates = [];

  @override
  void initState() {
    super.initState();
    _savedScene = widget.duplicate ? null : widget.scene;
    _baseline =
        widget.scene?.settings ?? app.scenarios.catalog!.currentSettings!;
    _draft = _baseline.toProfile(app.activeProfile);
    _binding = widget.scene?.boundAccountId;
    _name = TextEditingController(text: widget.scene?.name ?? '');
    _countries = TextEditingController(
      text: _draft.geoDirectCountries.join(', '),
    );
    _dns = TextEditingController(
      text: (_baseline.values['dns_servers'] as List? ?? []).join(', '),
    );
    _proxyDns = TextEditingController(
      text: (_baseline.values['proxy_dns_servers'] as List? ?? []).join(', '),
    );
    _dirty = _savedScene == null;
    unawaited(_loadExits());
    unawaited(app.refreshGeoRules());
  }

  @override
  void dispose() {
    for (final c in [_name, _countries, _dns, _proxyDns]) {
      c.dispose();
    }
    super.dispose();
  }

  void _edit(UsqueProfile value, Iterable<String> fields) {
    setState(() {
      _draft = value;
      _changed.addAll(fields);
      _dirty = true;
      _error = null;
    });
  }

  List<String> _addresses(String value) =>
      value.split(RegExp(r'[,\s]+')).where((v) => v.isNotEmpty).toList();
  ScenarioSettings _settings() {
    final result = _baseline.toMap(),
        projection = ScenarioSettings.fromProfile(_draft).toMap();
    for (final field in _changed) {
      result[field] = projection[field];
    }
    if (_changed.contains('geo_direct_countries')) {
      result['geo_direct_countries'] =
          _countries.text
              .toUpperCase()
              .split(RegExp(r'[,\s]+'))
              .where((c) => c.isNotEmpty)
              .toSet()
              .toList()
            ..sort();
    }
    if (_changed.contains('dns_servers')) {
      result['dns_servers'] = _addresses(_dns.text);
    }
    if (_changed.contains('proxy_dns_servers')) {
      result['proxy_dns_servers'] = _addresses(_proxyDns.text);
    }
    return ScenarioSettings(result);
  }

  Future<void> _loadExits() async {
    setState(() => _loading = true);
    try {
      final result = await app.chainProfile({'action': 'list'});
      if (!mounted) return;
      if (result.error != null) {
        throw const FormatException('Exit catalogue unavailable');
      }
      final gates = <VpnGateServer>[];
      var offset = 0;
      if (app.engineCapabilities?.vpnGatePoolFavorites == true) {
        do {
          final page = await app.listVpnGate(
            favoritesOnly: true,
            offset: offset,
            limit: 50,
          );
          if (!mounted) return;
          gates.addAll(page.servers);
          offset += page.servers.length;
          if (page.servers.isEmpty || offset >= page.total || offset >= 5000) {
            break;
          }
        } while (true);
      }
      setState(() {
        _exits = result.profiles;
        _gates = gates;
      });
    } on Object {
      if (mounted && _draft.chainEnabled) {
        setState(() => _error = app.strings.get('scenario_reference_missing'));
      }
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<void> _save({bool use = false}) async {
    if (_saving || !(_form.currentState?.validate() ?? false)) return;
    try {
      _draft.routing.validate();
    } on RoutingRuleError catch (error) {
      setState(() => _error = app.strings.get(error.key));
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });
    final settings = _settings();
    final result = await app.scenarios.save(
      previous: _savedScene,
      name: _name.text.trim(),
      boundAccountId: _binding,
      settings: settings,
    );
    if (!mounted) return;
    setState(() {
      _saving = false;
      if (result != null) {
        _savedScene = result;
        _baseline = result.settings;
        _draft = result.settings.toProfile(app.activeProfile);
        _name.text = result.name;
        _binding = result.boundAccountId;
        _countries.text = _draft.geoDirectCountries.join(', ');
        _dns.text = (result.settings.values['dns_servers'] as List? ?? []).join(
          ', ',
        );
        _proxyDns.text =
            (result.settings.values['proxy_dns_servers'] as List? ?? []).join(
              ', ',
            );
        _changed.clear();
        _dirty = false;
      } else {
        _error = app.strings.get(scenarioErrorKey(app.scenarios.errorCode));
      }
    });
    if (result != null && use && mounted) {
      await previewAndUseScenario(context, app, result);
    }
  }

  String get _exitKey {
    if (!_draft.chainEnabled) return 'none';
    if (_draft.chainSource == ChainSource.vpnGate) {
      return 'gate:${_draft.vpnGate.serverId}:${_draft.vpnGate.configSha256}';
    }
    return 'chain:${_draft.chainExit?.profileId}:${_draft.chainExit?.revision}';
  }

  void _chooseExit(String? value) {
    if (value == null) return;
    if (value == 'none') {
      _edit(
        _draft.copyWith(
          chainExit: _draft.chainExit?.copyWith(enabled: false),
          vpnGate: _draft.vpnGate.copyWith(enabled: false),
        ),
        ['chain_exit', 'vpn_gate'],
      );
      return;
    }
    final exit = _exits
        .where((e) => 'chain:${e.id}:${e.revision}' == value)
        .firstOrNull;
    if (exit != null) {
      _edit(
        _draft.copyWith(
          chainExit: ChainExitSettings(
            enabled: true,
            source: exit.source,
            profileId: exit.id,
            revision: exit.revision,
            protonServerId: exit.source == ChainSource.protonVpn ? '' : null,
          ),
          vpnGate: _draft.vpnGate.copyWith(enabled: false),
        ),
        ['chain_exit', 'vpn_gate'],
      );
      return;
    }
    final gate = _gates
        .where((g) => 'gate:${g.id}:${g.configSha256}' == value)
        .firstOrNull;
    if (gate != null) {
      _edit(
        _draft.copyWith(
          chainExit: const ChainExitSettings(
            enabled: true,
            source: ChainSource.vpnGate,
          ),
          vpnGate: VpnGateSettings(
            enabled: true,
            serverId: gate.id,
            configSha256: gate.configSha256,
          ),
        ),
        ['chain_exit', 'vpn_gate'],
      );
    }
  }

  Future<void> _chooseCountries() async {
    final search = TextEditingController();
    final selected = _draft.geoDirectCountries.toSet();
    final s = app.strings;
    final route = DialogRoute<Set<String>>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, update) {
          final query = search.text.trim().toLowerCase();
          final countries = kIsoCountries
              .where(
                (c) =>
                    c.code.toLowerCase().contains(query) ||
                    c.name.toLowerCase().contains(query) ||
                    s.countryName(c.code).toLowerCase().contains(query),
              )
              .toList();
          return UsqueDialog(
            icon: LucideIcons.earth,
            title: s.get('bypass_countries'),
            content: SizedBox(
              width: 560,
              height: (MediaQuery.sizeOf(context).height * .55).clamp(
                180.0,
                480.0,
              ),
              child: Column(
                children: [
                  TextField(
                    controller: search,
                    decoration: InputDecoration(
                      labelText: s.get('geo_search'),
                      prefixIcon: const Icon(LucideIcons.search),
                    ),
                    onChanged: (_) => update(() {}),
                  ),
                  const SizedBox(height: 8),
                  Expanded(
                    child: ListView.builder(
                      itemCount: countries.length,
                      itemBuilder: (context, index) {
                        final country = countries[index];
                        final ready = app.geoRules.entries.any(
                          (entry) =>
                              entry.countryCode == country.code &&
                              entry.hasGeoip,
                        );
                        return CheckboxListTile(
                          key: ValueKey('scenario-country-${country.code}'),
                          secondary: CountryFlag(countryCode: country.code),
                          title: Text(s.countryName(country.code)),
                          subtitle: Text(
                            '${country.code} · ${s.get(ready ? 'geo_downloaded' : 'geo_not_downloaded')}',
                          ),
                          value: selected.contains(country.code),
                          onChanged: (value) => update(() {
                            if (value == true) {
                              selected.add(country.code);
                            } else {
                              selected.remove(country.code);
                            }
                          }),
                        );
                      },
                    ),
                  ),
                ],
              ),
            ),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context),
                child: Text(s.get('cancel')),
              ),
              FilledButton(
                onPressed: () => Navigator.pop(context, selected),
                child: Text(s.get('save_changes')),
              ),
            ],
          );
        },
      ),
    );
    final result = await Navigator.of(context, rootNavigator: true).push(route);
    await route.completed;
    search.dispose();
    if (result == null || !mounted) return;
    final codes = result.toList()..sort();
    _countries.text = codes.join(', ');
    _edit(_draft.copyWith(geoDirectCountries: codes), ['geo_direct_countries']);
  }

  Future<void> _chooseProtonServer() async {
    final exit = _draft.chainExit;
    if (exit?.profileId == null || _loading || _saving) return;
    setState(() => _loading = true);
    try {
      final server = await chooseSavedProtonServer(
        context,
        app,
        exit!.profileId!,
        exit.protonServerId,
      );
      if (server != null && mounted && !_saving && _draft.chainExit == exit) {
        _edit(
          _draft.copyWith(chainExit: exit.copyWith(protonServerId: server)),
          ['chain_exit'],
        );
      }
    } on Object {
      if (mounted && _draft.chainEnabled) {
        setState(() => _error = app.strings.get('scenario_reference_missing'));
      }
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Widget _switch(String key, bool value, void Function(bool) change) =>
      SwitchListTile.adaptive(
        contentPadding: EdgeInsets.zero,
        title: Text(app.strings.get(key)),
        value: value,
        onChanged: _saving ? null : change,
      );
  Widget _dropdown<T>({
    required String label,
    required T value,
    required List<DropdownMenuItem<T>> items,
    required ValueChanged<T?> onChanged,
  }) => DropdownButtonFormField<T>(
    key: ValueKey('$label:$value'),
    initialValue: value,
    isExpanded: true,
    decoration: FieldDropdown.decoration(context, labelText: label),
    items: items,
    onChanged: _saving ? null : onChanged,
  );
  Widget _heading(String key) => Padding(
    padding: const EdgeInsets.only(top: 28, bottom: 16),
    child: ContentHeading(title: app.strings.get(key)),
  );

  @override
  Widget build(BuildContext context) {
    final s = app.strings;
    final availableIds = app.profiles.map((p) => p.id).toSet();
    final exitItems = <DropdownMenuItem<String>>[
      const DropdownMenuItem(value: 'none', child: Text('WARP')),
      for (final exit in _exits)
        DropdownMenuItem(
          value: 'chain:${exit.id}:${exit.revision}',
          child: Text('${exit.name} · ${exit.source.label}'),
        ),
      for (final gate in _gates)
        DropdownMenuItem(
          value: 'gate:${gate.id}:${gate.configSha256}',
          child: Text('${gate.hostname} · VPN Gate'),
        ),
    ];
    if (!exitItems.any((item) => item.value == _exitKey)) {
      exitItems.add(
        DropdownMenuItem(
          value: _exitKey,
          child: Text(s.get('scenario_reference_missing')),
        ),
      );
    }
    return UnsavedChangesGuard(
      strings: s,
      dirty: _dirty,
      saving: _saving,
      child: Column(
        children: [
          Expanded(
            child: PageFrame(
              title: _savedScene?.name ?? s.get('scenario_create'),
              contentWidth: 880,
              header: Align(
                alignment: AlignmentDirectional.centerStart,
                child: TextButton.icon(
                  onPressed: _saving ? null : () => Navigator.maybePop(context),
                  icon: const Icon(LucideIcons.arrowLeft),
                  label: Text(s.get('back')),
                ),
              ),
              child: Form(
                key: _form,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    TextFormField(
                      key: const ValueKey('scenario-name'),
                      controller: _name,
                      enabled: !_saving,
                      decoration: InputDecoration(
                        labelText: s.get('scenario_name'),
                      ),
                      maxLength: 64,
                      validator: (value) =>
                          value == null ||
                              value.trim().isEmpty ||
                              value.trim().runes.length > 64
                          ? s.get('scenario_validation_name')
                          : null,
                      onChanged: (_) => setState(() => _dirty = true),
                    ),
                    const SizedBox(height: 16),
                    _dropdown<String>(
                      label: s.get('scenario_binding'),
                      value: _binding ?? '',
                      items: [
                        DropdownMenuItem(
                          value: '',
                          child: Text(s.get('scenario_follow_account')),
                        ),
                        for (final a in app.profiles)
                          DropdownMenuItem(value: a.id, child: Text(a.name)),
                        if (_binding != null &&
                            !availableIds.contains(_binding))
                          DropdownMenuItem(
                            value: _binding,
                            child: Text(s.get('scenario_reference_missing')),
                          ),
                      ],
                      onChanged: (id) => setState(() {
                        _binding = id == '' ? null : id;
                        _dirty = true;
                      }),
                    ),
                    _heading('scenario_output'),
                    _switch(
                      defaultTargetPlatform == TargetPlatform.android
                          ? 'vpn_mode'
                          : 'tunnel_output',
                      _draft.frontends.tunnel,
                      (v) => _edit(
                        _draft.copyWith(
                          frontends: _draft.frontends.copyWith(tunnel: v),
                        ),
                        ['frontends'],
                      ),
                    ),
                    _switch(
                      'socks_listener',
                      _draft.frontends.socks5,
                      (v) => _edit(
                        _draft.copyWith(
                          frontends: _draft.frontends.copyWith(socks5: v),
                        ),
                        ['frontends'],
                      ),
                    ),
                    _switch(
                      'http_listener',
                      _draft.frontends.http,
                      (v) => _edit(
                        _draft.copyWith(
                          frontends: _draft.frontends.copyWith(http: v),
                          proxy: _draft.proxy.copyWith(
                            systemProxy: v && _draft.proxy.systemProxy,
                          ),
                        ),
                        ['frontends', 'proxy_system_proxy'],
                      ),
                    ),
                    if (defaultTargetPlatform == TargetPlatform.windows)
                      _switch(
                        'home_system_proxy',
                        _draft.proxy.systemProxy,
                        (v) => _edit(
                          _draft.copyWith(
                            proxy: _draft.proxy.copyWith(systemProxy: v),
                            frontends: _draft.frontends.copyWith(
                              http: v || _draft.frontends.http,
                            ),
                          ),
                          ['frontends', 'proxy_system_proxy'],
                        ),
                      ),
                    _heading('routing_protection'),
                    _switch(
                      'allow_lan',
                      _draft.allowLan,
                      (v) => _edit(_draft.copyWith(allowLan: v), ['allow_lan']),
                    ),
                    _switch(
                      'routing_ads',
                      _draft.routing.adsEnabled,
                      (v) => _edit(
                        _draft.copyWith(
                          routing: RoutingSettings(
                            rules: _draft.routing.rules,
                            adsEnabled: v,
                          ),
                        ),
                        ['routing'],
                      ),
                    ),
                    RoutingRulesEditor(
                      value: _draft.routing,
                      strings: s,
                      onChanged: _saving
                          ? null
                          : (v) =>
                                _edit(_draft.copyWith(routing: v), ['routing']),
                    ),
                    const SizedBox(height: 16),
                    LinkRow(
                      icon: LucideIcons.earth,
                      title: s.get('bypass_countries'),
                      value: _draft.geoDirectCountries
                          .map(s.countryName)
                          .join(', '),
                      onTap: _saving ? null : _chooseCountries,
                    ),
                    _heading('dns'),
                    WarpDnsEditor(
                      value: _draft.warpDns,
                      enabled: !_saving,
                      strings: s,
                      onChanged: (v) =>
                          _edit(_draft.copyWith(warpDns: v), ['warp_dns']),
                    ),
                    const SizedBox(height: 16),
                    TextFormField(
                      controller: _dns,
                      enabled: !_saving,
                      decoration: InputDecoration(
                        labelText: s.get('warp_dns_plain'),
                      ),
                      onChanged: (_) => setState(() {
                        _changed.add('dns_servers');
                        _dirty = true;
                      }),
                      validator: (value) =>
                          _addresses(
                            value ?? '',
                          ).any((ip) => InternetAddress.tryParse(ip) == null)
                          ? s.get('scenario_invalid_dns')
                          : null,
                    ),
                    const SizedBox(height: 24),
                    DirectDnsEditor(
                      value: _draft.directDns,
                      enabled: !_saving,
                      strings: s,
                      onChanged: (v) =>
                          _edit(_draft.copyWith(directDns: v), ['direct_dns']),
                    ),
                    _heading('scenario_exit'),
                    if (_loading) const LinearProgressIndicator(),
                    _dropdown<String>(
                      label: s.get('scenario_exit'),
                      value: _exitKey,
                      items: exitItems,
                      onChanged: _chooseExit,
                    ),
                    const SizedBox(height: 8),
                    HintText(s.get('scenario_library_hint')),
                    TextButton.icon(
                      onPressed: _saving || _loading ? null : _loadExits,
                      icon: const Icon(LucideIcons.refreshCw),
                      label: Text(s.get('settings_refresh')),
                    ),
                    if (_draft.chainSource == ChainSource.protonVpn &&
                        _draft.chainEnabled)
                      TextButton(
                        onPressed: _saving ? null : _chooseProtonServer,
                        child: Text(s.proton('choose_server')),
                      ),
                    _dropdown<ProxyDnsMode>(
                      label: s.get('proxy_dns_mode'),
                      value: _draft.proxy.dnsMode,
                      items: ProxyDnsMode.values
                          .map(
                            (v) => DropdownMenuItem(
                              value: v,
                              child: Text(
                                v == ProxyDnsMode.remote
                                    ? s.get('automatic')
                                    : v == ProxyDnsMode.localConfigured
                                    ? s.get('dns')
                                    : v == ProxyDnsMode.system
                                    ? s.get('proxy_dns_system')
                                    : s.get('proxy_dns_remote'),
                              ),
                            ),
                          )
                          .toList(),
                      onChanged: (v) {
                        if (v != null) {
                          _edit(
                            _draft.copyWith(
                              proxy: _draft.proxy.copyWith(dnsMode: v),
                            ),
                            ['proxy_dns_mode'],
                          );
                        }
                      },
                    ),
                    const SizedBox(height: 16),
                    TextFormField(
                      controller: _proxyDns,
                      enabled: !_saving,
                      decoration: InputDecoration(labelText: s.get('dns')),
                      onChanged: (_) => setState(() {
                        _changed.add('proxy_dns_servers');
                        _dirty = true;
                      }),
                      validator: (value) =>
                          _addresses(
                            value ?? '',
                          ).any((ip) => InternetAddress.tryParse(ip) == null)
                          ? s.get('scenario_invalid_dns')
                          : null,
                    ),
                    _heading('scenario_connection'),
                    _dropdown<TransportPolicy>(
                      label: s.get('transport'),
                      value: _draft.transport,
                      items: TransportPolicy.values
                          .map(
                            (v) => DropdownMenuItem(
                              value: v,
                              child: Text(s.get(v.name)),
                            ),
                          )
                          .toList(),
                      onChanged: (v) {
                        if (v != null) {
                          _edit(_draft.copyWith(transport: v), ['transport']);
                        }
                      },
                    ),
                    const SizedBox(height: 16),
                    _dropdown<IpPolicy>(
                      label: s.get('ip_policy'),
                      value: _draft.ipPolicy,
                      items: IpPolicy.values
                          .map(
                            (v) => DropdownMenuItem(
                              value: v,
                              child: Text(
                                s.get(switch (v) {
                                  IpPolicy.automatic => 'automatic',
                                  IpPolicy.preferIpv4 => 'prefer_ipv4',
                                  IpPolicy.preferIpv6 => 'prefer_ipv6',
                                  IpPolicy.ipv4Only => 'ipv4_only',
                                  IpPolicy.ipv6Only => 'ipv6_only',
                                }),
                              ),
                            ),
                          )
                          .toList(),
                      onChanged: (v) {
                        if (v != null) {
                          _edit(_draft.copyWith(ipPolicy: v), ['ip_policy']);
                        }
                      },
                    ),
                    const SizedBox(height: 20),
                    HintText(s.get('scenario_global_hint')),
                    const SizedBox(height: 16),
                    TextButton(
                      onPressed: _saving ? null : () => _save(use: true),
                      child: Text(s.get('scenario_save_use')),
                    ),
                  ],
                ),
              ),
            ),
          ),
          if (_dirty || _saving || _error != null)
            SaveChangesBar(
              strings: s,
              dirty: _dirty,
              saving: _saving,
              error: _error,
              onSave: () => _save(),
              saveLabel: s.get('scenario_save'),
              contentWidth: 880,
              matchPageGutter: true,
            ),
        ],
      ),
    );
  }
}
