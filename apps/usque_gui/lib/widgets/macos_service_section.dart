import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import '../services/engine_client.dart';
import '../state/app_controller.dart';
import 'common.dart';

class MacosServiceSection extends StatefulWidget {
  const MacosServiceSection({required this.controller, super.key});
  final AppController controller;
  @override
  State<MacosServiceSection> createState() => _MacosServiceSectionState();
}

class _MacosServiceSectionState extends State<MacosServiceSection> {
  MacosServiceState? _state;
  bool _reading = true;
  bool _working = false;
  String? _notice;
  bool _restartNeeded = false;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh({String? notice}) async {
    setState(() => _reading = true);
    try {
      final state = await widget.controller.getMacosServiceState();
      if (!mounted) return;
      setState(() {
        _state = state;
        _notice = notice;
        _reading = false;
      });
    } on Object {
      if (!mounted) return;
      setState(() {
        _state = null;
        _notice = notice ?? 'macos_service_status_failed';
        _reading = false;
      });
    }
  }

  Future<void> _maintain(MacosServiceAction action) async {
    if (_working || _reading || widget.controller.macosServiceBusy) return;
    final controller = widget.controller;
    setState(() {
      _working = true;
      _notice = null;
    });
    String? notice;
    try {
      await controller.maintainMacosService(action);
      // Launch Services now owns the handoff. Leave the old window busy until
      // it exits instead of probing the service or accepting another install.
      if (action == MacosServiceAction.install) return;
      if (action == MacosServiceAction.repair ||
          action == MacosServiceAction.uninstall) {
        _restartNeeded = true;
      }
    } on Object catch (error) {
      notice =
          error is EngineException &&
              error.code == 'MACOS_SERVICE_RELAUNCH_FAILED'
          ? 'macos_service_relaunch_failed'
          : 'macos_service_failed';
    }
    if (!mounted) return;
    await _refresh(notice: notice);
    if (mounted) setState(() => _working = false);
  }

  Widget _button(
    MacosServiceAction action, {
    required bool enabled,
    required bool prominent,
  }) {
    final (key, label, icon) = switch (action) {
      MacosServiceAction.install => (
        'install',
        'macos_service_install',
        LucideIcons.shieldCheck,
      ),
      MacosServiceAction.repair => (
        'install',
        'macos_service_repair',
        LucideIcons.wrench,
      ),
      MacosServiceAction.recover => (
        'recover',
        'macos_service_restore',
        LucideIcons.rotateCcw,
      ),
      MacosServiceAction.uninstall => throw StateError(
        'Uninstall is a menu action',
      ),
    };
    final buttonKey = ValueKey('macos-service-$key');
    final onPressed = enabled ? () => _maintain(action) : null;
    final text = Text(widget.controller.strings.get(label));
    return prominent
        ? FilledButton.icon(
            key: buttonKey,
            onPressed: onPressed,
            icon: Icon(icon),
            label: text,
          )
        : OutlinedButton.icon(
            key: buttonKey,
            onPressed: onPressed,
            icon: Icon(icon),
            label: text,
          );
  }

  @override
  Widget build(BuildContext context) {
    final controller = widget.controller;
    final strings = controller.strings;
    final theme = Theme.of(context);
    final state = _state;
    final installed = state?.installed == true;
    final incompatible = installed && state?.compatible == false;
    final recovery = installed && state?.recoveryPending == true;
    final connected =
        controller.snapshot.isConnected || controller.snapshot.isTransitional;
    final busy = _working || controller.macosServiceBusy || _reading;
    final enabled = !busy && !connected && state?.canMaintain == true;
    final primary = !installed
        ? MacosServiceAction.install
        : recovery && !incompatible
        ? MacosServiceAction.recover
        : MacosServiceAction.repair;

    String? networkStatus;
    if (recovery || state?.protectionRetained == true) {
      networkStatus = [
        if (recovery) strings.get('macos_service_recovery'),
        if (state?.protectionRetained == true)
          strings.get('macos_service_protected'),
      ].join(' ');
    } else if (installed && !incompatible) {
      if (state!.tunAvailable) {
        networkStatus = strings.get('macos_service_ready');
      } else if (!state.releaseApproved && !state.testTunBuild) {
        networkStatus = strings.get('macos_service_gated');
      } else if (!state.preflightPassed) {
        networkStatus = strings.get('macos_service_preflight');
      }
    }

    return Padding(
      padding: RowInsets.row,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(
            children: [
              Expanded(
                child: ContentHeading(
                  icon: LucideIcons.shieldCheck,
                  title: strings.get('macos_service_title'),
                ),
              ),
              IconButton(
                visualDensity: VisualDensity.compact,
                tooltip: strings.get('macos_service_refresh'),
                onPressed: busy ? null : _refresh,
                icon: const Icon(LucideIcons.refreshCw, size: 18),
              ),
            ],
          ),
          const SizedBox(height: 4),
          Padding(
            padding: const EdgeInsetsDirectional.only(
              start: RowInsets.textIndent,
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                if (state != null) ...[
                  Wrap(
                    spacing: 12,
                    runSpacing: 4,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: [
                      InlineStatus(
                        label: strings.get(
                          !installed
                              ? 'macos_service_missing'
                              : incompatible
                              ? 'macos_service_incompatible'
                              : 'macos_service_installed',
                        ),
                        tone: incompatible || recovery
                            ? StatusTone.warning
                            : installed
                            ? StatusTone.success
                            : StatusTone.neutral,
                      ),
                      if (state.version.isNotEmpty)
                        Text(
                          '${strings.get('macos_service_version')} ${state.version}',
                          style: theme.textTheme.bodySmall?.copyWith(
                            color: theme.colorScheme.onSurfaceVariant,
                          ),
                        ),
                    ],
                  ),
                  if (networkStatus != null) ...[
                    const SizedBox(height: 6),
                    Text(networkStatus, style: theme.textTheme.bodyMedium),
                  ],
                  const SizedBox(height: 6),
                  Text(
                    strings.get(
                      !state.canMaintain
                          ? 'macos_service_helper_missing'
                          : installed
                          ? 'macos_service_admin_hint'
                          : 'macos_service_install_hint',
                    ),
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: theme.colorScheme.onSurfaceVariant,
                    ),
                  ),
                ],
                if (busy) ...[
                  const SizedBox(height: 8),
                  const LinearProgressIndicator(),
                ],
                for (final message in [
                  ?_notice,
                  if (_restartNeeded) 'macos_service_restart',
                  if (connected) 'macos_service_disconnect',
                ]) ...[
                  const SizedBox(height: 8),
                  InlineStatus(
                    label: strings.get(message),
                    tone: StatusTone.warning,
                  ),
                ],
                if (state != null) ...[
                  const SizedBox(height: 12),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: [
                      _button(
                        primary,
                        enabled: enabled,
                        prominent: !installed || incompatible || recovery,
                      ),
                      if (recovery)
                        _button(
                          incompatible
                              ? MacosServiceAction.recover
                              : MacosServiceAction.repair,
                          enabled: enabled,
                          prominent: false,
                        ),
                      if (installed)
                        PopupMenuButton<MacosServiceAction>(
                          key: const ValueKey('macos-service-more'),
                          tooltip: strings.get('macos_service_more'),
                          enabled: enabled,
                          icon: const Icon(LucideIcons.ellipsis),
                          onSelected: _maintain,
                          itemBuilder: (_) => [
                            if (!recovery)
                              PopupMenuItem(
                                value: MacosServiceAction.recover,
                                child: Text(
                                  strings.get('macos_service_restore'),
                                ),
                              ),
                            PopupMenuItem(
                              key: const ValueKey('macos-service-uninstall'),
                              value: MacosServiceAction.uninstall,
                              child: Text(
                                strings.get('macos_service_uninstall'),
                                style: TextStyle(
                                  color: theme.colorScheme.error,
                                ),
                              ),
                            ),
                          ],
                        ),
                    ],
                  ),
                ],
              ],
            ),
          ),
        ],
      ),
    );
  }
}
