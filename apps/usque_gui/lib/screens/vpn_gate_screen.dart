import 'dart:async';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import '../core/app_strings.dart';
import '../core/chain_strings.dart';
import '../core/user_facing_errors.dart';
import '../core/vpn_gate_presentation.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/chain_editor_layout.dart';
import '../widgets/common.dart';
import '../widgets/controller_selector.dart';
import '../widgets/unsaved_changes_guard.dart';
import '../widgets/vpn_gate_filters.dart';
import '../widgets/vpn_gate_server_row.dart';
import '../widgets/vpn_gate_summary.dart';

typedef _GateView = ({
  String catalogId,
  ConnectionPhase phase,
  VpnGateStatus gate,
  ChainExitStatus chain,
  String? warning,
  String? error,
  bool tcpSupported,
  bool favoritesSupported,
  bool appliedEnabled,
  String? networkMessage,
});

class VpnGateScreen extends StatefulWidget {
  const VpnGateScreen({
    required this.controller,
    this.active = true,
    this.leaveGuardKey,
    this.now = DateTime.now,
    this.chainPageBuilder,
    this.initialDraft,
    this.initialServer,
    this.initialEnabled,
    this.onDraftChanged,
    this.otherPending = false,
    super.key,
  });
  final AppController controller;
  final bool active;
  final GlobalKey<UnsavedChangesGuardState>? leaveGuardKey;
  final DateTime Function() now;

  /// Present when this view is one source on the chain proxy page.
  final ChainPageBuilder? chainPageBuilder;

  /// A draft retained by the chain page from an earlier visit to this source.
  final VpnGateSettings? initialDraft;
  final VpnGateServer? initialServer;

  /// The chain page switch as the user last left it under another source.
  final bool? initialEnabled;

  /// Lets the chain page retain the draft across source switches.
  final void Function(VpnGateSettings draft, VpnGateServer? server)?
  onDraftChanged;

  /// Whether another source on the chain page holds an unapplied draft, so
  /// leaving the page still asks before discarding it.
  final bool otherPending;

  /// Whether [draft] would change what is stored, seen from the chain page.
  static bool chainPending(UsqueProfile profile, VpnGateSettings draft) =>
      draft != chainBaseline(profile);

  /// The stored settings as the chain page sees them: the page switch belongs
  /// to the chain as a whole, and a server counts as selected only while
  /// VPN Gate is the stored source. Looking at this source while another one
  /// is stored is therefore not an edit; picking a server is.
  static VpnGateSettings chainBaseline(UsqueProfile profile) =>
      profile.chainSource == ChainSource.vpnGate
      ? profile.vpnGate.copyWith(enabled: profile.chainEnabled)
      : VpnGateSettings(enabled: profile.chainEnabled);

  @override
  State<VpnGateScreen> createState() => _VpnGateScreenState();
}

class _VpnGateScreenState extends State<VpnGateScreen>
    with WidgetsBindingObserver {
  late VpnGateSettings _draft, _baseline;
  VpnGateServer? _draftServer;
  VpnGateSettings? _preparedDraft;
  VpnGateDirectory _directory = const VpnGateDirectory();
  // Directory text needs no persisted scroll offset. A storage boundary with
  // no descendant PageStorageKeys keeps its internal scrollables from sharing
  // the page's double offset or the failure tile's bool expansion state.
  final _directoryTextStorage = PageStorageBucket();
  Timer? _hourly, _poll, _ageTick;
  String _country = 'ALL';
  bool _favoritesOnly = false;
  String? _nodeOperation, _nodeError;
  int _offset = 0, _query = 0;
  int _refreshGeneration = 0;
  Future<void>? _refreshRequest;
  bool _cancellingRefresh = false;
  bool _loading = false,
      _pollingRefresh = false,
      _saving = false,
      _ownsRefresh = false,
      _appResumed = true;
  bool get _foreground => _appResumed && widget.active;
  String? _fetchError, _saveError;
  bool get _onChainPage => widget.chainPageBuilder != null;
  VpnGateSettings get _stored => _onChainPage
      ? VpnGateScreen.chainBaseline(_controller.activeProfile)
      : _controller.activeProfile.vpnGate;
  bool get _dirty => _onChainPage
      ? VpnGateScreen.chainPending(_controller.activeProfile, _draft)
      : _draft != _baseline;
  AppController get _controller => widget.controller;

  /// Replaces the draft and reports it to the chain page.
  void _updateDraft(VpnGateSettings draft, VpnGateServer? server) {
    _draft = draft;
    _draftServer = server;
    widget.onDraftChanged?.call(draft, server);
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _baseline = _stored;
    _draft =
        widget.initialDraft ??
        _baseline.copyWith(enabled: widget.initialEnabled);
    _draftServer = widget.initialServer;
    _controller.addListener(_settingsChanged);
    unawaited(_load(refreshIfOld: true));
    _hourly = Timer.periodic(const Duration(hours: 1), (_) {
      if (_foreground) unawaited(_refresh());
    });
    _ageTick = Timer.periodic(const Duration(minutes: 1), (_) {
      if (_foreground) setState(() {});
    });
    _poll = Timer.periodic(const Duration(seconds: 1), (_) {
      if (_foreground &&
          !_loading &&
          !_pollingRefresh &&
          _refreshRequest == null &&
          !_cancellingRefresh &&
          _nodeOperation == null &&
          (_ownsRefresh || _directory.refreshing)) {
        unawaited(_pollRefresh());
      }
    });
  }

  void _settingsChanged() {
    final settings = _stored;
    if (!mounted || _saving || settings == _baseline) return;
    setState(() {
      if (!_dirty) {
        _updateDraft(
          settings,
          _directory.savedServer?.matches(settings) == true
              ? _directory.savedServer
              : null,
        );
      }
      _baseline = settings;
    });
  }

  @override
  void didUpdateWidget(covariant VpnGateScreen oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.active != widget.active) _visibilityChanged();
    if (oldWidget.controller == _controller) return;
    _abandonRefresh(oldWidget.controller);
    oldWidget.controller.removeListener(_settingsChanged);
    _controller.addListener(_settingsChanged);
    _baseline = _stored;
    _updateDraft(_baseline, null);
    unawaited(_load(refreshIfOld: true));
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _appResumed = state == AppLifecycleState.resumed;
    _visibilityChanged();
  }

  void _visibilityChanged() {
    _query++;
    _loading = false;
    if (_foreground) {
      unawaited(_load(refreshIfOld: true));
    } else if (_ownsRefresh) {
      unawaited(_cancelRefresh());
    }
    if (!_foreground && _nodeOperation != null) unawaited(_cancelNode());
  }

  @override
  void dispose() {
    _query++;
    WidgetsBinding.instance.removeObserver(this);
    _controller.removeListener(_settingsChanged);
    _hourly?.cancel();
    _poll?.cancel();
    _ageTick?.cancel();
    if (_nodeOperation != null) unawaited(_cancelNode(rebuild: false));
    unawaited(_releaseDraft());
    _abandonRefresh(_controller);
    super.dispose();
  }

  Future<void> _load({bool refreshIfOld = false}) async {
    if (!mounted || !_foreground) return;
    final query = ++_query;
    final refreshGeneration = _refreshGeneration;
    final startedDuringRefreshRequest = _refreshRequest != null;
    setState(() => _loading = true);
    try {
      final value = await _controller.listVpnGate(
        favoritesOnly: _favoritesOnly,
        countryCode: _country == 'ALL' || _country == 'UNKNOWN'
            ? null
            : _country,
        unknownCountry: _country == 'UNKNOWN',
        offset: _offset,
      );
      if (!mounted ||
          !_foreground ||
          query != _query ||
          refreshGeneration != _refreshGeneration) {
        return;
      }
      if (_offset > 0 && _offset >= value.total) {
        _offset = 0;
        await _load(refreshIfOld: refreshIfOld);
        return;
      }
      setState(() {
        _directory = value;
        _fetchError = null;
        if (!startedDuringRefreshRequest &&
            _refreshRequest == null &&
            const [
              'complete',
              'failed',
              'cancelled',
            ].contains(value.refreshStage)) {
          _ownsRefresh = false;
        }
        if (_draftServer == null &&
            value.savedServer?.matches(_draft) == true) {
          _draftServer = value.savedServer;
        }
      });
      if (refreshIfOld &&
          (value.fetchedAt == null ||
              DateTime.now().difference(value.fetchedAt!) >
                  const Duration(hours: 1))) {
        await _refresh();
      }
    } on Object {
      if (mounted && query == _query) {
        setState(() => _fetchError = 'gate_fetch_error');
      }
      if (refreshIfOld && mounted && query == _query) await _refresh();
    } finally {
      if (mounted && query == _query) setState(() => _loading = false);
    }
  }

  Future<void> _pollRefresh() async {
    final query = _query;
    _pollingRefresh = true;
    try {
      // A directory query validates and sorts the entire pool before paging.
      // Status-only queries bypass that work and keep the visible rows intact.
      final value = await _controller.listVpnGate(statusOnly: true, limit: 1);
      if (!mounted ||
          !_foreground ||
          query != _query ||
          _nodeOperation != null) {
        return;
      }
      if (!value.refreshing) await _load();
    } on Object {
      if (mounted && _foreground && query == _query && _fetchError == null) {
        setState(() => _fetchError = 'gate_fetch_error');
      }
    } finally {
      _pollingRefresh = false;
    }
  }

  Future<void> _refresh() async {
    if (!mounted ||
        _ownsRefresh ||
        _cancellingRefresh ||
        !_foreground ||
        _nodeOperation != null ||
        _saving) {
      return;
    }
    final generation = ++_refreshGeneration;
    _query++;
    final controller = _controller;
    setState(() {
      _loading = false;
      _ownsRefresh = true;
      _fetchError = null;
    });
    final request = controller.refreshVpnGate();
    _refreshRequest = request;
    try {
      await request;
    } on Object {
      if (mounted && generation == _refreshGeneration) {
        setState(() {
          _ownsRefresh = false;
          _fetchError = 'gate_fetch_error';
        });
      }
    } finally {
      if (identical(_refreshRequest, request)) _refreshRequest = null;
    }
  }

  Future<void> _cancelRefresh() async {
    if (_cancellingRefresh) return;
    final generation = ++_refreshGeneration;
    _query++;
    final controller = _controller;
    final pending = _refreshRequest;
    setState(() {
      _cancellingRefresh = true;
      _loading = false;
    });
    try {
      await _stopRefresh(controller, pending);
    } on Object {
      if (mounted && generation == _refreshGeneration) {
        setState(() => _fetchError = 'gate_fetch_error');
      }
    }
    if (mounted && generation == _refreshGeneration) {
      setState(() {
        _ownsRefresh = false;
        _cancellingRefresh = false;
      });
      await _load();
    }
  }

  Future<void> _stopRefresh(
    AppController controller,
    Future<void>? pending,
  ) async {
    try {
      await pending;
    } on Object {
      // A failed acknowledgement may still have started the native refresh.
    }
    await controller.refreshVpnGate(cancel: true);
  }

  void _abandonRefresh(AppController controller) {
    _refreshGeneration++;
    if (_ownsRefresh && !_cancellingRefresh) {
      unawaited(
        _stopRefresh(controller, _refreshRequest).catchError((Object _) {}),
      );
    }
    _ownsRefresh = false;
    _cancellingRefresh = false;
    _refreshRequest = null;
  }

  Future<void> _save() async {
    setState(() {
      _saving = true;
      _saveError = null;
    });
    final target = _draft;
    final account = _controller.activeProfile.id;
    final intent = _controller.connectionIntent;
    if (target.enabled &&
        !await _runNode('prepare', target.serverId, target.configSha256)) {
      if (mounted) setState(() => _saving = false);
      return;
    }
    if (!mounted ||
        account != _controller.activeProfile.id ||
        intent != _controller.connectionIntent ||
        target != _draft) {
      if (mounted) setState(() => _saving = false);
      return;
    }
    final stored = _controller.activeProfile.vpnGate;
    final saved = await _controller.saveNetwork(
      _controller.activeProfile.copyWith(
        // Disabling from the chain page keeps the last VPN Gate server.
        vpnGate: target.hasSelection
            ? target
            : stored.copyWith(enabled: target.enabled),
        chainExit: widget.chainPageBuilder == null
            ? null
            : ChainExitSettings(
                enabled: target.enabled,
                source: ChainSource.vpnGate,
              ),
      ),
      changedFields: [
        'vpn_gate',
        if (widget.chainPageBuilder != null) 'chain_exit',
      ],
    );
    if (!mounted) return;
    setState(() {
      _saving = false;
      if (saved) {
        _baseline = _stored;
        if (_draft != _baseline) {
          _updateDraft(
            _baseline,
            _directory.savedServer?.matches(_baseline) == true
                ? _directory.savedServer
                : _draftServer,
          );
        }
      } else {
        _saveError =
            _controller.networkSettings.saveError == 'VPN_GATE_SELECTION_STALE'
            ? 'gate_select_again'
            : 'gate_save_error';
      }
    });
    if (saved) {
      unawaited(_releaseDraft());
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            _controller.networkSettingsMessage ??
                _controller.strings.get('settings_unknown'),
          ),
        ),
      );
    }
  }

  Future<void> _releaseDraft() async {
    final selection = _preparedDraft;
    _preparedDraft = null;
    if (selection == null) return;
    try {
      await _controller.vpnGateNode(
        VpnGateNodeRequest(
          operationId: _controller.newVpnGateOperationId(),
          action: 'release',
          serverId: selection.serverId,
          configSha256: selection.configSha256,
        ),
      );
    } on Object {
      // Service startup also sweeps abandoned temporary references.
    }
  }

  Future<void> _cancelNode({bool rebuild = true}) async {
    final operation = _nodeOperation;
    _nodeOperation = null;
    if (operation == null) return;
    if (mounted && rebuild) setState(() {});
    try {
      await _controller.vpnGateNode(
        VpnGateNodeRequest(operationId: operation, action: 'cancel'),
      );
    } on Object {
      /* A disconnected service cannot complete a UI apply. */
    }
  }

  Future<bool> _runNode(
    String action,
    String id,
    String hash, {
    String expectedHash = '',
  }) async {
    if (_nodeOperation != null) return false;
    final operation = _controller.newVpnGateOperationId();
    final account = _controller.activeProfile.id;
    final intent = _controller.connectionIntent;
    setState(() {
      _nodeOperation = operation;
      _nodeError = null;
      _saveError = null;
    });
    try {
      await _controller.vpnGateNode(
        VpnGateNodeRequest(
          operationId: operation,
          action: action,
          serverId: id,
          configSha256: hash,
          expectedFavoriteHash: expectedHash,
        ),
      );
      while (mounted && _nodeOperation == operation && _foreground) {
        if (account != _controller.activeProfile.id ||
            intent != _controller.connectionIntent) {
          await _cancelNode();
          return false;
        }
        final progress = (await _controller.listVpnGate(
          limit: 1,
          statusOnly: true,
        )).nodeProgress;
        if (!mounted || _nodeOperation != operation) return false;
        if (progress.operationId != operation) {
          throw StateError('The node operation was replaced.');
        }
        if (progress.stage == 'complete') {
          if (action == 'prepare') {
            _preparedDraft = VpnGateSettings(serverId: id, configSha256: hash);
          }
          return true;
        }
        if (progress.stage == 'cancelled') return false;
        if (progress.stage == 'failed') {
          throw StateError(progress.error ?? 'Node preparation failed.');
        }
        await Future<void>.delayed(const Duration(milliseconds: 400));
      }
      return false;
    } on Object catch (error) {
      if (mounted && _nodeOperation == operation) {
        setState(() {
          _saveError = 'gate_prepare_error';
          _nodeError = userFacingError(_controller.strings, error);
        });
      }
      await _cancelNode();
      return false;
    } finally {
      if (mounted && _nodeOperation == operation) {
        setState(() => _nodeOperation = null);
      }
      if (mounted) unawaited(_load());
    }
  }

  Future<void> _favorite(VpnGateServer server, {bool update = false}) async {
    final favorite = server.favorite;
    final selected = _draft.copyWith(enabled: false);
    if (favorite != null &&
        selected.serverId == server.id &&
        selected.configSha256 == favorite.configSha256 &&
        _preparedDraft != selected &&
        _nodeOperation == null) {
      // Retain the user's exact local draft before its favorite is changed.
      if (!await _runNode('prepare', server.id, favorite.configSha256)) return;
    }
    if (favorite != null && !update) {
      try {
        await _controller.vpnGateNode(
          VpnGateNodeRequest(
            operationId: _controller.newVpnGateOperationId(),
            action: 'remove_favorite',
            serverId: server.id,
            configSha256: favorite.configSha256,
            expectedFavoriteHash: favorite.configSha256,
          ),
        );
        if (mounted) await _load();
      } on Object {
        if (mounted) setState(() => _saveError = 'gate_prepare_error');
      }
      return;
    }
    await _runNode(
      update ? 'update_favorite' : 'favorite',
      server.id,
      update ? favorite!.latestConfigSha256! : server.configSha256,
      expectedHash: favorite?.configSha256 ?? '',
    );
  }

  @override
  Widget build(BuildContext context) => ControllerSelector<_GateView>(
    controller: _controller,
    active: (_) => _foreground,
    // Throughput and network-quality samples do not change this page.
    selector: (controller) => (
      catalogId: controller.strings.catalogId,
      phase: controller.snapshot.phase,
      gate: controller.snapshot.vpnGate,
      chain: controller.snapshot.chainExit,
      warning: controller.snapshot.warning,
      error: controller.lastError,
      tcpSupported: controller.engineCapabilities?.vpnGateTcp ?? false,
      favoritesSupported:
          controller.engineCapabilities?.vpnGatePoolFavorites ?? false,
      appliedEnabled:
          controller.networkSettings.state?.appliedProfile?.vpnGate.enabled ??
          false,
      networkMessage: controller.networkSettingsMessage,
    ),
    builder: (context, _) {
      final strings = _controller.strings;
      final snapshot = _controller.snapshot;
      final view = VpnGatePresentation(
        snapshot,
        configuredEnabled: _controller.activeProfile.vpnGate.enabled,
      );
      final action = !snapshot.isConnected
          ? 'gate_save'
          : !_draft.enabled
          ? 'gate_disable_reconnect'
          : snapshot.vpnGate.connected ||
                _controller
                        .networkSettings
                        .state
                        ?.appliedProfile
                        ?.vpnGate
                        .enabled ==
                    true
          ? 'gate_switch'
          : 'gate_enable_reconnect';
      final refreshing = _ownsRefresh || _directory.refreshing;
      final supported =
          (_controller.engineCapabilities?.vpnGateTcp ?? false) &&
          (_controller.engineCapabilities?.vpnGatePoolFavorites ?? false);
      final onEnabledChanged = _saving || !supported
          ? null
          : (bool enabled) => setState(
              () =>
                  _updateDraft(_draft.copyWith(enabled: enabled), _draftServer),
            );
      final bottomBar = VpnGateSelectionBar(
        strings: strings,
        chainPage: _onChainPage,
        statusLabel:
            !_dirty ||
                _controller.networkSettings.unconfirmed ||
                _controller.networkSettings.saveError != null
            ? _controller.networkSettingsMessage
            : null,
        draft: _draft,
        savedEnabled: _baseline.enabled,
        dirty: _dirty,
        connected: snapshot.isConnected,
        saving: _saving,
        preparing: _nodeOperation != null,
        actionKey: action,
        server: _draftServer,
        saveError: _saveError,
        nodeError: _nodeError,
        onCancel: _cancelNode,
        onApply:
            _saving ||
                _nodeOperation != null ||
                !_dirty ||
                snapshot.isTransitional ||
                _draft.enabled && (!_draft.hasSelection || !supported)
            ? null
            : _save,
      );
      final refreshAction = TextButton.icon(
        onPressed: _nodeOperation != null || _cancellingRefresh
            ? null
            : refreshing
            ? _cancelRefresh
            : _refresh,
        icon: Icon(refreshing ? LucideIcons.x : LucideIcons.refreshCw),
        label: Text(strings.get(refreshing ? 'cancel' : 'gate_refresh')),
      );
      final slivers = <Widget>[
        SliverToBoxAdapter(
          child: FocusTraversalGroup(
            child: Builder(
              builder: (context) {
                final onChainPage = widget.chainPageBuilder != null;
                final toggle = SwitchListTile.adaptive(
                  key: const ValueKey('vpn-gate-toggle'),
                  contentPadding: EdgeInsets.zero,
                  title: Text(
                    onChainPage ? strings.chain('enable') : 'WARP → VPN Gate',
                  ),
                  subtitle: Text(strings.get('gate_scope')),
                  value: _draft.enabled,
                  onChanged: _saving || !supported
                      ? null
                      : (enabled) => setState(
                          () => _updateDraft(
                            _draft.copyWith(enabled: enabled),
                            _draftServer,
                          ),
                        ),
                );
                return PanelStack(
                  spacing: onChainPage ? 20 : 28,
                  children: [
                    if (!supported && !onChainPage)
                      WarningBanner(
                        title: strings.get('error'),
                        message: strings.vpnGateUnsupported,
                      ),
                    if (!onChainPage) toggle,
                    if (!onChainPage)
                      if (snapshot.chainExit.currentProfile case final current?)
                        ContentSection(
                          title: strings.chain('current'),
                          children: [
                            Text('${current.source.label} · ${current.name}'),
                            Text(
                              strings.get(
                                snapshot.isConnected
                                    ? 'connected'
                                    : snapshot.isTransitional
                                    ? 'connecting'
                                    : snapshot.phase == ConnectionPhase.error
                                    ? 'error'
                                    : 'disconnected',
                              ),
                            ),
                          ],
                        )
                      else
                        VpnGateConnectionSummary(
                          strings: strings,
                          view: view,
                          error:
                              snapshot.warning ??
                              _controller.lastError ??
                              snapshot.vpnGate.failure,
                        ),
                    ContentSection(
                      key: const ValueKey('chain-gate-directory'),
                      title: strings.get('gate_servers'),
                      gap: onChainPage ? 12 : 16,
                      subtitle:
                          '${strings.get('gate_source_metrics')} ${strings.get('gate_tcp_scope')}',
                      children: [
                        if (onChainPage) ...[
                          Align(
                            alignment: AlignmentDirectional.centerStart,
                            child: OutlinedButton.icon(
                              onPressed: refreshAction.onPressed,
                              icon: Icon(
                                refreshing
                                    ? LucideIcons.x
                                    : LucideIcons.refreshCw,
                              ),
                              label: Text(
                                strings.get(
                                  refreshing ? 'cancel' : 'gate_refresh',
                                ),
                              ),
                            ),
                          ),
                          const SizedBox(height: 12),
                        ],
                        if (_fetchError != null)
                          Padding(
                            padding: const EdgeInsets.only(bottom: 16),
                            child: WarningBanner(
                              title: strings.get('error'),
                              message: strings.get(_fetchError!),
                              danger: true,
                            ),
                          ),
                        VpnGateFilters(
                          strings: strings,
                          favoritesOnly: _favoritesOnly,
                          favoriteCount: _directory.favoriteCount,
                          country: _country,
                          countries: _directory.countries,
                          onScopeChanged: (favorites) {
                            setState(() {
                              _favoritesOnly = favorites;
                              _country = 'ALL';
                              _offset = 0;
                            });
                            unawaited(_load());
                          },
                          onCountryChanged: _saving
                              ? null
                              : (country) {
                                  setState(() {
                                    _country = country;
                                    _offset = 0;
                                  });
                                  unawaited(_load());
                                },
                        ),
                        if (_loading || refreshing)
                          const LinearProgressIndicator(minHeight: 2),
                        if (_directory.servers.isEmpty && !_loading)
                          Padding(
                            padding: const EdgeInsets.symmetric(vertical: 24),
                            child: Text(
                              strings.get(
                                _favoritesOnly
                                    ? 'gate_favorites_empty'
                                    : 'gate_empty',
                              ),
                            ),
                          ),
                      ],
                    ),
                  ],
                );
              },
            ),
          ),
        ),
        SliverList.builder(
          itemCount: _directory.servers.length,
          findChildIndexCallback: (key) {
            if (key is! ValueKey<(String, String)>) return null;
            final index = _directory.servers.indexWhere(
              (server) => (server.id, server.configSha256) == key.value,
            );
            return index < 0 ? null : index;
          },
          itemBuilder: (context, index) =>
              _serverRow(_directory.servers[index], strings),
        ),
        SliverToBoxAdapter(
          child: PanelStack(
            spacing: 28,
            children: [
              Wrap(
                alignment: WrapAlignment.spaceBetween,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  Text(
                    strings
                        .get('gate_paging')
                        .replaceAll(
                          '{current}',
                          '${_directory.total == 0 ? 0 : _offset + 1}–${_offset + _directory.servers.length}',
                        )
                        .replaceAll('{total}', '${_directory.total}'),
                  ),
                  Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      IconButton(
                        tooltip: strings.get('gate_previous'),
                        onPressed: _offset == 0 || _loading
                            ? null
                            : () {
                                setState(
                                  () => _offset = (_offset - 50).clamp(
                                    0,
                                    _directory.total,
                                  ),
                                );
                                unawaited(_load());
                              },
                        icon: const Icon(LucideIcons.chevronLeft),
                      ),
                      IconButton(
                        tooltip: strings.get('gate_next'),
                        onPressed:
                            _offset + _directory.servers.length >=
                                    _directory.total ||
                                _loading
                            ? null
                            : () {
                                setState(() => _offset += 50);
                                unawaited(_load());
                              },
                        icon: const Icon(LucideIcons.chevronRight),
                      ),
                    ],
                  ),
                ],
              ),
              ContentSection(
                title: strings.get('gate_directory'),
                children: [
                  Text(
                    '${strings.get('gate_source_fetched')}: ${_time(_directory.sourceFetchedAt)}',
                  ),
                  if (_directory.sourceFetchedAt != null &&
                      DateTime.now().difference(_directory.sourceFetchedAt!) >
                          const Duration(hours: 3))
                    Text(
                      strings.get(
                        DateTime.now().difference(_directory.sourceFetchedAt!) >
                                const Duration(hours: 24)
                            ? 'gate_source_very_old'
                            : 'gate_source_old',
                      ),
                    ),
                  ExpansionTile(
                    key: const PageStorageKey<String>(
                      'vpn-gate-source-details',
                    ),
                    title: Text(strings.get('technical_details')),
                    expandedCrossAxisAlignment: CrossAxisAlignment.stretch,
                    childrenPadding: const EdgeInsets.all(12),
                    children: [
                      Text(
                        '${strings.get('gate_received')}: ${_directory.fetchedAt?.toLocal().toString().split('.').first ?? '—'}',
                      ),
                      PageStorage(
                        bucket: _directoryTextStorage,
                        child: SelectableText(
                          '${strings.get('gate_source')}: ${_directory.sourceUrl ?? '—'}',
                        ),
                      ),
                      Text(
                        strings.get(
                          _directory.fetchedAt == null
                              ? 'gate_no_cache'
                              : _directory.cached
                              ? 'gate_cached'
                              : 'gate_verified',
                        ),
                      ),
                      Text(strings.get('gate_freshness')),
                    ],
                  ),
                  Align(
                    alignment: AlignmentDirectional.centerStart,
                    child: TextButton(
                      onPressed: () => showLicensePage(
                        context: context,
                        applicationName: 'Usque',
                      ),
                      child: Text(
                        MaterialLocalizations.of(context).licensesPageTitle,
                      ),
                    ),
                  ),
                  if (_directory.failures.isNotEmpty)
                    ExpansionTile(
                      key: const PageStorageKey<String>(
                        'vpn-gate-directory-failures',
                      ),
                      title: Text(strings.get('gate_fetch_error')),
                      children: [
                        for (final failure in _directory.failures)
                          Padding(
                            padding: const EdgeInsets.all(8),
                            child: PageStorage(
                              bucket: _directoryTextStorage,
                              child: SelectableText(failure),
                            ),
                          ),
                      ],
                    ),
                ],
              ),
            ],
          ),
        ),
      ];
      final page =
          widget.chainPageBuilder?.call(
            enabled: _draft.enabled,
            onEnabledChanged: onEnabledChanged,
            bottomBar: bottomBar,
            warning: !supported
                ? WarningBanner(
                    title: strings.get('error'),
                    message: strings.vpnGateUnsupported,
                  )
                : null,
            slivers: slivers,
          ) ??
          SubPage(
            title: 'VPN Gate',
            actions: [refreshAction],
            subtitle: strings.get('gate_subtitle'),
            backLabel: strings.get('back'),
            contentWidth: 880,
            bottomBar: bottomBar,
            slivers: slivers,
          );
      return UnsavedChangesGuard(
        key: widget.leaveGuardKey,
        strings: strings,
        dirty: _dirty || widget.otherPending,
        saving: _saving,
        child: !_onChainPage
            ? page
            : RadioGroup<(String, String)>(
                groupValue: (_draft.serverId, _draft.configSha256),
                onChanged: (value) {
                  if (value == null ||
                      !_draft.enabled ||
                      _saving ||
                      _nodeOperation != null) {
                    return;
                  }
                  final server = _directory.servers
                      .where(
                        (server) => (server.id, server.configSha256) == value,
                      )
                      .firstOrNull;
                  if (server != null) _selectServer(server);
                },
                child: page,
              ),
      );
    },
  );
  Widget _serverRow(VpnGateServer server, AppStrings strings) {
    final selected = server.matches(_draft);
    final enabled = _draft.enabled && !_saving && _nodeOperation == null;
    final supported =
        _controller.engineCapabilities?.vpnGatePoolFavorites ?? false;
    final favorite = server.favorite;
    return VpnGateServerRow(
      key: ValueKey((server.id, server.configSha256)),
      server: server,
      strings: strings,
      now: widget.now(),
      selected: selected,
      chainLayout: _onChainPage,
      saved:
          _onChainPage &&
          _controller.activeProfile.chainSource == ChainSource.vpnGate &&
          _baseline.enabled &&
          server.matches(_baseline),
      current:
          _onChainPage &&
          _controller.snapshot.isConnected &&
          _controller.snapshot.chainExit.currentProfile == null &&
          _controller.snapshot.vpnGate.connected &&
          _controller.snapshot.vpnGate.server?.id == server.id &&
          _controller.snapshot.vpnGate.server?.configSha256 ==
              server.configSha256,
      onSelect: !enabled ? null : () => _selectServer(server),
      onFavorite:
          !supported ||
              (favorite == null && (_saving || _nodeOperation != null))
          ? null
          : () => _favorite(server),
      onUpdateFavorite: !supported || _saving || _nodeOperation != null
          ? null
          : () => _favorite(server, update: true),
    );
  }

  void _selectServer(VpnGateServer server) {
    if (!server.matches(_draft)) unawaited(_releaseDraft());
    setState(() {
      _updateDraft(_draft.copyWith(server: server), server);
      _saveError = null;
    });
  }

  String _time(DateTime? value) =>
      value?.toLocal().toString().split('.').first ?? '—';
}
