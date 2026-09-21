import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import '../core/chain_strings.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/chain_source_icon.dart';
import '../widgets/common.dart';
import '../widgets/save_changes_bar.dart';
import '../widgets/unsaved_changes_guard.dart';
import '../widgets/usque_dialog.dart';
import 'vpn_gate_screen.dart';

class ChainProxyScreen extends StatefulWidget {
  const ChainProxyScreen({
    required this.controller,
    this.active = true,
    this.leaveGuardKey,
    super.key,
  });
  final AppController controller;
  final bool active;
  final GlobalKey<UnsavedChangesGuardState>? leaveGuardKey;
  @override
  State<ChainProxyScreen> createState() => _ChainProxyScreenState();
}

class _ChainProxyScreenState extends State<ChainProxyScreen> {
  final _ownGuard = GlobalKey<UnsavedChangesGuardState>();
  GlobalKey<UnsavedChangesGuardState> get _guard =>
      widget.leaveGuardKey ?? _ownGuard;
  late ChainSource _source = widget.controller.activeProfile.chainSource;
  Future<void> _switch(ChainSource? source) async {
    if (source == null || source == _source) return;
    if (!await (_guard.currentState?.confirmLeave() ?? Future.value(true)) ||
        !mounted) {
      return;
    }
    _guard.currentState?.resetDiscardDecision();
    setState(() => _source = source);
  }

  @override
  Widget build(BuildContext context) {
    final strings = widget.controller.strings;
    final picker = DropdownButtonFormField<ChainSource>(
      key: ValueKey('chain-source-${_source.wire}'),
      initialValue: _source,
      isExpanded: true,
      isDense: false,
      itemHeight: null,
      decoration: InputDecoration(labelText: strings.chain('source')),
      items: [
        for (final source in ChainSource.values)
          DropdownMenuItem(
            value: source,
            child: Row(
              children: [
                ChainSourceIcon(
                  source: source,
                  size: 20,
                  color: source == _source
                      ? Theme.of(context).colorScheme.primary
                      : null,
                ),
                const SizedBox(width: 12),
                Flexible(child: Text(source.label)),
              ],
            ),
          ),
      ],
      onChanged: (value) => unawaited(_switch(value)),
    );
    if (_source == ChainSource.vpnGate) {
      return VpnGateScreen(
        controller: widget.controller,
        active: widget.active,
        leaveGuardKey: _guard,
        sourcePicker: picker,
      );
    }
    return _CustomChainEditor(
      key: ValueKey(_source),
      controller: widget.controller,
      source: _source,
      picker: picker,
      guard: _guard,
    );
  }
}

class _CustomChainEditor extends StatefulWidget {
  const _CustomChainEditor({
    required this.controller,
    required this.source,
    required this.picker,
    required this.guard,
    super.key,
  });
  final AppController controller;
  final ChainSource source;
  final Widget picker;
  final GlobalKey<UnsavedChangesGuardState> guard;
  @override
  State<_CustomChainEditor> createState() => _CustomChainEditorState();
}

class _CustomChainEditorState extends State<_CustomChainEditor> {
  late ChainExitSettings _baseline, _draft;
  List<ChainProfileSummary> _profiles = const [];
  bool _loading = true, _saving = false;
  bool _wasSupported = false;
  String? _error;
  AppController get _app => widget.controller;
  bool get _dirty => _draft != _baseline;
  bool get _supported =>
      (_app.engineCapabilities?.chainProfileImport ?? false) &&
      (widget.source != ChainSource.wireguardCustom ||
          (_app.engineCapabilities?.chainWireguard ?? false));
  ChainExitSettings get _stored =>
      _app.activeProfile.chainExit ??
      ChainExitSettings(
        enabled: _app.activeProfile.vpnGate.enabled,
        source: _app.activeProfile.chainSource,
      );
  ChainProfileSummary? get _selected =>
      _profiles.where((p) => p.id == _draft.profileId).firstOrNull;
  bool get _modeConflict =>
      _draft.enabled &&
      _selected?.requiresUdp == true &&
      _app.activeProfile.dataPlane == DataPlaneMode.l4Proxy;
  @override
  void initState() {
    super.initState();
    _baseline = _stored;
    _draft = _baseline.source == widget.source
        ? _baseline
        : ChainExitSettings(source: widget.source, enabled: _baseline.enabled);
    _app.addListener(_changed);
    unawaited(_load());
  }

  void _changed() {
    if (!mounted) return;
    if (!_wasSupported && _supported) {
      _wasSupported = true;
      unawaited(_load());
    }
    if (!_dirty && !_saving) {
      _baseline = _stored;
      if (_baseline.source == widget.source) _draft = _baseline;
    }
    setState(() {});
  }

  @override
  void dispose() {
    _app.removeListener(_changed);
    super.dispose();
  }

  String _message(Map<String, Object?> error) =>
      '${_app.strings.chain(error['reason'] as String? ?? 'invalid_configuration')} (${error['field'] ?? ''}: ${error['line'] ?? 0})';
  Future<void> _load() async {
    _wasSupported = _supported;
    if (!_supported) {
      if (mounted) setState(() => _loading = false);
      return;
    }
    try {
      final result = await _app.chainProfile({'action': 'list'});
      if (!mounted) return;
      setState(() {
        _profiles = result.profiles;
        _loading = false;
        _error = result.error == null ? null : _message(result.error!);
      });
    } catch (_) {
      if (mounted) {
        setState(() {
          _loading = false;
          _error = _app.strings.chain('secure_storage_failed');
        });
      }
    }
  }

  Future<void> _import(bool file) async {
    String? text;
    if (file) {
      try {
        text = await _app.pickChainConfiguration();
      } catch (_) {
        if (mounted) {
          setState(() => _error = _app.strings.chain('file_unavailable'));
        }
        return;
      }
      if (text == null || !mounted) return;
    }
    if (!mounted) return;
    final result = await showDialog<ChainProfileSummary>(
      context: context,
      builder: (_) => _ImportDialog(
        controller: _app,
        source: widget.source,
        configuration: text,
      ),
    );
    if (!mounted || result == null) return;
    await _load();
    // Saving an imported object never selects it or changes a connection.
  }

  Future<void> _apply({bool switchMode = false}) async {
    final target = _draft;
    final account = _app.activeProfile.id;
    final intent = _app.connectionIntent;
    if (_saving ||
        !_supported ||
        target.enabled && _selected == null ||
        _modeConflict && !switchMode) {
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });
    final latest = _app.activeProfile;
    if (latest.id != account || _app.connectionIntent != intent) {
      setState(() => _saving = false);
      return;
    }
    final saved = await _app.saveNetwork(
      latest.copyWith(
        chainExit: target,
        vpnGate: latest.vpnGate.copyWith(enabled: false),
        dataPlane: switchMode ? DataPlaneMode.connectIp : latest.dataPlane,
      ),
      changedFields: ['chain_exit', 'vpn_gate', if (switchMode) 'data_plane'],
    );
    if (!mounted) return;
    setState(() {
      _saving = false;
      if (saved) {
        _baseline = _stored;
        _draft = _baseline;
      } else {
        _error = _app.lastError ?? _app.strings.chain('error');
      }
    });
  }

  Future<void> _manage(ChainProfileSummary profile, String action) async {
    final strings = _app.strings;
    if (action == 'credentials') {
      await showDialog<void>(
        context: context,
        builder: (_) => _ImportDialog(
          controller: _app,
          source: widget.source,
          credentialsFor: profile,
        ),
      );
      if (mounted) await _load();
      return;
    }
    final name = TextEditingController(text: profile.name);
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: action == 'remove' ? LucideIcons.trash2 : LucideIcons.pencil,
        title: strings.chain(action == 'remove' ? 'delete' : 'rename'),
        content: action == 'remove'
            ? Text(strings.chain('delete_confirm'))
            : TextField(
                controller: name,
                maxLength: 64,
                autofocus: true,
                decoration: InputDecoration(labelText: strings.chain('name')),
              ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(strings.chain('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(strings.chain(action == 'remove' ? 'delete' : 'save')),
          ),
        ],
      ),
    );
    final value = name.text;
    name.dispose();
    if (confirmed != true || !mounted) return;
    setState(() => _saving = true);
    try {
      final result = await _app.chainProfile({
        'action': action,
        'profile_id': profile.id,
        'revision': profile.editRevision,
        'name': value,
      });
      if (!mounted) return;
      setState(() {
        if (result.error == null) _profiles = result.profiles;
        _error = result.error == null ? null : _message(result.error!);
      });
    } catch (_) {
      if (mounted) {
        setState(() => _error = strings.chain('secure_storage_failed'));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final strings = _app.strings;
    final profiles = _profiles.where((p) => p.source == widget.source).toList();
    final current = _app.snapshot.chainExit.currentProfile;
    final saved = _profiles
        .where((p) => p.id == _baseline.profileId)
        .firstOrNull;
    return UnsavedChangesGuard(
      key: widget.guard,
      strings: strings,
      dirty: _dirty,
      saving: _saving,
      child: SubPage(
        title: strings.chain('title'),
        subtitle: strings.chain('subtitle'),
        backLabel: strings.get('back'),
        contentWidth: 880,
        bottomBar: SaveChangesBar(
          strings: strings,
          dirty: _dirty,
          saving: _saving,
          error: _error,
          statusLabel: _app.networkSettingsMessage,
          onSave:
              _dirty &&
                  !_saving &&
                  !_app.snapshot.isTransitional &&
                  !_modeConflict &&
                  _supported &&
                  (!_draft.enabled || _selected != null)
              ? () => unawaited(_apply())
              : null,
        ),
        child: PanelStack(
          spacing: 28,
          children: [
            widget.picker,
            SwitchListTile.adaptive(
              key: const ValueKey('chain-proxy-toggle'),
              contentPadding: EdgeInsets.zero,
              title: Text(strings.chain('enable')),
              subtitle: Text(strings.chain('scope')),
              value: _draft.enabled,
              onChanged: _saving || !_supported
                  ? null
                  : (value) => setState(
                      () => _draft = _draft.copyWith(enabled: value),
                    ),
            ),
            if (!_supported)
              WarningBanner(
                title: widget.source.label,
                message: strings.chain('unsupported'),
              ),
            ContentSection(
              title: strings.chain('current'),
              children: [
                Text(
                  current == null
                      ? (_app.snapshot.vpnGate.server?.hostname ??
                            strings.chain('disconnected'))
                      : '${current.source.label} · ${current.name}',
                ),
                if (current != null ||
                    _app.snapshot.vpnGate.server != null ||
                    _app.snapshot.isTransitional)
                  Text(
                    strings.chain(
                      _app.snapshot.chainExit.stage == 'connected'
                          ? 'connected'
                          : _app.snapshot.chainExit.stage == 'error'
                          ? 'error'
                          : _app.snapshot.isTransitional
                          ? 'connecting'
                          : 'disconnected',
                    ),
                  ),
                if (_app.snapshot.chainExit.failure != null)
                  Text(
                    strings.chain(
                      _app.snapshot.chainExit.failure == 'authentication'
                          ? 'authentication_failed'
                          : 'error',
                    ),
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                    ),
                  ),
                if (_app.snapshot.chainExit.dnsUnavailable)
                  Text(strings.chain('dns_unavailable')),
              ],
            ),
            ContentSection(
              title: strings.chain('profiles'),
              children: [
                Wrap(
                  spacing: 12,
                  runSpacing: 12,
                  children: [
                    OutlinedButton.icon(
                      onPressed: _supported && !_saving
                          ? () => unawaited(_import(true))
                          : null,
                      icon: const Icon(LucideIcons.fileUp),
                      label: Text(strings.chain('import_file')),
                    ),
                    OutlinedButton.icon(
                      onPressed: _supported && !_saving
                          ? () => unawaited(_import(false))
                          : null,
                      icon: const Icon(LucideIcons.clipboard),
                      label: Text(strings.chain('paste')),
                    ),
                  ],
                ),
                if (_loading) const LinearProgressIndicator(),
                if (!_loading && profiles.isEmpty) Text(strings.chain('empty')),
                RadioGroup<String>(
                  groupValue: _draft.profileId,
                  onChanged: (id) {
                    if (!_draft.enabled || _saving || id == null) return;
                    final profile = profiles.firstWhere((p) => p.id == id);
                    setState(
                      () => _draft = _draft.copyWith(
                        profileId: profile.id,
                        revision: profile.revision,
                      ),
                    );
                  },
                  child: Column(
                    children: [
                      for (final profile in profiles)
                        RadioListTile<String>(
                          value: profile.id,
                          enabled: _draft.enabled && !_saving && _supported,
                          contentPadding: EdgeInsets.zero,
                          title: Text(profile.name),
                          subtitle: Text(
                            '${profile.host}:${profile.port} · ${profile.transportLabel}',
                          ),
                          secondary: PopupMenuButton<String>(
                            enabled: !_saving,
                            onSelected: (action) =>
                                unawaited(_manage(profile, action)),
                            itemBuilder: (_) => [
                              PopupMenuItem(
                                value: 'rename',
                                child: Text(strings.chain('rename')),
                              ),
                              if (profile.source == ChainSource.openvpnCustom)
                                PopupMenuItem(
                                  value: 'credentials',
                                  child: Text(strings.chain('credentials')),
                                ),
                              PopupMenuItem(
                                value: 'remove',
                                enabled: profile.id != _draft.profileId,
                                child: Text(strings.chain('delete')),
                              ),
                            ],
                          ),
                        ),
                    ],
                  ),
                ),
              ],
            ),
            ContentSection(
              title: strings.chain('saved'),
              child: Text(
                saved?.name ??
                    (_baseline.source == ChainSource.vpnGate
                        ? 'VPN Gate'
                        : strings.chain('no_selection')),
              ),
            ),
            ContentSection(
              title: strings.chain('draft'),
              children: [
                Text(_selected?.name ?? strings.chain('no_selection')),
                if (_selected case final selected?)
                  _ProfileDetails(profile: selected, controller: _app),
                TextButton(
                  onPressed: _saving
                      ? null
                      : () => setState(
                          () => _draft = _draft.copyWith(
                            enabled: false,
                            clearSelection: true,
                          ),
                        ),
                  child: Text(strings.chain('clear')),
                ),
              ],
            ),
            if (_modeConflict)
              ContentSection(
                children: [
                  Text(strings.chain('l4')),
                  FilledButton(
                    onPressed: _saving || _app.snapshot.isTransitional
                        ? null
                        : () => unawaited(_apply(switchMode: true)),
                    child: Text(strings.chain('switch_mode')),
                  ),
                ],
              ),
          ],
        ),
      ),
    );
  }
}

class _ProfileDetails extends StatelessWidget {
  const _ProfileDetails({required this.profile, required this.controller});
  final ChainProfileSummary profile;
  final AppController controller;
  @override
  Widget build(BuildContext context) {
    final s = controller.strings;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SelectableText(
          '${s.chain('endpoint')}: ${profile.host}:${profile.port} · ${profile.transportLabel}',
        ),
        Text(profile.addressFamily),
        Text('${s.chain('source')}: ${profile.source.label}'),
        if (profile.addresses.isNotEmpty)
          SelectableText(
            '${s.chain('addresses')}: ${profile.addresses.join(', ')}',
          ),
        SelectableText(
          '${s.chain('dns')}: ${profile.dns.isEmpty ? s.chain('dns_fallback') : profile.dns.join(', ')}',
        ),
        if (profile.allowedIps.isNotEmpty) ...[
          SelectableText(
            '${s.chain('allowed')}: ${profile.allowedIps.join(', ')}',
          ),
          Text(s.chain('restricted')),
        ],
        if (profile.mtu != null) Text('MTU: ${profile.mtu}'),
      ],
    );
  }
}

class _ImportDialog extends StatefulWidget {
  const _ImportDialog({
    required this.controller,
    required this.source,
    this.configuration,
    this.credentialsFor,
  });
  final AppController controller;
  final ChainSource source;
  final String? configuration;
  final ChainProfileSummary? credentialsFor;
  @override
  State<_ImportDialog> createState() => _ImportDialogState();
}

class _ImportDialogState extends State<_ImportDialog> {
  late final _name = TextEditingController(
    text: widget.credentialsFor?.name ?? widget.source.label,
  );
  late final _configuration = TextEditingController(
    text: widget.configuration ?? '',
  );
  final _username = TextEditingController(),
      _password = TextEditingController(),
      _keyPassword = TextEditingController();
  ChainProfileSummary? _preview;
  bool _busy = false;
  String? _error;
  @override
  void dispose() {
    for (final controller in [
      _name,
      _configuration,
      _username,
      _password,
      _keyPassword,
    ]) {
      controller.clear();
      controller.dispose();
    }
    super.dispose();
  }

  Future<void> _submit(bool save) async {
    final s = widget.controller.strings;
    if (_busy) return;
    if (utf8.encode(_configuration.text).length > 128 * 1024) {
      setState(() => _error = s.chain('invalid_size_or_encoding'));
      return;
    }
    final profile = widget.credentialsFor ?? _preview;
    if (save &&
        (profile?.requiresAuth == true &&
                (_username.text.isEmpty || _password.text.isEmpty) ||
            profile?.requiresKeyPassword == true &&
                _keyPassword.text.isEmpty)) {
      setState(() => _error = s.chain('missing_field'));
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final result = await widget.controller.chainProfile({
        'action': widget.credentialsFor != null
            ? 'credentials'
            : save
            ? 'import'
            : 'preview',
        'source': widget.source.wire,
        'name': _name.text,
        if (widget.credentialsFor case final stored?) ...{
          'profile_id': stored.id,
          'revision': stored.editRevision,
        },
        'configuration': _configuration.text,
        'username': _username.text,
        'password': _password.text,
        'private_key_password': _keyPassword.text,
      });
      if (!mounted) return;
      if (result.error case final error?) {
        setState(
          () => _error =
              '${s.chain(error['reason'] as String? ?? 'invalid_configuration')} (${error['field']}: ${error['line']})',
        );
      } else if (save) {
        Navigator.pop(context, result.preview);
      } else {
        setState(() => _preview = result.preview);
      }
    } catch (_) {
      if (mounted) setState(() => _error = s.chain('secure_storage_failed'));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final s = widget.controller.strings;
    final profile = widget.credentialsFor ?? _preview;
    return PopScope(
      canPop: !_busy,
      child: UsqueDialog(
        icon: LucideIcons.fileUp,
        title: widget.credentialsFor == null
            ? widget.source.label
            : s.chain('credentials'),
        content: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (widget.credentialsFor == null) ...[
                TextField(
                  controller: _name,
                  enabled: !_busy,
                  maxLength: 64,
                  decoration: InputDecoration(labelText: s.chain('name')),
                ),
                const SizedBox(height: 16),
                if (widget.configuration == null)
                  TextField(
                    controller: _configuration,
                    enabled: !_busy,
                    minLines: 5,
                    maxLines: 10,
                    autocorrect: false,
                    enableSuggestions: false,
                    decoration: InputDecoration(
                      labelText: s.chain('configuration'),
                    ),
                    onChanged: (_) => setState(() => _preview = null),
                  ),
                if (_preview == null)
                  OutlinedButton(
                    onPressed: _busy ? null : () => unawaited(_submit(false)),
                    child: Text(s.chain(_busy ? 'checking' : 'preview')),
                  ),
              ],
              if (profile != null) ...[
                const SizedBox(height: 16),
                _ProfileDetails(
                  profile: profile,
                  controller: widget.controller,
                ),
                if (profile.requiresAuth ||
                    widget.credentialsFor != null &&
                        profile.source == ChainSource.openvpnCustom) ...[
                  const SizedBox(height: 16),
                  TextField(
                    controller: _username,
                    enabled: !_busy,
                    autocorrect: false,
                    enableSuggestions: false,
                    decoration: InputDecoration(labelText: s.chain('username')),
                  ),
                  const SizedBox(height: 16),
                  TextField(
                    controller: _password,
                    enabled: !_busy,
                    obscureText: true,
                    autocorrect: false,
                    enableSuggestions: false,
                    decoration: InputDecoration(labelText: s.chain('password')),
                  ),
                ],
                if (profile.requiresKeyPassword) ...[
                  const SizedBox(height: 16),
                  TextField(
                    controller: _keyPassword,
                    enabled: !_busy,
                    obscureText: true,
                    decoration: InputDecoration(
                      labelText: s.chain('key_password'),
                    ),
                  ),
                ],
              ],
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.only(top: 16),
                  child: Text(
                    _error!,
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                    ),
                  ),
                ),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: _busy ? null : () => Navigator.pop(context),
            child: Text(s.chain('cancel')),
          ),
          FilledButton(
            onPressed: _busy || profile == null
                ? null
                : () => unawaited(_submit(true)),
            child: Text(s.chain('save_import')),
          ),
        ],
      ),
    );
  }
}
