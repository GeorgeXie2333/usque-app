import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/chain_strings.dart';
import '../models/app_models.dart';
import '../models/network_settings.dart';
import '../state/app_controller.dart';
import 'chain_exit_summary.dart';
import 'chain_source_picker.dart';
import 'common.dart';
import 'controller_selector.dart';

/// Desktop shortcuts to the confirmed network configuration.
///
/// These switches express the saved preference, never an inferred runtime
/// state. Each save starts from the latest confirmed profile and patches only
/// its own field, so drafts held by another settings page remain untouched.
class HomeDesktopControls extends StatefulWidget {
  const HomeDesktopControls({
    required this.controller,
    required this.onOpenChainProxy,
    this.minHeight,
    super.key,
  });

  final AppController controller;
  final VoidCallback onOpenChainProxy;

  /// Height the rows share beside the connection ring; null when stacked.
  final double? minHeight;

  @override
  State<HomeDesktopControls> createState() => _HomeDesktopControlsState();
}

typedef _ControlsView = ({
  String catalogId,
  bool locked,
  bool tunnel,
  bool tunnelSupported,
  bool systemProxySupported,
  bool http,
  bool systemProxy,
  bool chainEnabled,
  String? failure,
  bool unconfirmed,
  bool reconnect,
});

class _HomeDesktopControlsState extends State<HomeDesktopControls> {
  bool _saving = false;
  String? _saveFailure;
  NetworkSettingsState? _fallbackSettingsState;

  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_clearConfirmedFallback);
  }

  @override
  void didUpdateWidget(covariant HomeDesktopControls oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.removeListener(_clearConfirmedFallback);
      widget.controller.addListener(_clearConfirmedFallback);
      _saveFailure = null;
      _fallbackSettingsState = null;
    }
  }

  @override
  void dispose() {
    widget.controller.removeListener(_clearConfirmedFallback);
    super.dispose();
  }

  void _clearConfirmedFallback() {
    if (_saveFailure == null || !mounted) return;
    final settings = widget.controller.networkSettings.state;
    final newOperation =
        settings?.operationId != _fallbackSettingsState?.operationId ||
        settings?.sourceEpoch != _fallbackSettingsState?.sourceEpoch;
    if (settings?.persisted == true &&
        settings?.operationId != null &&
        newOperation) {
      setState(() {
        _saveFailure = null;
        _fallbackSettingsState = null;
      });
    }
  }

  bool _locked(AppController app) => app.networkShortcutsLocked;

  _ControlsView _view(AppController app) {
    final profile = app.activeProfile;
    final settings = app.networkSettings;
    final state = settings.state;
    final failed = settings.state?.status == NetworkSettingsApplyStatus.failed;
    final unknown =
        settings.unconfirmed ||
        settings.state?.operationId != null &&
            settings.state?.status == NetworkSettingsApplyStatus.unknown;
    final deferredFailure =
        app.snapshot.isConnected &&
        state?.operationId != null &&
        state?.persisted == true &&
        state?.status == NetworkSettingsApplyStatus.deferred &&
        state?.errorCode != null;
    return (
      catalogId: app.strings.catalogId,
      locked: _locked(app),
      tunnel: profile.frontends.tunnel,
      tunnelSupported: app.tunnelOutputSupported,
      systemProxySupported: app.systemProxyOutputSupported,
      http: profile.frontends.http,
      systemProxy: profile.proxy.systemProxy,
      chainEnabled: profile.chainEnabled,
      failure:
          failed || unknown || deferredFailure || settings.saveError != null
          ? app.networkSettingsMessage
          : null,
      unconfirmed: unknown,
      reconnect: app.networkSettingsCanReconnect,
    );
  }

  Future<void> _save(UsqueProfile profile, List<String> fields) async {
    final app = widget.controller;
    if (_saving || _locked(app)) return;
    final previousError = app.lastError;
    setState(() {
      _saving = true;
      _saveFailure = null;
      _fallbackSettingsState = null;
    });
    try {
      final confirmed = await app.saveNetwork(profile, changedFields: fields);
      if (!mounted) return;
      if (!confirmed) {
        setState(() {
          // Authoritative failures already rebuild through the selector. Do
          // not retain a duplicate that could outlive a later settings save.
          _saveFailure = _view(app).failure != null
              ? null
              : (app.lastError != previousError ? app.lastError : null) ??
                    app.strings.get('settings_save_failed');
          _fallbackSettingsState = app.networkSettings.state;
        });
      }
    } on Object {
      if (mounted) {
        setState(() {
          _saveFailure = _view(app).failure != null
              ? null
              : app.strings.get('settings_unknown');
          _fallbackSettingsState = app.networkSettings.state;
        });
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  void _tunnel(bool enabled) {
    final profile = widget.controller.activeProfile;
    if (profile.frontends.tunnel == enabled) return;
    unawaited(
      _save(
        profile.copyWith(
          frontends: profile.frontends.copyWith(tunnel: enabled),
        ),
        const ['frontends.tunnel'],
      ),
    );
  }

  void _systemProxy(bool enabled) {
    final profile = widget.controller.activeProfile;
    if (profile.proxy.systemProxy == enabled ||
        enabled && !profile.frontends.http) {
      return;
    }
    unawaited(
      _save(
        profile.copyWith(proxy: profile.proxy.copyWith(systemProxy: enabled)),
        const ['proxy.system_proxy'],
      ),
    );
  }

  void _chain(bool enabled) {
    final app = widget.controller;
    if (_saving || _locked(app)) return;
    final profile = app.activeProfile;
    if (profile.chainEnabled == enabled) return;
    final source = profile.chainSource;
    final exit = profile.chainExit;
    final selected = source == ChainSource.vpnGate
        ? profile.vpnGate.hasSelection
        : exit?.profileId?.isNotEmpty == true &&
              exit?.revision?.isNotEmpty == true;
    if (enabled &&
        (!selected ||
            !ChainSourcePicker.available(app.engineCapabilities, source))) {
      widget.onOpenChainProxy();
      return;
    }
    if (source == ChainSource.vpnGate) {
      // Keep both the legacy selection and a modern VPN Gate chain in sync.
      // No new node is selected or prepared by this shortcut.
      unawaited(
        _save(
          profile.copyWith(
            vpnGate: profile.vpnGate.copyWith(enabled: enabled),
            chainExit: exit?.copyWith(enabled: enabled),
          ),
          ['vpn_gate', if (exit != null) 'chain_exit'],
        ),
      );
    } else if (exit != null) {
      unawaited(
        _save(
          profile.copyWith(chainExit: exit.copyWith(enabled: enabled)),
          const ['chain_exit'],
        ),
      );
    }
  }

  Future<void> _refresh() async {
    if (_saving) return;
    setState(() {
      _saving = true;
      _saveFailure = null;
    });
    try {
      await widget.controller.networkSettings.refresh();
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) => ControllerSelector<_ControlsView>(
    controller: widget.controller,
    selector: _view,
    builder: (context, view) {
      final strings = widget.controller.strings;
      final enabled = !_saving && !view.locked;
      final failure = view.failure ?? _saveFailure;
      final desktop =
          defaultTargetPlatform == TargetPlatform.windows ||
          defaultTargetPlatform == TargetPlatform.linux;
      // Beside the ring, the rows share its height so the first and last
      // rows meet the ring's top and bottom edges.
      final rows = desktop ? 3 : 2;
      final rowMinHeight = widget.minHeight == null
          ? 0.0
          : (widget.minHeight! - (rows - 1)) / rows;
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          ContentList(
            children: [
              _OutputControl(
                title: strings.tunnelOutputLabel(Theme.of(context).platform),
                icon: LucideIcons.ethernetPort,
                details: Text(
                  strings.get(
                    view.tunnelSupported
                        ? 'home_tun_hint'
                        : 'linux_output_unavailable',
                  ),
                ),
                switchKey: const ValueKey('home-tun-switch'),
                value: view.tunnel,
                onChanged: enabled && (view.tunnelSupported || view.tunnel)
                    ? _tunnel
                    : null,
                minHeight: rowMinHeight,
              ),
              if (desktop)
                _OutputControl(
                  title: strings.get('home_system_proxy'),
                  icon: LucideIcons.monitorCog,
                  details: Text(
                    strings.get(
                      !view.systemProxySupported
                          ? 'linux_output_unavailable'
                          : !view.http && !view.systemProxy
                          ? 'home_system_proxy_requires_http'
                          : 'home_system_proxy_hint',
                    ),
                  ),
                  switchKey: const ValueKey('home-system-proxy-switch'),
                  value: view.systemProxy,
                  onChanged:
                      enabled &&
                          (view.systemProxySupported || view.systemProxy) &&
                          (view.http || view.systemProxy)
                      ? _systemProxy
                      : null,
                  minHeight: rowMinHeight,
                ),
              _OutputControl(
                title: strings.chain('title'),
                icon: LucideIcons.link,
                openKey: const ValueKey('home-chain-proxy-settings'),
                onOpen: _saving ? null : widget.onOpenChainProxy,
                details: _ChainDetails(controller: widget.controller),
                switchKey: const ValueKey('home-chain-proxy-switch'),
                value: view.chainEnabled,
                onChanged: enabled ? _chain : null,
                minHeight: rowMinHeight,
              ),
            ],
          ),
          if (failure != null)
            Padding(
              padding: const EdgeInsetsDirectional.only(
                start: _OutputControl.textIndent,
                top: 12,
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Semantics(
                    liveRegion: true,
                    child: Text(
                      failure,
                      key: const ValueKey('home-controls-error'),
                      style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                        color: Theme.of(context).colorScheme.error,
                      ),
                    ),
                  ),
                  if (view.unconfirmed)
                    TextButton(
                      key: const ValueKey('home-controls-refresh'),
                      onPressed: _saving ? null : () => unawaited(_refresh()),
                      style: TextButton.styleFrom(padding: EdgeInsets.zero),
                      child: Text(strings.get('retry')),
                    )
                  else if (view.reconnect)
                    TextButton(
                      key: const ValueKey('home-controls-reconnect'),
                      onPressed: enabled ? widget.controller.retry : null,
                      style: TextButton.styleFrom(padding: EdgeInsets.zero),
                      child: Text(strings.get('settings_reconnect')),
                    ),
                ],
              ),
            ),
        ],
      );
    },
  );
}

/// The chain proxy's source and choice under its title, with its status when
/// a session can report one. A switched-off chain lets the switch speak.
class _ChainDetails extends StatelessWidget {
  const _ChainDetails({required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) => ChainExitSummaryBuilder(
    controller: controller,
    section: AppSection.home,
    builder: (context, view) {
      final strings = controller.strings;
      final theme = Theme.of(context);
      if (!view.enabled && !view.live && view.selection == null) {
        return Text(strings.chain('subtitle'));
      }
      final label = view.enabled || view.live
          ? view.statusLabel(strings)
          : null;
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          ChainSelectionText(view, key: const ValueKey('home-chain-selection')),
          if (label != null) ...[
            const SizedBox(height: 2),
            Semantics(
              liveRegion: true,
              child: Text(
                label,
                key: const ValueKey('home-chain-status'),
                style: theme.textTheme.bodySmall?.copyWith(
                  color: statusToneColor(context, view.tone),
                ),
              ),
            ),
          ],
        ],
      );
    },
  );
}

/// One output row: icon, title and a muted line in one text column, with
/// the switch centred on the whole row, as in the settings list tiles.
class _OutputControl extends StatelessWidget {
  const _OutputControl({
    required this.title,
    required this.icon,
    required this.details,
    required this.switchKey,
    required this.value,
    required this.onChanged,
    this.onOpen,
    this.openKey,
    this.minHeight = 0,
  });

  /// Icon (24) plus its gap (12): where titles and their details start.
  static const double textIndent = 36;

  final String title;
  final IconData icon;
  final Widget details;
  final Key switchKey;
  final bool value;
  final ValueChanged<bool>? onChanged;
  final VoidCallback? onOpen;
  final Key? openKey;
  final double minHeight;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final muted = theme.colorScheme.onSurfaceVariant;
    final label = Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        ExcludeSemantics(child: Icon(icon, size: 24, color: muted)),
        const SizedBox(width: 12),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Row(
                children: [
                  Flexible(
                    child: Text(
                      title,
                      style: theme.textTheme.titleMedium?.copyWith(
                        color: theme.colorScheme.onSurface,
                      ),
                    ),
                  ),
                  if (onOpen != null) ...[
                    const SizedBox(width: 4),
                    Icon(
                      LucideIcons.chevronRightDir,
                      size: 16,
                      color: theme.colorScheme.onSurface,
                    ),
                  ],
                ],
              ),
              const SizedBox(height: 4),
              DefaultTextStyle.merge(
                style: theme.textTheme.bodyMedium?.copyWith(color: muted),
                child: details,
              ),
            ],
          ),
        ),
      ],
    );
    return ConstrainedBox(
      constraints: BoxConstraints(minHeight: minHeight),
      child: Align(
        alignment: AlignmentDirectional.centerStart,
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 12),
          child: Row(
            children: [
              Expanded(
                child: onOpen == null
                    ? label
                    : TextButton(
                        key: openKey,
                        onPressed: onOpen,
                        style: TextButton.styleFrom(
                          padding: EdgeInsets.zero,
                          alignment: AlignmentDirectional.centerStart,
                          foregroundColor: theme.colorScheme.onSurface,
                        ),
                        child: label,
                      ),
              ),
              const SizedBox(width: 12),
              Semantics(
                label: title,
                child: Switch(
                  key: switchKey,
                  value: value,
                  onChanged: onChanged,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
