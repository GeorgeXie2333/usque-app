import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/chain_strings.dart';
import '../models/app_models.dart';
import '../models/network_settings.dart';
import '../state/app_controller.dart';
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
    super.key,
  });

  final AppController controller;
  final VoidCallback onOpenChainProxy;

  @override
  State<HomeDesktopControls> createState() => _HomeDesktopControlsState();
}

typedef _ControlsView = ({
  String catalogId,
  bool locked,
  bool tunnel,
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

  bool _locked(AppController app) =>
      !app.initialized ||
      app.busy ||
      app.snapshot.isTransitional ||
      app.snapshot.errorCode == 'WINDOWS_RECOVERY_BLOCKED' ||
      !app.networkSettings.supported ||
      app.networkSettings.unconfirmed ||
      app.networkSettings.state?.status ==
          NetworkSettingsApplyStatus.applying ||
      app.networkSettings.state?.operationId != null &&
          app.networkSettings.state?.status ==
              NetworkSettingsApplyStatus.unknown;

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
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          ContentList(
            children: [
              _OutputControl(
                title: strings.tunnelOutputLabel(Theme.of(context).platform),
                icon: LucideIcons.ethernetPort,
                option: Text(strings.get('home_tun_hint')),
                switchKey: const ValueKey('home-tun-switch'),
                value: view.tunnel,
                onChanged: enabled ? _tunnel : null,
              ),
              if (defaultTargetPlatform == TargetPlatform.windows)
                _OutputControl(
                  title: strings.get('home_system_proxy'),
                  icon: LucideIcons.monitorCog,
                  option: Text(
                    strings.get(
                      !view.http && !view.systemProxy
                          ? 'home_system_proxy_requires_http'
                          : 'home_system_proxy_hint',
                    ),
                  ),
                  switchKey: const ValueKey('home-system-proxy-switch'),
                  value: view.systemProxy,
                  onChanged: enabled && (view.http || view.systemProxy)
                      ? _systemProxy
                      : null,
                ),
              _OutputControl(
                title: strings.chain('title'),
                icon: LucideIcons.link,
                bottomSpacing: 2,
                onOpen: _saving ? null : widget.onOpenChainProxy,
                option: TextButton.icon(
                  key: const ValueKey('home-chain-proxy-settings'),
                  onPressed: _saving ? null : widget.onOpenChainProxy,
                  style: TextButton.styleFrom(
                    padding: EdgeInsets.zero,
                    alignment: AlignmentDirectional.centerStart,
                  ),
                  iconAlignment: IconAlignment.end,
                  icon: const Icon(LucideIcons.chevronRight, size: 16),
                  label: Text(strings.get('settings')),
                ),
                switchKey: const ValueKey('home-chain-proxy-switch'),
                value: view.chainEnabled,
                onChanged: enabled ? _chain : null,
              ),
            ],
          ),
          if (failure != null) ...[
            const SizedBox(height: 12),
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
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: TextButton(
                  key: const ValueKey('home-controls-refresh'),
                  onPressed: _saving ? null : () => unawaited(_refresh()),
                  child: Text(strings.get('retry')),
                ),
              )
            else if (view.reconnect)
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: TextButton(
                  key: const ValueKey('home-controls-reconnect'),
                  onPressed: enabled ? widget.controller.retry : null,
                  child: Text(strings.get('settings_reconnect')),
                ),
              ),
          ],
        ],
      );
    },
  );
}

class _OutputControl extends StatelessWidget {
  const _OutputControl({
    required this.title,
    required this.icon,
    required this.option,
    required this.switchKey,
    required this.value,
    required this.onChanged,
    this.onOpen,
    this.bottomSpacing = 1,
  });

  final String title;
  final IconData icon;
  final Widget option;
  final Key switchKey;
  final bool value;
  final ValueChanged<bool>? onChanged;
  final VoidCallback? onOpen;
  final double bottomSpacing;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final heading = Row(
      children: [
        ExcludeSemantics(
          child: Icon(
            icon,
            size: 24,
            color: theme.colorScheme.onSurfaceVariant,
          ),
        ),
        const SizedBox(width: 12),
        Expanded(child: Text(title, style: theme.textTheme.titleMedium)),
      ],
    );
    final toggle = Semantics(
      label: title,
      child: SizedBox(
        width: 80,
        height: 60,
        child: Center(
          child: Transform.scale(
            scale: 1.2,
            child: Switch(
              key: switchKey,
              value: value,
              onChanged: onChanged,
              materialTapTargetSize: MaterialTapTargetSize.padded,
            ),
          ),
        ),
      ),
    );
    return Padding(
      padding: EdgeInsets.only(bottom: bottomSpacing),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          if (onOpen != null)
            TextButton(
              key: const ValueKey('home-chain-proxy-heading'),
              onPressed: onOpen,
              style: TextButton.styleFrom(
                padding: EdgeInsets.zero,
                minimumSize: const Size(0, 48),
                alignment: AlignmentDirectional.centerStart,
                foregroundColor: theme.colorScheme.onSurface,
              ),
              child: heading,
            )
          else
            ConstrainedBox(
              constraints: const BoxConstraints(minHeight: 48),
              child: Align(
                alignment: AlignmentDirectional.centerStart,
                child: heading,
              ),
            ),
          Row(
            children: [
              Expanded(
                child: DefaultTextStyle.merge(
                  style: theme.textTheme.bodyMedium?.copyWith(
                    color: theme.colorScheme.onSurfaceVariant,
                  ),
                  child: option,
                ),
              ),
              const SizedBox(width: 12),
              toggle,
            ],
          ),
        ],
      ),
    );
  }
}
