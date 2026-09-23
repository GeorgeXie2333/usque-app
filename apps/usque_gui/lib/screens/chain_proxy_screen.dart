import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import '../core/chain_strings.dart';
import '../core/usque_theme.dart';
import '../core/warp_strings.dart';
import '../models/app_models.dart';
import '../services/engine_client.dart';
import '../state/app_controller.dart';
import '../widgets/chain_current_connection.dart';
import '../widgets/chain_editor_layout.dart';
import '../widgets/chain_source_picker.dart';
import '../widgets/common.dart';
import '../widgets/save_changes_bar.dart';
import '../widgets/unsaved_changes_guard.dart';
import '../widgets/usque_dialog.dart';
import '../widgets/warp_wireguard_panel.dart';
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

/// The stored chain settings, or their legacy equivalent before the first
/// chain-exit save.
ChainExitSettings _storedChainExit(UsqueProfile profile) =>
    profile.chainExit ??
    ChainExitSettings(
      enabled: profile.vpnGate.enabled,
      source: profile.chainSource,
    );

/// Whether a custom draft changes what is stored.
///
/// Looking at another source is not an edit: only the page switch and a
/// configuration selection count, so switching sources never asks to discard
/// anything.
bool _chainPending(ChainExitSettings stored, ChainExitSettings draft) {
  if (draft.enabled != stored.enabled) return true;
  if (draft.source != stored.source) return draft.profileId != null;
  return draft != stored;
}

class _ChainProxyScreenState extends State<ChainProxyScreen> {
  final _ownGuard = GlobalKey<UnsavedChangesGuardState>();
  GlobalKey<UnsavedChangesGuardState> get _guard =>
      widget.leaveGuardKey ?? _ownGuard;
  late ChainSource _source = widget.controller.activeProfile.chainSource;

  /// Unapplied drafts, kept while the user compares sources.
  ///
  /// Switching the source is browsing, not editing: it never asks to discard
  /// anything, and a selection made under one source is still there when the
  /// user comes back to it. Applied or reloaded settings replace them.
  final _drafts = <ChainSource, ChainExitSettings>{};
  VpnGateSettings? _gateDraft;
  VpnGateServer? _gateServer;

  /// The page switch as the user last left it, under any source.
  bool? _enabled;
  late ChainExitSettings? _storedExit =
      widget.controller.activeProfile.chainExit;
  AppController get _app => widget.controller;

  @override
  void initState() {
    super.initState();
    _app.addListener(_storedChanged);
  }

  @override
  void dispose() {
    _app.removeListener(_storedChanged);
    super.dispose();
  }

  void _storedChanged() {
    final stored = _app.activeProfile.chainExit;
    if (stored == _storedExit || !mounted) return;
    setState(() {
      _storedExit = stored;
      _drafts.clear();
      _gateDraft = null;
      _gateServer = null;
      _enabled = null;
    });
  }

  void _retain(ChainExitSettings draft) {
    _drafts[draft.source] = draft;
    _enabled = draft.enabled;
  }

  void _retainGate(VpnGateSettings draft, VpnGateServer? server) {
    _gateDraft = draft;
    _gateServer = server;
    _enabled = draft.enabled;
  }

  /// The draft a custom editor for [source] starts from: the draft retained
  /// for that source, else the stored settings, with the page switch carried
  /// over from the source the user just left.
  ChainExitSettings _initialDraft(ChainSource source) {
    final stored = _storedChainExit(_app.activeProfile);
    final base =
        _drafts[source] ??
        (stored.source == source
            ? stored
            : ChainExitSettings(source: source, enabled: stored.enabled));
    return base.copyWith(enabled: _enabled ?? base.enabled);
  }

  /// Whether a source other than [current] holds an unapplied draft, so
  /// leaving the page still asks before discarding it.
  bool _otherPending(ChainSource current) {
    final profile = _app.activeProfile;
    final stored = _storedChainExit(profile);
    for (final draft in _drafts.values) {
      if (draft.source != current &&
          _chainPending(
            stored,
            draft.copyWith(enabled: _enabled ?? draft.enabled),
          )) {
        return true;
      }
    }
    return current != ChainSource.vpnGate &&
        _gateDraft != null &&
        VpnGateScreen.chainPending(
          profile,
          _gateDraft!.copyWith(enabled: _enabled),
        );
  }

  void _switch(ChainSource? source) {
    if (source == null || source == _source) return;
    setState(() => _source = source);
  }

  @override
  Widget build(BuildContext context) {
    if (_source == ChainSource.vpnGate) {
      return VpnGateScreen(
        controller: _app,
        active: widget.active,
        leaveGuardKey: _guard,
        chainPageBuilder: _buildPage,
        initialDraft: _gateDraft?.copyWith(enabled: _enabled),
        initialServer: _gateServer,
        initialEnabled: _enabled,
        onDraftChanged: _retainGate,
        otherPending: _otherPending(ChainSource.vpnGate),
      );
    }
    return _CustomChainEditor(
      key: ValueKey(_source),
      controller: _app,
      source: _source,
      pageBuilder: _buildPage,
      guard: _guard,
      initialDraft: _initialDraft(_source),
      onDraftChanged: _retain,
      otherPending: _otherPending(_source),
    );
  }

  Widget _buildPage({
    required bool enabled,
    required ValueChanged<bool>? onEnabledChanged,
    required Widget bottomBar,
    required List<Widget> slivers,
    Widget? warning,
  }) => SubPage(
    title: _app.strings.chain('title'),
    subtitle: _app.strings.chain('subtitle'),
    backLabel: _app.strings.get('back'),
    contentWidth: 880,
    bottomBar: ConstrainedBox(
      constraints: BoxConstraints(
        maxHeight: MediaQuery.sizeOf(context).height * .45,
      ),
      child: SingleChildScrollView(primary: false, child: bottomBar),
    ),
    slivers: [
      SliverToBoxAdapter(
        child: Padding(
          padding: const EdgeInsets.only(bottom: 20),
          child: PanelStack(
            spacing: 20,
            children: [
              SwitchListTile.adaptive(
                key: ValueKey(
                  _source == ChainSource.vpnGate
                      ? 'vpn-gate-toggle'
                      : 'chain-proxy-toggle',
                ),
                contentPadding: EdgeInsets.zero,
                title: Text(_app.strings.chain('enable')),
                subtitle: Text(_app.strings.chain('scope')),
                value: enabled,
                onChanged: onEnabledChanged,
              ),
              ChainSourcePicker(
                controller: _app,
                source: _source,
                onChanged: _switch,
              ),
              ?warning,
              ChainCurrentConnection(
                controller: _app,
                configuredEnabled: _app.activeProfile.chainEnabled,
              ),
            ],
          ),
        ),
      ),
      ...slivers,
    ],
  );
}

class _CustomChainEditor extends StatefulWidget {
  const _CustomChainEditor({
    required this.controller,
    required this.source,
    required this.pageBuilder,
    required this.guard,
    required this.initialDraft,
    required this.onDraftChanged,
    required this.otherPending,
    super.key,
  });
  final AppController controller;
  final ChainSource source;
  final ChainPageBuilder pageBuilder;
  final GlobalKey<UnsavedChangesGuardState> guard;
  final ChainExitSettings initialDraft;
  final ValueChanged<ChainExitSettings> onDraftChanged;
  final bool otherPending;
  @override
  State<_CustomChainEditor> createState() => _CustomChainEditorState();
}

class _CustomChainEditorState extends State<_CustomChainEditor> {
  final _warpPanel = GlobalKey<WarpWireguardPanelState>();
  late ChainExitSettings _baseline, _draft;
  List<ChainProfileSummary> _profiles = const [];
  bool _loading = true, _saving = false;
  bool _warpEndpointValid = true;
  bool _wasSupported = false;

  /// Result of the last apply, shown in the action bar.
  String? _error;

  /// Import, file and library problems, shown beside the import actions.
  String? _libraryError;
  AppController get _app => widget.controller;
  bool get _dirty => _chainPending(_baseline, _draft);
  bool get _supported =>
      (_app.engineCapabilities?.chainProfileImport ?? false) &&
      (widget.source != ChainSource.warpWireguard ||
          (_app.engineCapabilities?.chainWarpWireguard ?? false)) &&
      (!widget.source.isWireguard ||
          (_app.engineCapabilities?.chainWireguard ?? false));
  ChainExitSettings get _stored => _storedChainExit(_app.activeProfile);

  /// The stored settings as seen from this source, with nothing pending.
  ChainExitSettings get _fresh => _baseline.source == widget.source
      ? _baseline
      : ChainExitSettings(source: widget.source, enabled: _baseline.enabled);
  ChainProfileSummary? get _selected =>
      _profiles.where((p) => p.id == _draft.profileId).firstOrNull;
  bool get _l4 => _app.activeProfile.dataPlane == DataPlaneMode.l4Proxy;
  bool get _modeConflict =>
      _draft.enabled && _selected?.requiresUdp == true && _l4;
  bool get _multiEndpointBlocked =>
      _draft.enabled &&
      (_selected?.candidates.length ?? 0) > 1 &&
      !(_app.engineCapabilities?.chainOpenvpnMultiEndpoint ?? false);

  /// Why the draft cannot be applied yet, or null when it can.
  String? get _validation {
    final strings = _app.strings;
    if (!_warpEndpointValid) return strings.chain('invalid_endpoint');
    if (_draft.enabled && _selected == null) {
      return strings.chain('select_required');
    }
    if (_multiEndpointBlocked) {
      return strings.chain('multi_endpoint_unavailable');
    }
    return null;
  }

  @override
  void initState() {
    super.initState();
    _baseline = _stored;
    _draft = widget.initialDraft.source == widget.source
        ? widget.initialDraft
        : _fresh;
    _app.addListener(_changed);
    unawaited(_load());
  }

  /// Replaces the draft and lets the page retain it across source switches.
  void _setDraft(ChainExitSettings draft) {
    if (draft == _draft) return;
    _draft = draft;
    widget.onDraftChanged(draft);
  }

  void _changed() {
    if (!mounted) return;
    if (!_wasSupported && _supported) {
      _wasSupported = true;
      unawaited(_load());
    }
    if (!_dirty && !_saving) {
      _baseline = _stored;
      _setDraft(_fresh);
    }
    setState(() {});
  }

  @override
  void dispose() {
    _app.removeListener(_changed);
    super.dispose();
  }

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
        _libraryError = result.error == null
            ? null
            : _app.strings.chainError(result.error!);
      });
    } catch (_) {
      if (mounted) {
        setState(() {
          _loading = false;
          _libraryError = _app.strings.chain('secure_storage_failed');
        });
      }
    }
  }

  Future<void> _import(bool file) async {
    String? text;
    if (file) {
      try {
        text = await _app.pickChainConfiguration();
      } catch (error) {
        if (mounted) {
          final key = switch (error) {
            EngineException(code: 'CHAIN_FILE_UNAVAILABLE') =>
              'file_unavailable',
            EngineException(code: 'CHAIN_FILE_TOO_LARGE') =>
              'invalid_size_or_encoding',
            EngineException(code: 'CHAIN_FILE_ENCODING_INVALID') =>
              'file_encoding_invalid',
            EngineException(code: 'CHAIN_FILE_BUSY') => 'file_busy',
            _ => 'file_read_failed',
          };
          setState(() => _libraryError = _app.strings.chain(key));
        }
        return;
      }
      if (text == null || !mounted) return;
    }
    if (!mounted) return;
    setState(() => _libraryError = null);
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
        _validation != null ||
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
        _setDraft(_baseline);
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
    if (action == 'remove' &&
        (profile.id == _draft.profileId ||
            profile.id == _baseline.profileId ||
            profile.id == _app.snapshot.chainExit.currentProfile?.id)) {
      setState(() => _libraryError = strings.chain('profile_in_use'));
      return;
    }
    final name = TextEditingController(text: profile.name);
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: action == 'remove' ? LucideIcons.trash2 : LucideIcons.pencil,
        danger: action == 'remove',
        title: strings.chain(action == 'remove' ? 'delete' : 'rename'),
        subtitle: profile.name,
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
    setState(() {
      _saving = true;
      _libraryError = null;
    });
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
        _libraryError = result.error == null
            ? null
            : strings.chainError(result.error!);
      });
    } catch (_) {
      if (mounted) {
        setState(() => _libraryError = strings.chain('secure_storage_failed'));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final strings = _app.strings;
    final theme = Theme.of(context);
    final snapshot = _app.snapshot;
    final profiles = _profiles.where((p) => p.source == widget.source).toList();
    final current = snapshot.chainExit.currentProfile;
    final validation = _validation;
    final canApply =
        !_saving &&
        !snapshot.isTransitional &&
        _supported &&
        validation == null &&
        (_dirty || _modeConflict);
    final disabling = _dirty && _baseline.enabled && !_draft.enabled;
    final mutedStyle = theme.textTheme.bodyMedium?.copyWith(
      color: theme.colorScheme.onSurfaceVariant,
    );
    return UnsavedChangesGuard(
      key: widget.guard,
      strings: strings,
      dirty: _dirty || widget.otherPending,
      saving: _saving,
      child: widget.pageBuilder(
        enabled: _draft.enabled,
        onEnabledChanged: _saving || !_supported
            ? null
            : (value) =>
                  setState(() => _setDraft(_draft.copyWith(enabled: value))),
        warning: !_supported
            ? WarningBanner(
                title: widget.source.label,
                message: strings.chain('unsupported'),
              )
            : null,
        bottomBar: SaveChangesBar(
          key: const ValueKey('chain-proxy-bar'),
          contentWidth: 880,
          matchPageGutter: true,
          strings: strings,
          dirty: _dirty || _modeConflict,
          saving: _saving,
          error: _error,
          validationError: _dirty ? validation : null,
          statusLabel: _error != null
              ? null
              : _modeConflict
              ? strings.chain('l4')
              : !_dirty ||
                    _app.networkSettings.unconfirmed ||
                    _app.networkSettings.saveError != null
              ? _app.networkSettingsMessage
              : null,
          saveLabel: _modeConflict
              ? strings.chain('switch_mode')
              : snapshot.isConnected && _dirty
              ? strings.chain('apply_reconnect')
              : null,
          summary: !_dirty
              ? null
              : disabling
              ? Text(
                  strings.chain('pending_disable'),
                  style: theme.textTheme.labelLarge,
                )
              : _draft.enabled && _selected != null
              ? Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      '${strings.chain('draft')}: ${_selected!.name}',
                      style: theme.textTheme.labelLarge,
                    ),
                    const SizedBox(height: 2),
                    Text(
                      '${(_draft.endpointOverride ?? ChainEndpoint(_selected!.host, _selected!.port)).label} · ${_selected!.transportLabel}',
                      style: UsqueTheme.mono(
                        context,
                        color: theme.colorScheme.onSurfaceVariant,
                      ),
                    ),
                  ],
                )
              : null,
          onSave: canApply
              ? () => unawaited(_apply(switchMode: _modeConflict))
              : null,
        ),
        slivers: [
          SliverToBoxAdapter(
            child: ContentSection(
              title: strings.chain('profiles'),
              gap: 12,
              children: [
                Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  children: [
                    if (widget.source == ChainSource.warpWireguard)
                      OutlinedButton.icon(
                        onPressed: !_supported || _saving
                            ? null
                            : () async {
                                final state = _warpPanel.currentState;
                                if (state != null) await state.generate();
                              },
                        icon: const Icon(LucideIcons.plus),
                        label: Text(strings.warp('generate')),
                      ),
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
                BannerSlot(
                  spacing: 0,
                  child: _libraryError == null
                      ? null
                      : WarningBanner(
                          key: const ValueKey('chain-library-error'),
                          title: strings.get('error'),
                          message: _libraryError!,
                          danger: true,
                          onDismiss: () => setState(() => _libraryError = null),
                        ),
                ),
                if (_loading) const LinearProgressIndicator(minHeight: 2),
                if (!_loading && profiles.isEmpty)
                  Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    spacing: 4,
                    children: [
                      Text(strings.chain('empty')),
                      Text(
                        strings.chain(
                          widget.source.isWireguard
                              ? 'empty_hint_wireguard'
                              : 'empty_hint_openvpn',
                        ),
                        style: mutedStyle,
                      ),
                      Text(strings.chain('import_limits'), style: mutedStyle),
                    ],
                  ),
                if (profiles.isNotEmpty && !_draft.enabled && _supported)
                  Text(strings.chain('enable_to_choose'), style: mutedStyle),
                RadioGroup<String>(
                  groupValue: _draft.profileId,
                  onChanged: (id) {
                    if (!_draft.enabled || _saving || id == null) return;
                    final profile = profiles.firstWhere((p) => p.id == id);
                    setState(
                      () => _setDraft(
                        _draft.copyWith(
                          profileId: profile.id,
                          revision: profile.revision,
                        ),
                      ),
                    );
                  },
                  child: ContentList(
                    children: [
                      for (final profile in profiles)
                        _ProfileRow(
                          controller: _app,
                          profile: profile,
                          enabled: _draft.enabled && !_saving && _supported,
                          busy: _saving,
                          selected: profile.id == _draft.profileId,
                          saved:
                              _baseline.enabled &&
                              profile.id == _baseline.profileId,
                          current: profile.id == current?.id,
                          needsConnectIp: _l4 && profile.requiresUdp,
                          onAction: (action) =>
                              unawaited(_manage(profile, action)),
                        ),
                    ],
                  ),
                ),
                if (_draft.enabled && _draft.profileId != null)
                  Align(
                    alignment: AlignmentDirectional.centerStart,
                    child: TextButton.icon(
                      onPressed: _saving
                          ? null
                          : () => setState(
                              () => _setDraft(
                                _draft.copyWith(clearSelection: true),
                              ),
                            ),
                      icon: const Icon(LucideIcons.x, size: 18),
                      label: Text(strings.chain('clear')),
                    ),
                  ),
              ],
            ),
          ),
          if (widget.source == ChainSource.warpWireguard && _supported)
            SliverToBoxAdapter(
              child: Padding(
                padding: const EdgeInsets.only(top: 24),
                child: WarpWireguardPanel(
                  key: _warpPanel,
                  controller: _app,
                  endpoint: _selected == null
                      ? null
                      : ChainEndpoint(_selected!.host, _selected!.port),
                  overrideEndpoint: _draft.endpointOverride,
                  onEndpoint: (endpoint) => setState(
                    () => _setDraft(
                      _draft.copyWith(
                        endpointOverride: endpoint,
                        clearEndpoint: endpoint == null,
                      ),
                    ),
                  ),
                  onValid: (valid) =>
                      setState(() => _warpEndpointValid = valid),
                  onGenerated: () => unawaited(_load()),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _ProfileRow extends StatelessWidget {
  const _ProfileRow({
    required this.controller,
    required this.profile,
    required this.enabled,
    required this.busy,
    required this.selected,
    required this.saved,
    required this.current,
    required this.needsConnectIp,
    required this.onAction,
  });
  final AppController controller;
  final ChainProfileSummary profile;
  final bool enabled, busy, selected, saved, current, needsConnectIp;
  final ValueChanged<String> onAction;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final theme = Theme.of(context);
    final tags = <(String, StatusTone)>[
      if (current) ('current', StatusTone.success),
      if (saved) ('saved', StatusTone.brand),
      if (needsConnectIp) ('requires_connect_ip', StatusTone.warning),
    ];
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ChainExitTile<String>(
          tileKey: ValueKey('chain-profile-${profile.id}'),
          value: profile.id,
          enabled: enabled,
          selected: selected,
          title: Text(profile.name),
          subtitle: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                '${profile.host}:${profile.port} · ${profile.transportLabel}',
                style: UsqueTheme.mono(
                  context,
                  color: enabled
                      ? theme.colorScheme.onSurfaceVariant
                      : theme.disabledColor,
                ),
              ),
              if (tags.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: Wrap(
                    spacing: 12,
                    runSpacing: 4,
                    children: [
                      for (final (key, tone) in tags)
                        InlineStatus(label: strings.chain(key), tone: tone),
                    ],
                  ),
                ),
            ],
          ),
          trailing: PopupMenuButton<String>(
            key: ValueKey('chain-profile-menu-${profile.id}'),
            tooltip: strings.chain('menu'),
            enabled: !busy,
            onSelected: onAction,
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
                child: Text(strings.chain('delete')),
              ),
            ],
          ),
        ),
        if (selected && enabled)
          Padding(
            padding: const EdgeInsetsDirectional.only(
              start: 56,
              end: 8,
              bottom: 10,
            ),
            child: ExpansionTile(
              key: PageStorageKey('chain-profile-details-${profile.id}'),
              tilePadding: EdgeInsets.zero,
              title: Text(strings.get('technical_details')),
              expandedCrossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                _ProfileDetails(profile: profile, controller: controller),
              ],
            ),
          ),
      ],
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
    final theme = Theme.of(context);
    final note = theme.textTheme.bodySmall?.copyWith(
      color: theme.colorScheme.onSurfaceVariant,
    );
    Widget row(String label, String value) => ReadoutRow(
      label: label,
      stackWhenNarrow: true,
      value: MonoValue(value: value),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      spacing: 6,
      children: [
        row(s.chain('endpoint'), '${profile.host}:${profile.port}'),
        ReadoutRow(
          label: s.chain('transport'),
          value: Text(
            '${profile.transportLabel} · ${profile.addressFamily}',
            textAlign: TextAlign.end,
            style: theme.textTheme.bodyMedium,
          ),
        ),
        if (profile.candidates.length > 1) ...[
          row(
            s.chain('candidates'),
            profile.candidates.map((endpoint) => endpoint.label).join('\n'),
          ),
          Text(
            s.chain(profile.remoteRandom ? 'random_order' : 'file_order'),
            style: note,
          ),
        ],
        if (profile.addresses.isNotEmpty)
          row(s.chain('addresses'), profile.addresses.join('\n')),
        if (profile.dns.isEmpty)
          ReadoutRow(
            label: s.chain('dns'),
            stackWhenNarrow: true,
            value: Text(
              s.chain('dns_fallback'),
              textAlign: TextAlign.end,
              style: theme.textTheme.bodyMedium,
            ),
          )
        else
          row(s.chain('dns'), profile.dns.join('\n')),
        if (profile.allowedIps.isNotEmpty) ...[
          row(s.chain('allowed'), profile.allowedIps.join('\n')),
          Text(s.chain('restricted'), style: note),
        ],
        if (profile.mtu != null) row('MTU', '${profile.mtu}'),
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
    text: widget.credentialsFor?.name ?? '',
  );
  late final _configuration = TextEditingController(
    text: widget.configuration ?? '',
  );
  final _username = TextEditingController(),
      _password = TextEditingController(),
      _keyPassword = TextEditingController();
  ChainProfileSummary? _preview;
  bool _busy = false;
  bool _nameEdited = false;
  bool _showPassword = false, _showKeyPassword = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    if (widget.configuration != null) {
      // A file has already been chosen; checking it is the only next step.
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) unawaited(_submit(false));
      });
    }
  }

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

  /// A cheap structural check that catches a file pasted under the wrong
  /// source before the engine reports an unhelpful parse error.
  String? _sourceMismatch(String text) {
    final lines = text
        .split('\n')
        .map((line) => line.trim().toLowerCase())
        .where((line) => line.isNotEmpty && !line.startsWith('#'))
        .toList();
    final wireguard = lines.any(
      (line) => line == '[interface]' || line == '[peer]',
    );
    final openvpn = lines.any(
      (line) =>
          line == 'client' ||
          line.startsWith('remote ') ||
          line.startsWith('dev ') ||
          line.startsWith('<ca>'),
    );
    return switch (widget.source) {
      ChainSource.openvpnCustom when wireguard && !openvpn =>
        'looks_like_wireguard',
      ChainSource.wireguardCustom || ChainSource.warpWireguard
          when openvpn && !wireguard =>
        'looks_like_openvpn',
      _ => null,
    };
  }

  Future<void> _submit(bool save) async {
    final s = widget.controller.strings;
    if (_busy) return;
    if (utf8.encode(_configuration.text).length > 128 * 1024) {
      setState(() => _error = s.chain('invalid_size_or_encoding'));
      return;
    }
    if (widget.credentialsFor == null) {
      final mismatch = _sourceMismatch(_configuration.text);
      if (mismatch != null) {
        setState(() => _error = s.chain(mismatch));
        return;
      }
    }
    final profile = widget.credentialsFor ?? _preview;
    if (save &&
        (profile?.candidates.length ?? 0) > 1 &&
        !(widget.controller.engineCapabilities?.chainOpenvpnMultiEndpoint ??
            false)) {
      setState(() => _error = s.chain('multi_endpoint_unavailable'));
      return;
    }
    if (save &&
        (profile?.requiresAuth == true &&
                (_username.text.isEmpty || _password.text.isEmpty) ||
            profile?.requiresKeyPassword == true &&
                _keyPassword.text.isEmpty)) {
      setState(() => _error = s.chain('missing_field'));
      return;
    }
    if (save && _name.text.trim().isEmpty) {
      setState(() => _error = s.chain('missing_field'));
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      // The engine validates the name even when only checking. A file is
      // checked before the user has named it, so a check uses a stand-in
      // and the real name is sent only when saving.
      final name = _name.text.trim();
      final result = await widget.controller.chainProfile({
        'action': widget.credentialsFor != null
            ? 'credentials'
            : save
            ? 'import'
            : 'preview',
        'source': widget.source.wire,
        'name': !save && name.isEmpty ? widget.source.label : name,
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
        setState(() => _error = s.chainError(error));
      } else if ((result.preview?.candidates.length ?? 0) > 1 &&
          !(widget.controller.engineCapabilities?.chainOpenvpnMultiEndpoint ??
              false)) {
        setState(() => _error = s.chain('multi_endpoint_unavailable'));
      } else if (save) {
        Navigator.pop(context, result.preview);
      } else {
        setState(() {
          _preview = result.preview;
          if (!_nameEdited && _name.text.trim().isEmpty) {
            _name.text = result.preview?.host ?? widget.source.label;
          }
        });
      }
    } catch (_) {
      if (mounted) setState(() => _error = s.chain('secure_storage_failed'));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Widget _secret(
    TextEditingController controller,
    String label,
    bool visible,
    VoidCallback toggle,
  ) {
    final s = widget.controller.strings;
    return TextField(
      controller: controller,
      enabled: !_busy,
      obscureText: !visible,
      autocorrect: false,
      enableSuggestions: false,
      decoration: InputDecoration(
        labelText: label,
        suffixIcon: IconButton(
          tooltip: s.chain(visible ? 'hide_password' : 'show_password'),
          onPressed: toggle,
          icon: Icon(visible ? LucideIcons.eyeOff : LucideIcons.eye, size: 20),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final s = widget.controller.strings;
    final theme = Theme.of(context);
    final profile = widget.credentialsFor ?? _preview;
    final lines = widget.configuration == null
        ? 0
        : widget.configuration!.trim().isEmpty
        ? 0
        : widget.configuration!.trimRight().split('\n').length;
    return PopScope(
      canPop: !_busy,
      child: UsqueDialog(
        icon: widget.credentialsFor == null
            ? LucideIcons.fileUp
            : LucideIcons.keyRound,
        title: widget.credentialsFor == null
            ? widget.source.label
            : s.chain('credentials'),
        subtitle: widget.credentialsFor?.name,
        content: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (widget.credentialsFor == null) ...[
                if (widget.configuration == null) ...[
                  TextField(
                    controller: _configuration,
                    enabled: !_busy,
                    minLines: 6,
                    maxLines: 12,
                    autofocus: true,
                    autocorrect: false,
                    enableSuggestions: false,
                    style: UsqueTheme.mono(context),
                    decoration: InputDecoration(
                      labelText: s.chain('configuration'),
                      alignLabelWithHint: true,
                    ),
                    onChanged: (_) => setState(() {
                      _preview = null;
                      _error = null;
                    }),
                  ),
                  const SizedBox(height: 16),
                ] else ...[
                  Row(
                    children: [
                      Icon(
                        LucideIcons.fileCheck,
                        size: 18,
                        color: theme.colorScheme.onSurfaceVariant,
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          s
                              .chain('file_loaded')
                              .replaceAll('{lines}', '$lines'),
                          style: theme.textTheme.bodyMedium?.copyWith(
                            color: theme.colorScheme.onSurfaceVariant,
                          ),
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 16),
                ],
                if (_preview == null)
                  Align(
                    alignment: AlignmentDirectional.centerStart,
                    child: OutlinedButton.icon(
                      onPressed: _busy ? null : () => unawaited(_submit(false)),
                      icon: _busy
                          ? const SizedBox.square(
                              dimension: 16,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Icon(LucideIcons.searchCheck, size: 18),
                      label: Text(s.chain(_busy ? 'checking' : 'preview')),
                    ),
                  ),
              ],
              if (profile != null) ...[
                if (widget.credentialsFor == null) ...[
                  const SizedBox(height: 16),
                  TextField(
                    controller: _name,
                    enabled: !_busy,
                    maxLength: 64,
                    autofocus: widget.configuration != null,
                    decoration: InputDecoration(labelText: s.chain('name')),
                    onChanged: (_) => _nameEdited = true,
                  ),
                ],
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
                  _secret(
                    _password,
                    s.chain('password'),
                    _showPassword,
                    () => setState(() => _showPassword = !_showPassword),
                  ),
                ],
                if (profile.requiresKeyPassword) ...[
                  const SizedBox(height: 16),
                  _secret(
                    _keyPassword,
                    s.chain('key_password'),
                    _showKeyPassword,
                    () => setState(() => _showKeyPassword = !_showKeyPassword),
                  ),
                ],
              ],
              BannerSlot(
                spacing: 0,
                child: _error == null
                    ? null
                    : Padding(
                        padding: const EdgeInsets.only(top: 16),
                        child: WarningBanner(
                          key: const ValueKey('chain-import-error'),
                          title: s.get('error'),
                          message: _error!,
                          danger: true,
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
