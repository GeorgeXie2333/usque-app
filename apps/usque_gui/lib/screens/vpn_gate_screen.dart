import 'dart:async';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import '../core/app_strings.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/unsaved_changes_guard.dart';

class VpnGateScreen extends StatefulWidget {
  const VpnGateScreen({required this.controller, super.key});
  final AppController controller;
  @override
  State<VpnGateScreen> createState() => _VpnGateScreenState();
}

class _VpnGateScreenState extends State<VpnGateScreen>
    with WidgetsBindingObserver {
  late VpnGateSettings _draft, _baseline;
  VpnGateServer? _draftServer;
  VpnGateDirectory _directory = const VpnGateDirectory();
  Timer? _hourly, _poll;
  String _country = 'ALL';
  int _offset = 0, _query = 0;
  bool _loading = false,
      _saving = false,
      _ownsRefresh = false,
      _foreground = true;
  String? _fetchError, _saveError;
  String? get _error => _saveError ?? _fetchError;
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
    _poll = Timer.periodic(const Duration(seconds: 1), (_) {
      if (_foreground && !_loading && (_ownsRefresh || _directory.refreshing)) {
        unawaited(_load());
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
    if (oldWidget.controller == _controller) return;
    oldWidget.controller.removeListener(_settingsChanged);
    _controller.addListener(_settingsChanged);
    _draft = _baseline = _controller.activeProfile.vpnGate;
    _draftServer = null;
    unawaited(_load(refreshIfOld: true));
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _foreground = state == AppLifecycleState.resumed;
    if (_foreground) {
      unawaited(_load(refreshIfOld: true));
    } else if (_ownsRefresh) {
      unawaited(_cancelRefresh());
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _controller.removeListener(_settingsChanged);
    _hourly?.cancel();
    _poll?.cancel();
    if (_ownsRefresh) {
      unawaited(
        _controller.refreshVpnGate(cancel: true).catchError((Object _) {}),
      );
    }
    super.dispose();
  }

  Future<void> _load({bool refreshIfOld = false}) async {
    if (!mounted) return;
    final query = ++_query;
    setState(() => _loading = true);
    try {
      final value = await _controller.listVpnGate(
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

  Future<void> _refresh() async {
    if (!mounted || _ownsRefresh || !_foreground) return;
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
    final saved = await _controller.saveNetwork(
      _controller.activeProfile.copyWith(vpnGate: _draft),
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

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: _controller,
    builder: (context, _) {
      final strings = _controller.strings;
      final snapshot = _controller.snapshot;
      final current = snapshot.isConnected && snapshot.vpnGate.connected
          ? snapshot.vpnGate.server
          : null;
      final action = !snapshot.isConnected
          ? 'gate_save'
          : !_draft.enabled
          ? 'gate_disable_reconnect'
          : _controller
                    .networkSettings
                    .state
                    ?.appliedProfile
                    ?.vpnGate
                    .enabled ==
                true
          ? 'gate_switch'
          : 'gate_enable_reconnect';
      final refreshing = _ownsRefresh || _directory.refreshing;
      final supported = _controller.engineCapabilities?.vpnGateTcp ?? false;
      return UnsavedChangesGuard(
        strings: strings,
        dirty: _dirty,
        saving: _saving,
        child: SubPage(
          title: 'VPN Gate',
          subtitle: strings.get('gate_subtitle'),
          backLabel: strings.get('back'),
          contentWidth: 880,
          bottomBar: SafeArea(
            top: false,
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: FilledButton.icon(
                key: const ValueKey('vpn-gate-apply'),
                onPressed:
                    _saving ||
                        !_dirty ||
                        snapshot.isTransitional ||
                        _draft.enabled && (!_draft.hasSelection || !supported)
                    ? null
                    : _save,
                icon: const Icon(LucideIcons.check),
                label: Text(strings.get(action)),
              ),
            ),
          ),
          actions: [
            TextButton.icon(
              onPressed: refreshing ? _cancelRefresh : _refresh,
              icon: Icon(refreshing ? LucideIcons.x : LucideIcons.refreshCw),
              label: Text(strings.get(refreshing ? 'cancel' : 'gate_refresh')),
            ),
          ],
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
                LayoutBuilder(
                  builder: (context, constraints) {
                    final width = constraints.maxWidth < 580
                        ? constraints.maxWidth
                        : (constraints.maxWidth - 20) / 2;
                    return Wrap(
                      spacing: 20,
                      runSpacing: 16,
                      children: [
                        SizedBox(
                          width: width,
                          child: _selection(
                            strings.get('gate_current'),
                            current,
                            strings.get('gate_no_connection'),
                          ),
                        ),
                        SizedBox(
                          width: width,
                          child: _selection(
                            strings.get('gate_draft'),
                            _draftServer,
                            strings.get(
                              _draft.hasSelection
                                  ? 'gate_saved'
                                  : 'gate_choose',
                            ),
                          ),
                        ),
                      ],
                    );
                  },
                ),
                if (_error != null)
                  WarningBanner(
                    title: strings.get('error'),
                    message: strings.get(_error!),
                    danger: true,
                  ),
                if (_error == null && snapshot.vpnGate.stage == 'error')
                  WarningBanner(
                    title: strings.get('error'),
                    message: strings.get('gate_proxy_blocked'),
                    danger: true,
                  ),
                ContentSection(
                  title: strings.get('gate_servers'),
                  subtitle: strings.get('gate_source_metrics'),
                  children: [
                    DropdownButtonFormField<String>(
                      key: ValueKey('vpn-gate-country-$_country'),
                      initialValue: _country,
                      isExpanded: true,
                      decoration: InputDecoration(
                        labelText: strings.get('gate_country'),
                      ),
                      items: [
                        DropdownMenuItem(
                          value: 'ALL',
                          child: Text(strings.get('gate_all_countries')),
                        ),
                        for (final country in _directory.countries)
                          DropdownMenuItem(
                            value: country.code ?? 'UNKNOWN',
                            child: Text(
                              '${country.code == null ? strings.get('gate_unknown_country') : country.name ?? country.code} (${country.count})',
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                        if (_country != 'ALL' &&
                            !_directory.countries.any(
                              (c) => (c.code ?? 'UNKNOWN') == _country,
                            ))
                          DropdownMenuItem(
                            value: _country,
                            child: Text(_country),
                          ),
                      ],
                      onChanged: _saving
                          ? null
                          : (country) {
                              if (country != null) {
                                setState(() {
                                  _country = country;
                                  _offset = 0;
                                });
                                unawaited(_load());
                              }
                            },
                    ),
                    if (_loading || refreshing)
                      const LinearProgressIndicator(minHeight: 2),
                    if (_directory.servers.isEmpty && !_loading)
                      Padding(
                        padding: const EdgeInsets.symmetric(vertical: 24),
                        child: Text(strings.get('gate_empty')),
                      ),
                    for (final server in _directory.servers)
                      _serverRow(server, strings),
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
                  ],
                ),
                ContentSection(
                  title: strings.get('gate_directory'),
                  children: [
                    Text(
                      '${strings.get('gate_received')}: ${_directory.fetchedAt?.toLocal().toString().split('.').first ?? '—'}',
                    ),
                    SelectableText(
                      '${strings.get('gate_source')}: ${_directory.sourceUrl ?? '—'}',
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
                        title: Text(strings.get('gate_fetch_error')),
                        children: [
                          for (final failure in _directory.failures)
                            Padding(
                              padding: const EdgeInsets.all(8),
                              child: SelectableText(failure),
                            ),
                        ],
                      ),
                  ],
                ),
              ],
            ),
          ),
        ),
      );
    },
  );
  Widget _selection(String title, VpnGateServer? server, String empty) =>
      Semantics(
        container: true,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(title, style: Theme.of(context).textTheme.labelLarge),
            const SizedBox(height: 8),
            Text(
              server == null
                  ? empty
                  : '${server.countryCode ?? '—'} · ${server.ip}',
              style: Theme.of(
                context,
              ).textTheme.titleMedium?.copyWith(fontFamily: UsqueFonts.mono),
            ),
          ],
        ),
      );
  Widget _serverRow(VpnGateServer server, AppStrings strings) {
    final selected = server.matches(_draft);
    return ListTile(
      key: ValueKey('vpn-gate-node-${server.id}'),
      selected: selected,
      leading: Icon(selected ? LucideIcons.circleCheck : LucideIcons.circle),
      title: Text(
        '${server.countryCode ?? '—'} · ${server.ip}',
        style: const TextStyle(fontFamily: UsqueFonts.mono),
      ),
      subtitle: Text(
        '${server.hostname}\n${strings.get('gate_score')}: ${server.score ?? '—'} · ${server.pingMs == null ? '—' : '${server.pingMs} ms'} · ${server.speedBps == null ? '—' : '${(server.speedBps! / 1000000).toStringAsFixed(1)} Mbps'}',
      ),
      isThreeLine: true,
      onTap: _saving
          ? null
          : () => setState(() {
              _draft = _draft.copyWith(server: server);
              _draftServer = server;
              _saveError = null;
            }),
    );
  }
}
