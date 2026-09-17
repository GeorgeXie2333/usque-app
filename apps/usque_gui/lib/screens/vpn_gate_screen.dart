import 'dart:async';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import '../core/app_strings.dart';
import '../core/user_facing_errors.dart';
import '../core/vpn_gate_presentation.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
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
  String? warning,
  String? error,
  bool tcpSupported,
  bool favoritesSupported,
  bool appliedEnabled,
});

class VpnGateScreen extends StatefulWidget {
  const VpnGateScreen({
    required this.controller,
    this.active = true,
    this.leaveGuardKey,
    this.now = DateTime.now,
    super.key,
  });
  final AppController controller;
  final bool active;
  final GlobalKey<UnsavedChangesGuardState>? leaveGuardKey;
  final DateTime Function() now;
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
  bool _loading = false,
      _pollingRefresh = false,
      _saving = false,
      _ownsRefresh = false,
      _appResumed = true;
  bool get _foreground => _appResumed && widget.active;
  String? _fetchError, _saveError;
  bool get _dirty => _draft != _baseline;
  AppController get _controller => widget.controller;
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _draft = _baseline = _controller.activeProfile.vpnGate;
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
          _nodeOperation == null &&
          (_ownsRefresh || _directory.refreshing)) {
        unawaited(_pollRefresh());
      }
    });
  }

  void _settingsChanged() {
    final settings = _controller.activeProfile.vpnGate;
    if (!mounted || _saving || settings == _baseline) return;
    setState(() {
      if (!_dirty) {
        _draft = settings;
        _draftServer = _directory.savedServer?.matches(settings) == true
            ? _directory.savedServer
            : null;
      }
      _baseline = settings;
    });
  }

  @override
  void didUpdateWidget(covariant VpnGateScreen oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.active != widget.active) _visibilityChanged();
    if (oldWidget.controller == _controller) return;
    oldWidget.controller.removeListener(_settingsChanged);
    _controller.addListener(_settingsChanged);
    _draft = _baseline = _controller.activeProfile.vpnGate;
    _draftServer = null;
    unawaited(_load(refreshIfOld: true));
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _appResumed = state == AppLifecycleState.resumed;
    _visibilityChanged();
  }

  void _visibilityChanged() {
    if (_foreground) {
      unawaited(_load(refreshIfOld: true));
    } else if (_ownsRefresh) {
      unawaited(_cancelRefresh());
    }
    if (!_foreground && _nodeOperation != null) unawaited(_cancelNode());
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _controller.removeListener(_settingsChanged);
    _hourly?.cancel();
    _poll?.cancel();
    _ageTick?.cancel();
    if (_nodeOperation != null) unawaited(_cancelNode(rebuild: false));
    unawaited(_releaseDraft());
    if (_ownsRefresh) {
      unawaited(
        _controller.refreshVpnGate(cancel: true).catchError((Object _) {}),
      );
    }
    super.dispose();
  }

  Future<void> _load({bool refreshIfOld = false}) async {
    if (!mounted || !_foreground) return;
    final query = ++_query;
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
      if (!mounted || query != _query) return;
      if (_offset > 0 && _offset >= value.total) {
        _offset = 0;
        await _load(refreshIfOld: refreshIfOld);
        return;
      }
      setState(() {
        _directory = value;
        _fetchError = null;
        if (const [
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
      if (refreshIfOld && mounted) await _refresh();
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
        !_foreground ||
        _nodeOperation != null ||
        _saving) {
      return;
    }
    setState(() {
      _ownsRefresh = true;
      _fetchError = null;
    });
    try {
      await _controller.refreshVpnGate();
    } on Object {
      if (mounted) {
        setState(() {
          _ownsRefresh = false;
          _fetchError = 'gate_fetch_error';
        });
      }
    }
  }

  Future<void> _cancelRefresh() async {
    try {
      await _controller.refreshVpnGate(cancel: true);
    } on Object {
      if (mounted) setState(() => _fetchError = 'gate_fetch_error');
    }
    if (mounted) {
      setState(() => _ownsRefresh = false);
      await _load();
    }
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
    final saved = await _controller.saveNetwork(
      _controller.activeProfile.copyWith(vpnGate: target),
      changedFields: const ['vpn_gate'],
    );
    if (!mounted) return;
    setState(() {
      _saving = false;
      if (saved) {
        _baseline = _draft;
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
      warning: controller.snapshot.warning,
      error: controller.lastError,
      tcpSupported: controller.engineCapabilities?.vpnGateTcp ?? false,
      favoritesSupported:
          controller.engineCapabilities?.vpnGatePoolFavorites ?? false,
      appliedEnabled:
          controller.networkSettings.state?.appliedProfile?.vpnGate.enabled ??
          false,
    ),
    builder: (context, _) {
      final strings = _controller.strings;
      final snapshot = _controller.snapshot;
      final view = VpnGatePresentation(
        snapshot,
        configuredEnabled: _baseline.enabled,
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
      return UnsavedChangesGuard(
        key: widget.leaveGuardKey,
        strings: strings,
        dirty: _dirty,
        saving: _saving,
        child: SubPage(
          title: 'VPN Gate',
          subtitle: strings.get('gate_subtitle'),
          backLabel: strings.get('back'),
          contentWidth: 880,
          bottomBar: VpnGateSelectionBar(
            strings: strings,
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
          ),
          actions: [
            TextButton.icon(
              onPressed: _nodeOperation != null
                  ? null
                  : refreshing
                  ? _cancelRefresh
                  : _refresh,
              icon: Icon(refreshing ? LucideIcons.x : LucideIcons.refreshCw),
              label: Text(strings.get(refreshing ? 'cancel' : 'gate_refresh')),
            ),
          ],
          slivers: [
            SliverToBoxAdapter(
              child: FocusTraversalGroup(
                child: PanelStack(
                  spacing: 28,
                  children: [
                    if (!supported)
                      WarningBanner(
                        title: strings.get('error'),
                        message: strings.vpnGateUnsupported,
                      ),
                    SwitchListTile.adaptive(
                      key: const ValueKey('vpn-gate-toggle'),
                      contentPadding: EdgeInsets.zero,
                      title: const Text('WARP → VPN Gate'),
                      subtitle: Text(strings.get('gate_scope')),
                      value: _draft.enabled,
                      onChanged: _saving || !supported
                          ? null
                          : (enabled) => setState(
                              () => _draft = _draft.copyWith(enabled: enabled),
                            ),
                    ),
                    VpnGateConnectionSummary(
                      strings: strings,
                      view: view,
                      error:
                          snapshot.warning ??
                          _controller.lastError ??
                          snapshot.vpnGate.failure,
                    ),
                    ContentSection(
                      title: strings.get('gate_servers'),
                      subtitle:
                          '${strings.get('gate_source_metrics')} ${strings.get('gate_tcp_scope')}',
                      children: [
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
                          DateTime.now().difference(
                                _directory.sourceFetchedAt!,
                              ) >
                              const Duration(hours: 3))
                        Text(
                          strings.get(
                            DateTime.now().difference(
                                      _directory.sourceFetchedAt!,
                                    ) >
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
          ],
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
      onSelect: !enabled
          ? null
          : () {
              if (!selected) unawaited(_releaseDraft());
              setState(() {
                _draft = _draft.copyWith(server: server);
                _draftServer = server;
                _saveError = null;
              });
            },
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

  String _time(DateTime? value) =>
      value?.toLocal().toString().split('.').first ?? '—';
}
