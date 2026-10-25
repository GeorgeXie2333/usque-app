import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import '../core/chain_strings.dart';
import '../core/proton_strings.dart';
import '../core/usque_motion.dart';
import '../core/usque_theme.dart';
import '../core/warp_strings.dart';
import '../models/app_models.dart';
import '../services/engine_client.dart';
import '../state/app_controller.dart';
import '../widgets/chain_current_connection.dart';
import '../widgets/chain_editor_layout.dart';
import '../widgets/chain_source_picker.dart';
import '../widgets/common.dart';
import '../widgets/proton_vpn_panel.dart';
import '../widgets/save_changes_bar.dart';
import '../widgets/unsaved_changes_guard.dart';
import '../widgets/usque_dialog.dart';
import '../widgets/warp_wireguard_panel.dart';
import 'vpn_gate_chain_editor.dart';

part 'chain_batch_import.dart';
part 'chain_proxy_form.dart';

class ChainProxyScreen extends StatefulWidget {
  const ChainProxyScreen({
    required this.controller,
    this.active = true,
    this.now = DateTime.now,
    super.key,
  });
  final AppController controller;
  final bool active;
  final DateTime Function() now;
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
  final _guard = GlobalKey<UnsavedChangesGuardState>();
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

  /// Configuration text that belonged to another source, reopened there.
  _CarriedImport? _carried;
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
        VpnGateChainEditor.chainPending(
          profile,
          _gateDraft!.copyWith(enabled: _enabled),
        );
  }

  void _switch(ChainSource? source) {
    if (source == null || source == _source) return;
    setState(() => _source = source);
  }

  /// Moves an import to the source its text belongs to and reopens it there.
  void _switchWithImport(ChainSource source, _CarriedImport carried) {
    if (source == _source) return;
    setState(() {
      _source = source;
      _carried = carried;
    });
  }

  @override
  Widget build(BuildContext context) {
    if (_source == ChainSource.vpnGate) {
      return VpnGateChainEditor(
        controller: _app,
        active: widget.active,
        now: widget.now,
        leaveGuardKey: _guard,
        chainPageBuilder: _buildPage,
        initialDraft: _gateDraft?.copyWith(enabled: _enabled),
        initialServer: _gateServer,
        initialEnabled: _enabled,
        onDraftChanged: _retainGate,
        otherPending: _otherPending(ChainSource.vpnGate),
      );
    }
    // The carried import opens once, in the editor created for its source.
    final carried = _carried;
    _carried = null;
    return _CustomChainEditor(
      key: ValueKey(_source),
      controller: _app,
      source: _source,
      pageBuilder: _buildPage,
      guard: _guard,
      initialDraft: _initialDraft(_source),
      onDraftChanged: _retain,
      otherPending: _otherPending(_source),
      carriedImport: carried,
      onSwitchWithImport: _switchWithImport,
    );
  }

  Widget _buildPage({
    required bool enabled,
    required ValueChanged<bool>? onEnabledChanged,
    required Widget? bottomBar,
    required List<Widget> slivers,
    Widget? warning,
  }) => SubPage(
    title: _app.strings.chain('title'),
    subtitle: _app.strings.chain('subtitle'),
    backLabel: _app.strings.get('back'),
    contentWidth: 880,
    // With nothing to apply or report, the bar would only hold a disabled
    // button, so it appears with the first edit, save or status instead.
    bottomBar: AnimatedSwitcher(
      duration: UsqueMotion.of(context, UsqueMotion.fast),
      switchInCurve: UsqueMotion.standard,
      switchOutCurve: UsqueMotion.exit,
      transitionBuilder: (child, animation) => SizeTransition(
        sizeFactor: animation,
        alignment: AlignmentDirectional.topStart,
        child: child,
      ),
      child: bottomBar == null
          ? const SizedBox(width: double.infinity)
          : ConstrainedBox(
              key: const ValueKey('chain-bottom-bar'),
              constraints: BoxConstraints(
                maxHeight: MediaQuery.sizeOf(context).height * .45,
              ),
              child: SingleChildScrollView(primary: false, child: bottomBar),
            ),
    ),
    slivers: [
      SliverToBoxAdapter(
        child: Padding(
          padding: const EdgeInsets.only(bottom: 20),
          child: PanelStack(
            spacing: 20,
            children: [
              RowTileTheme(
                child: SwitchListTile.adaptive(
                  key: ValueKey(
                    _source == ChainSource.vpnGate
                        ? 'vpn-gate-toggle'
                        : 'chain-proxy-toggle',
                  ),
                  contentPadding: EdgeInsets.zero,
                  secondary: const Icon(LucideIcons.link),
                  title: Text(_app.strings.chain('enable')),
                  subtitle: Text(_app.strings.chain('scope')),
                  value: enabled,
                  onChanged: onEnabledChanged,
                ),
              ),
              // The running exit comes before browsing, so a failure stays
              // in view whichever source is open below.
              ChainCurrentConnection(
                controller: _app,
                configuredEnabled: _app.activeProfile.chainEnabled,
              ),
              ChainSourcePicker(
                controller: _app,
                source: _source,
                onChanged: _switch,
              ),
              ?warning,
            ],
          ),
        ),
      ),
      ...slivers,
    ],
  );
}

/// Configuration text the user started importing under another source.
typedef _CarriedImport = ({String text, String? defaultName, bool fromFile});

/// What the import dialog asks the editor to do after it closes.
typedef _ImportOutcome = ({
  ChainProfileSummary? saved,
  bool select,
  ChainSource? switchTo,
  _CarriedImport? carried,
});

class _CustomChainEditor extends StatefulWidget {
  const _CustomChainEditor({
    required this.controller,
    required this.source,
    required this.pageBuilder,
    required this.guard,
    required this.initialDraft,
    required this.onDraftChanged,
    required this.otherPending,
    required this.onSwitchWithImport,
    this.carriedImport,
    super.key,
  });
  final AppController controller;
  final ChainSource source;
  final ChainPageBuilder pageBuilder;
  final GlobalKey<UnsavedChangesGuardState> guard;
  final ChainExitSettings initialDraft;
  final ValueChanged<ChainExitSettings> onDraftChanged;
  final bool otherPending;
  final _CarriedImport? carriedImport;
  final void Function(ChainSource, _CarriedImport) onSwitchWithImport;
  @override
  State<_CustomChainEditor> createState() => _CustomChainEditorState();
}

class _CustomChainEditorState extends State<_CustomChainEditor> {
  final _warpPanel = GlobalKey<WarpWireguardPanelState>();
  final _protonPanel = GlobalKey<ProtonVpnPanelState>();
  late ChainExitSettings _baseline, _draft;
  List<ChainProfileSummary> _profiles = const [];
  bool _loading = true, _saving = false, _importing = false;
  bool _warpEndpointValid = true;
  bool _wasSupported = false;

  /// Result of the last apply, shown in the action bar.
  String? _error;

  /// Import, file and library problems, shown beside the import actions.
  String? _libraryError;
  AppController get _app => widget.controller;
  bool get _dirty => _chainPending(_baseline, _draft);
  bool get _supported =>
      ChainSourcePicker.available(_app.engineCapabilities, widget.source) &&
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
    if (widget.carriedImport case final carried?) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) unawaited(_openImport(carried));
      });
    }
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

  Future<void> _addProxy() async {
    final outcome = await showDialog<_ImportOutcome>(
      context: context,
      builder: (_) => _ProxyDialog(controller: _app, source: widget.source),
    );
    if (mounted) await _finishImport(outcome);
  }

  /// Reloads the library and, when asked, selects the saved configuration
  /// as an unapplied draft. Saving alone never changes a connection.
  Future<void> _finishImport(_ImportOutcome? outcome) async {
    if (outcome?.switchTo case final source?) {
      widget.onSwitchWithImport(source, outcome!.carried!);
      return;
    }
    await _load();
    final saved = outcome?.saved;
    if (!mounted || saved == null || !outcome!.select || _saving) return;
    if (!_profiles.any((profile) => profile.id == saved.id)) return;
    setState(
      () => _setDraft(
        _draft.copyWith(
          enabled: true,
          profileId: saved.id,
          revision: saved.revision,
        ),
      ),
    );
  }

  Future<void> _openImport(_CarriedImport carried) async {
    setState(() => _libraryError = null);
    final outcome = await showDialog<_ImportOutcome>(
      context: context,
      builder: (_) => _ImportDialog(
        controller: _app,
        source: widget.source,
        configuration: carried.fromFile ? carried.text : null,
        initialText: carried.fromFile ? null : carried.text,
        defaultName: carried.defaultName,
      ),
    );
    if (mounted) await _finishImport(outcome);
  }

  Future<void> _import(bool file) async {
    if (_importing) return;
    setState(() => _importing = true);
    try {
      await _importFilesOrText(file);
    } finally {
      if (mounted) setState(() => _importing = false);
    }
  }

  Future<void> _importFilesOrText(bool file) async {
    if (!file) {
      await _openImport((text: '', defaultName: null, fromFile: false));
      return;
    }
    final String? text;
    final String defaultName;
    try {
      final files = await _app.pickChainConfigurations();
      if (!mounted || files.isEmpty) return;
      if (files.length > 1) {
        setState(() => _libraryError = null);
        await showDialog<void>(
          context: context,
          barrierDismissible: false,
          builder: (_) => _BatchImportDialog(
            controller: _app,
            source: widget.source,
            files: files,
          ),
        );
        if (mounted) await _load();
        return;
      }
      final picked = files.single;
      if (picked.errorCode case final code?) {
        throw EngineException(code, 'Configuration file could not be read.');
      }
      text = picked.configuration;
      defaultName = _chainFileDefaultName(picked.name);
    } catch (error) {
      if (mounted) {
        final key = switch (error) {
          EngineException(code: 'CHAIN_FILE_UNAVAILABLE') => 'file_unavailable',
          EngineException(code: 'CHAIN_FILE_TOO_LARGE') =>
            'invalid_size_or_encoding',
          EngineException(code: 'CHAIN_FILE_ENCODING_INVALID') =>
            'file_encoding_invalid',
          EngineException(code: 'CHAIN_FILE_BUSY') => 'file_busy',
          EngineException(code: 'CHAIN_FILE_COUNT_LIMIT') => 'file_count_limit',
          _ => 'file_read_failed',
        };
        setState(() => _libraryError = _app.strings.chain(key));
      }
      return;
    }
    if (text == null || !mounted) return;
    await _openImport((text: text, defaultName: defaultName, fromFile: true));
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
    if (action == 'details') {
      await showDialog<void>(
        context: context,
        builder: (context) => UsqueDialog(
          icon: LucideIcons.info,
          title: strings.get('technical_details'),
          subtitle: profile.name,
          content: SingleChildScrollView(
            child: _ProfileDetails(profile: profile, controller: _app),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: Text(strings.get('close')),
            ),
          ],
        ),
      );
      return;
    }
    if (action == 'clear') {
      if (!_saving && profile.id == _draft.profileId) {
        setState(() => _setDraft(_draft.copyWith(clearSelection: true)));
      }
      return;
    }
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
      if (widget.source == ChainSource.protonVpn && action == 'remove') {
        await _protonPanel.currentState?.logout(profile.id);
        if (mounted) await _load();
        return;
      }
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

  Future<bool> _prepareProtonRemoval(List<String> ids) async {
    if (_saving) return false;
    if (ids.contains(_app.snapshot.chainExit.currentProfile?.id)) {
      setState(() => _error = _app.strings.proton('profile_in_use'));
      return false;
    }
    final latest = _app.activeProfile;
    final stored = _storedChainExit(latest);
    if (ids.contains(stored.profileId)) {
      setState(() {
        _saving = true;
        _error = null;
      });
      final saved = await _app.saveNetwork(
        latest.copyWith(
          chainExit: stored.copyWith(enabled: false, clearSelection: true),
        ),
        changedFields: ['chain_exit'],
      );
      if (!mounted) return false;
      setState(() {
        _saving = false;
        if (saved) {
          _baseline = _stored;
        } else {
          _error = _app.lastError ?? _app.strings.chain('error');
        }
      });
      if (!saved) return false;
    }
    if (mounted && ids.contains(_draft.profileId)) {
      setState(
        () => _setDraft(_draft.copyWith(enabled: false, clearSelection: true)),
      );
    }
    return mounted;
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
    final marker = chainCurrentMarker(snapshot);
    final statusLabel = _error != null
        ? null
        : _modeConflict
        ? strings.chain('l4')
        : !_dirty ||
              _app.networkSettings.unconfirmed ||
              _app.networkSettings.saveError != null
        ? _app.networkSettingsMessage
        : null;
    final showBar =
        _dirty ||
        _modeConflict ||
        _saving ||
        _error != null ||
        statusLabel != null ||
        _app.networkSettings.canRefresh;
    final empty = !_loading && profiles.isEmpty;
    final busy = !_supported || _saving;
    // The first way to add a configuration leads while the library is empty.
    Widget action({
      required IconData icon,
      required String label,
      required VoidCallback? onPressed,
      required bool first,
    }) => first && empty
        ? FilledButton.tonalIcon(
            onPressed: onPressed,
            icon: Icon(icon),
            label: Text(label),
          )
        : OutlinedButton.icon(
            onPressed: onPressed,
            icon: Icon(icon),
            label: Text(label),
          );
    final actions = <Widget>[
      if (widget.source == ChainSource.warpWireguard)
        action(
          first: true,
          onPressed: busy
              ? null
              : () async {
                  final state = _warpPanel.currentState;
                  if (state != null) await state.generate();
                },
          icon: LucideIcons.plus,
          label: strings.warp('generate'),
        ),
      if (widget.source.isProxy)
        action(
          first: true,
          onPressed: busy ? null : _addProxy,
          icon: LucideIcons.plus,
          label: strings.chain('add_proxy'),
        ),
      if (!widget.source.isProxy && widget.source != ChainSource.protonVpn) ...[
        action(
          first: widget.source != ChainSource.warpWireguard,
          onPressed: busy || _importing ? null : () => unawaited(_import(true)),
          icon: LucideIcons.fileUp,
          label: strings.chain('import_file'),
        ),
        action(
          first: false,
          onPressed: busy || _importing
              ? null
              : () => unawaited(_import(false)),
          icon: LucideIcons.clipboard,
          label: strings.chain('paste'),
        ),
      ],
    ];
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
        bottomBar: !showBar
            ? null
            : SaveChangesBar(
                key: const ValueKey('chain-proxy-bar'),
                contentWidth: 880,
                matchPageGutter: true,
                strings: strings,
                dirty: _dirty || _modeConflict,
                saving: _saving,
                onRefresh: _app.networkSettings.canRefresh
                    ? _app.networkSettings.refresh
                    : null,
                refreshing: _app.networkSettings.refreshing,
                error: _error,
                validationError: _dirty ? validation : null,
                statusLabel: statusLabel,
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
                            widget.source == ChainSource.protonVpn
                                ? strings.proton('resolve_on_connect')
                                : '${(_draft.endpointOverride ?? ChainEndpoint(_selected!.host, _selected!.port)).label} · ${_selected!.transportLabel}',
                            style: UsqueTheme.address(
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
          if (widget.source == ChainSource.protonVpn && _libraryError != null)
            SliverToBoxAdapter(
              child: Padding(
                padding: const EdgeInsets.only(bottom: 16),
                child: WarningBanner(
                  title: strings.get('error_generic'),
                  message: _libraryError!,
                  danger: true,
                  onDismiss: () => setState(() => _libraryError = null),
                ),
              ),
            ),
          if (widget.source == ChainSource.protonVpn && _supported)
            SliverToBoxAdapter(
              child: Padding(
                padding: const EdgeInsets.only(bottom: 24),
                child: ProtonVpnPanel(
                  key: _protonPanel,
                  controller: _app,
                  onChanged: () => unawaited(_load()),
                  selectedProfileId: _draft.profileId,
                  selectedServerId: _draft.protonServerId,
                  onSelectServer: (id, server) {
                    final profile = profiles
                        .where((p) => p.id == id)
                        .firstOrNull;
                    if (profile == null || _saving || !_supported) return;
                    setState(
                      () => _setDraft(
                        _draft.copyWith(
                          enabled: true,
                          profileId: profile.id,
                          revision: profile.revision,
                          protonServerId: server,
                        ),
                      ),
                    );
                  },
                  savedProfileId: _baseline.profileId,
                  currentProfileId: current?.id,
                  currentStatus: marker == null
                      ? null
                      : (
                          marker.$1 == 'current'
                              ? strings.proton('current_account')
                              : strings.chain(marker.$1),
                          marker.$2,
                        ),
                  selectionEnabled:
                      !_saving &&
                      !_loading &&
                      _libraryError == null &&
                      _supported,
                  beforeRemove: _prepareProtonRemoval,
                  onSelectProfile: (id) {
                    final profile = profiles
                        .where((profile) => profile.id == id)
                        .firstOrNull;
                    if (profile != null && !_saving) {
                      setState(
                        () => _setDraft(
                          _draft.copyWith(
                            enabled: true,
                            profileId: profile.id,
                            revision: profile.revision,
                          ),
                        ),
                      );
                    }
                  },
                ),
              ),
            ),
          if (widget.source != ChainSource.protonVpn)
            SliverToBoxAdapter(
              child: ContentSection(
                title: strings.chain('profiles'),
                gap: 12,
                children: [
                  Wrap(spacing: 8, runSpacing: 8, children: actions),
                  const SizedBox(height: 12),
                  BannerSlot(
                    spacing: 12,
                    child: _libraryError == null
                        ? null
                        : WarningBanner(
                            key: const ValueKey('chain-library-error'),
                            title: strings.get('error_generic'),
                            message: _libraryError!,
                            danger: true,
                            onDismiss: () =>
                                setState(() => _libraryError = null),
                          ),
                  ),
                  if (_loading) const LinearProgressIndicator(minHeight: 2),
                  if (empty)
                    _EmptyLibrary(
                      title: strings.chain(
                        widget.source.isProxy ? 'empty_proxy' : 'empty',
                      ),
                      hints: [
                        strings.chain(
                          widget.source.isProxy
                              ? 'proxy_hint'
                              : widget.source.isWireguard
                              ? 'empty_hint_wireguard'
                              : 'empty_hint_openvpn',
                        ),
                        if (!widget.source.isProxy)
                          strings.chain('import_limits'),
                      ],
                      style: mutedStyle,
                    ),
                  // Choosing a configuration also turns the chain on in the
                  // draft; nothing changes until the draft is applied.
                  RadioGroup<String>(
                    groupValue: _draft.profileId,
                    onChanged: (id) {
                      if (_saving || !_supported || id == null) return;
                      final profile = profiles.firstWhere((p) => p.id == id);
                      setState(
                        () => _setDraft(
                          _draft.copyWith(
                            enabled: true,
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
                            enabled: !_saving && _supported,
                            busy: _saving,
                            selected: profile.id == _draft.profileId,
                            saved:
                                _baseline.enabled &&
                                profile.id == _baseline.profileId,
                            current: profile.id == current?.id ? marker : null,
                            inUse:
                                profile.id == _draft.profileId ||
                                profile.id == _baseline.profileId ||
                                profile.id == current?.id,
                            needsConnectIp: _l4 && profile.requiresUdp,
                            onAction: (action) =>
                                unawaited(_manage(profile, action)),
                          ),
                      ],
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

/// An empty library: what to add and the limits that apply, under one mark.
class _EmptyLibrary extends StatelessWidget {
  const _EmptyLibrary({
    required this.title,
    required this.hints,
    required this.style,
  });
  final String title;
  final List<String> hints;
  final TextStyle? style;

  @override
  Widget build(BuildContext context) => Padding(
    key: const ValueKey('chain-library-empty'),
    padding: const EdgeInsets.only(top: 4, bottom: 8),
    child: Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.only(top: 1),
          child: Icon(
            LucideIcons.inbox,
            size: 20,
            color: Theme.of(context).colorScheme.onSurfaceVariant,
          ),
        ),
        const SizedBox(width: 12),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            spacing: 4,
            children: [
              Text(title),
              for (final hint in hints) Text(hint, style: style),
            ],
          ),
        ),
      ],
    ),
  );
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
    required this.inUse,
    required this.needsConnectIp,
    required this.onAction,
  });
  final AppController controller;
  final ChainProfileSummary profile;
  final bool enabled, busy, selected, saved, inUse, needsConnectIp;

  /// The running session's state when it uses this configuration.
  final (String, StatusTone)? current;
  final ValueChanged<String> onAction;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final theme = Theme.of(context);
    final current = this.current;
    // A connected or connecting session already shows that it uses the saved
    // choice; a failed one keeps both facts visible.
    final live =
        current != null &&
        (current.$2 == StatusTone.success || current.$2 == StatusTone.brand);
    final tags = <(String, StatusTone)>[
      ?current,
      if (saved && !live) ('saved', StatusTone.brand),
      if (needsConnectIp) ('requires_connect_ip', StatusTone.warning),
    ];
    return ChainExitTile<String>(
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
            style: UsqueTheme.address(
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
            value: 'details',
            child: Text(strings.get('technical_details')),
          ),
          if (selected)
            PopupMenuItem(value: 'clear', child: Text(strings.chain('clear'))),
          const PopupMenuDivider(),
          PopupMenuItem(value: 'rename', child: Text(strings.chain('rename'))),
          if (profile.source == ChainSource.openvpnCustom ||
              profile.source.isProxy && profile.requiresAuth)
            PopupMenuItem(
              value: 'credentials',
              child: Text(strings.chain('credentials')),
            ),
          // A configuration in use stays listed with the reason it cannot be
          // deleted yet, rather than failing after the user picks it.
          PopupMenuItem(
            key: ValueKey('chain-profile-delete-${profile.id}'),
            value: 'remove',
            enabled: !inUse,
            child: inUse
                ? Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(strings.chain('delete')),
                      // Inherits the disabled item's colour at a smaller size.
                      Text(
                        strings.chain('delete_blocked'),
                        style: TextStyle(
                          fontSize: theme.textTheme.bodySmall?.fontSize,
                        ),
                      ),
                    ],
                  )
                : Text(strings.chain('delete')),
          ),
        ],
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
              s.chain(
                profile.source.isProxy
                    ? (profile.dnsTransport == 'tcp' ||
                              !(controller
                                      .engineCapabilities
                                      ?.chainProxyEncryptedDns ??
                                  false)
                          ? 'dns_inherit'
                          : 'dns_${profile.dnsTransport}')
                    : 'dns_fallback',
              ),
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
    this.initialText,
    this.defaultName,
    this.credentialsFor,
  });
  final AppController controller;
  final ChainSource source;

  /// Text read from a file; it is checked at once and not edited here.
  final String? configuration;

  /// Editable text to start from, such as a paste moved from another source.
  final String? initialText;
  final String? defaultName;
  final ChainProfileSummary? credentialsFor;
  @override
  State<_ImportDialog> createState() => _ImportDialogState();
}

class _ImportDialogState extends State<_ImportDialog> {
  /// Typing pauses this long before pasted text is checked automatically.
  static const _checkDelay = Duration(milliseconds: 600);

  late final _name = TextEditingController(
    text: widget.credentialsFor?.name ?? '',
  );
  late final _configuration = TextEditingController(
    text: widget.configuration ?? widget.initialText ?? '',
  );
  final _username = TextEditingController(),
      _password = TextEditingController(),
      _keyPassword = TextEditingController();
  ChainProfileSummary? _preview;
  Timer? _checkTimer;
  bool _busy = false, _checking = false;
  bool _nameEdited = false;
  bool _showPassword = false, _showKeyPassword = false;
  String? _error;

  /// The source the text appears to belong to, offered as a switch.
  ChainSource? _belongsTo;

  @override
  void initState() {
    super.initState();
    if (_configuration.text.trim().isNotEmpty) {
      // Text is already present; checking it is the only next step.
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) unawaited(_submit(false));
      });
    }
  }

  @override
  void dispose() {
    _checkTimer?.cancel();
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

  void _edited() {
    _checkTimer?.cancel();
    setState(() {
      _preview = null;
      _error = null;
      _belongsTo = null;
    });
    if (_configuration.text.trim().isEmpty) return;
    _checkTimer = Timer(_checkDelay, () {
      if (mounted) unawaited(_submit(false));
    });
  }

  Future<void> _submit(bool save, {bool select = false}) async {
    final s = widget.controller.strings;
    if (_busy || _checking) return;
    _checkTimer?.cancel();
    final text = _configuration.text;
    if (utf8.encode(text).length > 128 * 1024) {
      setState(() => _error = s.chain('invalid_size_or_encoding'));
      return;
    }
    if (widget.credentialsFor == null) {
      final mismatch = _chainSourceMismatch(widget.source, text);
      if (mismatch != null) {
        setState(() {
          _error = s.chain(mismatch);
          _belongsTo = mismatch == 'looks_like_wireguard'
              ? ChainSource.wireguardCustom
              : ChainSource.openvpnCustom;
        });
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
      // Checking leaves the text editable; a later edit discards the result.
      if (save) {
        _busy = true;
      } else {
        _checking = true;
      }
      _error = null;
      _belongsTo = null;
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
        'configuration': text,
        'username': _username.text,
        'password': _password.text,
        'private_key_password': _keyPassword.text,
      });
      if (!mounted || !save && _configuration.text != text) return;
      if (result.error case final error?) {
        setState(() => _error = s.chainError(error));
      } else if ((result.preview?.candidates.length ?? 0) > 1 &&
          !(widget.controller.engineCapabilities?.chainOpenvpnMultiEndpoint ??
              false)) {
        setState(() => _error = s.chain('multi_endpoint_unavailable'));
      } else if (save) {
        Navigator.pop<_ImportOutcome>(context, (
          saved: result.preview,
          select: select,
          switchTo: null,
          carried: null,
        ));
      } else {
        setState(() {
          _preview = result.preview;
          if (!_nameEdited && _name.text.trim().isEmpty) {
            _name.text =
                widget.defaultName ??
                result.preview?.host ??
                widget.source.label;
          }
        });
      }
    } catch (_) {
      if (mounted) setState(() => _error = s.chain('secure_storage_failed'));
    } finally {
      if (mounted) {
        setState(() {
          _busy = false;
          _checking = false;
        });
        // Text edited during a check is checked again once it settles.
        if (!save &&
            _configuration.text != text &&
            _configuration.text.trim().isNotEmpty &&
            !(_checkTimer?.isActive ?? false)) {
          _edited();
        }
      }
    }
  }

  void _switchSource(ChainSource source) {
    Navigator.pop<_ImportOutcome>(context, (
      saved: null,
      select: false,
      switchTo: source,
      carried: (
        text: _configuration.text,
        defaultName: widget.defaultName,
        fromFile: widget.configuration != null,
      ),
    ));
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
    final importing = widget.credentialsFor == null;
    final canSave = !_busy && !_checking && profile != null;
    final lines = widget.configuration == null
        ? 0
        : widget.configuration!.trim().isEmpty
        ? 0
        : widget.configuration!.trimRight().split('\n').length;
    return PopScope(
      canPop: !_busy,
      child: UsqueDialog(
        icon: importing ? LucideIcons.fileUp : LucideIcons.keyRound,
        title: importing ? widget.source.label : s.chain('credentials'),
        subtitle: widget.credentialsFor?.name,
        content: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (importing) ...[
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
                    onChanged: (_) => _edited(),
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
                      onPressed: _busy || _checking
                          ? null
                          : () => unawaited(_submit(false)),
                      icon: _checking
                          ? const SizedBox.square(
                              dimension: 16,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Icon(LucideIcons.searchCheck, size: 18),
                      label: Text(s.chain(_checking ? 'checking' : 'preview')),
                    ),
                  ),
              ],
              if (profile != null) ...[
                if (importing) ...[
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
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            WarningBanner(
                              key: const ValueKey('chain-import-error'),
                              title: s.get('error_generic'),
                              message: _error!,
                              danger: true,
                            ),
                            if (_belongsTo case final source?
                                when ChainSourcePicker.available(
                                  widget.controller.engineCapabilities,
                                  source,
                                ))
                              Padding(
                                padding: const EdgeInsets.only(top: 8),
                                child: TextButton.icon(
                                  key: const ValueKey('chain-import-switch'),
                                  onPressed: _busy
                                      ? null
                                      : () => _switchSource(source),
                                  icon: const Icon(
                                    LucideIcons.arrowRightLeft,
                                    size: 18,
                                  ),
                                  label: Text(
                                    s
                                        .chain('switch_source')
                                        .replaceAll('{source}', source.label),
                                  ),
                                ),
                              ),
                          ],
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
          if (importing) ...[
            OutlinedButton(
              key: const ValueKey('chain-import-save'),
              onPressed: canSave ? () => unawaited(_submit(true)) : null,
              child: Text(s.chain('save_import')),
            ),
            FilledButton(
              key: const ValueKey('chain-import-save-select'),
              onPressed: canSave
                  ? () => unawaited(_submit(true, select: true))
                  : null,
              child: Text(s.chain('save_select')),
            ),
          ] else
            FilledButton(
              onPressed: canSave ? () => unawaited(_submit(true)) : null,
              child: Text(s.chain('save_import')),
            ),
        ],
      ),
    );
  }
}

String _chainFileDefaultName(String filename) {
  final extension = filename.lastIndexOf('.');
  return extension > 0 ? filename.substring(0, extension) : filename;
}

/// A cheap structural check that catches a file pasted under the wrong
/// source before the engine reports an unhelpful parse error.
String? _chainSourceMismatch(ChainSource source, String text) {
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
  return switch (source) {
    ChainSource.openvpnCustom when wireguard && !openvpn =>
      'looks_like_wireguard',
    ChainSource.wireguardCustom || ChainSource.warpWireguard
        when openvpn && !wireguard =>
      'looks_like_openvpn',
    _ => null,
  };
}
