import 'dart:async';

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/usque_theme.dart';
import '../core/vpn_gate_presentation.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import 'common.dart';
import 'controller_selector.dart';
import 'vpn_gate_summary.dart';

typedef _EntrySelection = ({
  bool active,
  bool supported,
  String locale,
  VpnGateSettings settings,
  ConnectionPhase phase,
  VpnGateStatus status,
});

/// A persistent shortcut whose live status updates independently of the form.
class VpnGateEntry extends StatelessWidget {
  const VpnGateEntry({
    required this.controller,
    required this.onOpen,
    super.key,
    this.title = 'VPN Gate',
    this.entryKey = const ValueKey('proxy-vpn-gate-entry'),
  });

  final String title;
  final Key entryKey;

  final AppController controller;
  final VoidCallback onOpen;

  @override
  Widget build(BuildContext context) => ControllerSelector<_EntrySelection>(
    controller: controller,
    // Exclude throughput counters so statistics do not rebuild this shortcut.
    selector: (app) => (
      active: app.section == AppSection.proxy,
      supported: app.engineCapabilities?.vpnGateTcp ?? false,
      locale: app.strings.catalogId,
      settings: app.activeProfile.vpnGate,
      phase: app.snapshot.phase,
      status: app.snapshot.vpnGate,
    ),
    builder: (context, value) => _EntryCard(
      controller: controller,
      value: value,
      onOpen: onOpen,
      title: title,
      entryKey: entryKey,
    ),
  );
}

class _EntryCard extends StatefulWidget {
  const _EntryCard({
    required this.controller,
    required this.value,
    required this.onOpen,
    required this.title,
    required this.entryKey,
  });

  final AppController controller;
  final _EntrySelection value;
  final VoidCallback onOpen;
  final String title;
  final Key entryKey;

  @override
  State<_EntryCard> createState() => _EntryCardState();
}

class _EntryCardState extends State<_EntryCard> {
  VpnGateServer? _savedServer;
  int _loadGeneration = 0;

  @override
  void initState() {
    super.initState();
    _loadSavedServer();
  }

  @override
  void didUpdateWidget(covariant _EntryCard oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) _savedServer = null;
    if (oldWidget.controller != widget.controller ||
        oldWidget.value.settings != widget.value.settings ||
        oldWidget.value.active != widget.value.active ||
        oldWidget.value.supported != widget.value.supported ||
        oldWidget.value.status.server != widget.value.status.server &&
            (widget.value.status.server?.matches(widget.value.settings) ??
                false)) {
      _loadSavedServer();
    }
  }

  void _loadSavedServer() {
    final generation = ++_loadGeneration;
    final value = widget.value;
    if (!(_savedServer?.matches(value.settings) ?? false)) _savedServer = null;
    if (value.status.server?.matches(value.settings) ?? false) {
      _savedServer = value.status.server;
    }
    if (!value.active ||
        !value.supported ||
        !value.settings.hasSelection ||
        _savedServer != null) {
      return;
    }
    unawaited(_readSavedServer(generation, value.settings));
  }

  Future<void> _readSavedServer(
    int generation,
    VpnGateSettings settings,
  ) async {
    try {
      // This reads the local catalogue once, without refreshing or probing.
      // statusOnly omits saved_server, so request the smallest directory page.
      final directory = await widget.controller.listVpnGate(limit: 1);
      if (!mounted || generation != _loadGeneration) return;
      final server = directory.savedServer;
      if (server != null && server.matches(settings)) {
        setState(() => _savedServer = server);
      }
    } catch (_) {
      // A missing local catalogue must not hide the entry or imply a failure
      // of the live connection. The saved identifier remains available.
    }
  }

  @override
  Widget build(BuildContext context) {
    final strings = widget.controller.strings;
    final theme = Theme.of(context);
    final tokens = UsqueTokens.of(context);
    final value = widget.value;
    final settings = value.settings;
    final view = VpnGatePresentation(
      EngineSnapshot(phase: value.phase, vpnGate: value.status),
      configuredEnabled: settings.enabled,
    );
    final gateActive =
        view.connected ||
        view.failed ||
        view.busy && (settings.enabled || value.status.stage != 'disabled');
    final liveServer = gateActive ? view.server : null;
    final savedServer = (value.status.server?.matches(settings) ?? false)
        ? value.status.server
        : _savedServer;
    final server = liveServer ?? savedServer;
    final hasIdentity = server != null || settings.hasSelection;
    final identityLabel = liveServer != null
        ? view.serverLabelKey
        : 'gate_saved_server';
    final statusKey = view.statusKey == 'gate_no_connection'
        ? 'gate_enabled_idle'
        : view.statusKey;
    final tone = view.failed
        ? StatusTone.danger
        : view.connected
        ? StatusTone.success
        : gateActive && view.busy
        ? StatusTone.brand
        : StatusTone.neutral;
    final statusColor = statusToneColor(context, tone);

    return LayoutBuilder(
      builder: (context, constraints) {
        final compact =
            constraints.maxWidth < 600 ||
            MediaQuery.textScalerOf(context).scale(14) > 21;
        final action = Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Flexible(
              child: Text(
                strings.get('gate_manage'),
                style: theme.textTheme.labelLarge?.copyWith(
                  color: theme.colorScheme.primary,
                ),
              ),
            ),
            const SizedBox(width: 8),
            Icon(
              LucideIcons.chevronRight,
              size: 18,
              color: theme.colorScheme.primary,
            ),
          ],
        );
        return Panel(
          key: widget.entryKey,
          onTap: widget.onOpen,
          padding: EdgeInsets.all(compact ? 16 : 20),
          child: Row(
            crossAxisAlignment: compact
                ? CrossAxisAlignment.start
                : CrossAxisAlignment.center,
            children: [
              ExcludeSemantics(
                child: Container(
                  width: compact ? 36 : 44,
                  height: compact ? 36 : 44,
                  decoration: BoxDecoration(
                    color: theme.colorScheme.primary.withValues(
                      alpha: tokens.tint,
                    ),
                    borderRadius: BorderRadius.circular(UsqueRadii.control),
                  ),
                  child: Icon(
                    LucideIcons.globe,
                    size: compact ? 22 : 24,
                    color: theme.colorScheme.primary,
                  ),
                ),
              ),
              SizedBox(width: compact ? 12 : 16),
              Expanded(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Wrap(
                      spacing: 12,
                      runSpacing: 8,
                      crossAxisAlignment: WrapCrossAlignment.center,
                      children: [
                        Text(widget.title, style: theme.textTheme.titleLarge),
                        if (widget.title != 'VPN Gate') const Text('VPN Gate'),
                        Semantics(
                          liveRegion: true,
                          child: Container(
                            padding: const EdgeInsets.symmetric(
                              horizontal: 8,
                              vertical: 4,
                            ),
                            decoration: BoxDecoration(
                              color: statusColor.withValues(alpha: tokens.tint),
                              borderRadius: BorderRadius.circular(
                                UsqueRadii.chip,
                              ),
                            ),
                            child: InlineStatus(
                              label: strings.get(statusKey),
                              tone: tone,
                            ),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 8),
                    if (hasIdentity) ...[
                      Text(
                        strings.get(identityLabel),
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: theme.colorScheme.onSurfaceVariant,
                        ),
                      ),
                      const SizedBox(height: 4),
                      if (server != null)
                        VpnGateNodeIdentity(server: server)
                      else
                        Text(
                          settings.serverId,
                          style: theme.textTheme.bodyMedium?.copyWith(
                            fontFamily: UsqueFonts.mono,
                          ),
                        ),
                    ] else
                      Text(
                        strings.get('gate_subtitle'),
                        style: theme.textTheme.bodyMedium?.copyWith(
                          color: theme.colorScheme.onSurfaceVariant,
                        ),
                      ),
                    if (view.connected) ...[
                      const SizedBox(height: 6),
                      Text(
                        'WARP → VPN Gate',
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: theme.colorScheme.onSurfaceVariant,
                        ),
                      ),
                    ],
                    if (compact) ...[const SizedBox(height: 12), action],
                  ],
                ),
              ),
              if (!compact) ...[
                const SizedBox(width: 20),
                ConstrainedBox(
                  constraints: BoxConstraints(
                    maxWidth: constraints.maxWidth * .25,
                  ),
                  child: action,
                ),
              ],
            ],
          ),
        );
      },
    );
  }
}
