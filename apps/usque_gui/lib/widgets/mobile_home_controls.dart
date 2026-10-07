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

/// Mobile quick controls for VPN/TUN and chain proxy outputs.
///
/// Aligned with desktop HomeDesktopControls semantics: switches express
/// saved preferences and patch only their own field in the confirmed profile.
class MobileHomeControls extends StatefulWidget {
  const MobileHomeControls({
    required this.controller,
    required this.onOpenChainProxy,
    super.key,
  });

  final AppController controller;
  final VoidCallback onOpenChainProxy;

  @override
  State<MobileHomeControls> createState() => _MobileHomeControlsState();
}

typedef _ControlsView = ({
  String catalogId,
  bool locked,
  bool tunnel,
  bool chainEnabled,
  String? failure,
  bool unconfirmed,
  bool reconnect,
});

class _MobileHomeControlsState extends State<MobileHomeControls> {
  bool _saving = false;
  String? _saveFailure;
  NetworkSettingsState? _fallbackSettingsState;

  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_clearConfirmedFallback);
  }

  @override
  void didUpdateWidget(covariant MobileHomeControls oldWidget) {
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
      return ContentSection(
        padding: const EdgeInsets.symmetric(vertical: 12, horizontal: 16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              strings.get('home_mobile_controls'),
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: 12),
            _MobileOutputRow(
              title: strings.tunnelOutputLabel(defaultTargetPlatform),
              icon: LucideIcons.ethernetPort,
              value: view.tunnel,
              onChanged: enabled ? _tunnel : null,
              onTap: enabled
                  ? () => _tunnel(!view.tunnel)
                  : null,
            ),
            const SizedBox(height: 8),
            _MobileOutputRow(
              title: strings.chain('title'),
              icon: LucideIcons.link,
              value: view.chainEnabled,
              onChanged: enabled ? _chain : null,
              onTap: enabled
                  ? () => _chain(!view.chainEnabled)
                  : null,
              onOpenSettings: _saving ? null : widget.onOpenChainProxy,
            ),
            if (failure != null) ...[
              const SizedBox(height: 12),
              Semantics(
                liveRegion: true,
                child: Text(
                  failure,
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: Theme.of(context).colorScheme.error,
                  ),
                ),
              ),
              if (view.unconfirmed)
                Align(
                  alignment: AlignmentDirectional.centerStart,
                  child: TextButton(
                    onPressed: _saving ? null : () => unawaited(_refresh()),
                    child: Text(strings.get('retry')),
                  ),
                )
              else if (view.reconnect)
                Align(
                  alignment: AlignmentDirectional.centerStart,
                  child: TextButton(
                    onPressed: enabled ? widget.controller.retry : null,
                    child: Text(strings.get('settings_reconnect')),
                  ),
                ),
            ],
          ],
        ),
      );
    },
  );
}

class _MobileOutputRow extends StatelessWidget {
  const _MobileOutputRow({
    required this.title,
    required this.icon,
    required this.value,
    required this.onChanged,
    this.onTap,
    this.onOpenSettings,
  });

  final String title;
  final IconData icon;
  final bool value;
  final ValueChanged<bool>? onChanged;
  final VoidCallback? onTap;
  final VoidCallback? onOpenSettings;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Material(
      color: Colors.transparent,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(8),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 4),
          child: Row(
            children: [
              Icon(
                icon,
                size: 20,
                color: theme.colorScheme.onSurfaceVariant,
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Text(
                  title,
                  style: theme.textTheme.bodyMedium,
                ),
              ),
              if (onOpenSettings != null) ...[
                IconButton(
                  icon: const Icon(LucideIcons.settings, size: 18),
                  onPressed: onOpenSettings,
                  visualDensity: VisualDensity.compact,
                  tooltip: theme.localizations.moreButtonTooltip,
                ),
                const SizedBox(width: 4),
              ],
              Semantics(
                label: title,
                child: Switch(
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
