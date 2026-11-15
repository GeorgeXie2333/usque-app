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
  bool _busy = false, _failed = false, _restart = false;
  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh({bool actionFailed = false}) async {
    try {
      final state = await widget.controller.getMacosServiceState();
      if (mounted) {
        setState(() {
          _state = state;
          _failed = actionFailed;
        });
      }
    } on Object {
      if (mounted) setState(() => _failed = true);
    }
  }

  Future<void> _maintain(MacosServiceAction action) async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      await widget.controller.maintainMacosService(action);
      if (mounted) {
        setState(() => _restart = action != MacosServiceAction.recover);
      }
      await _refresh();
    } on Object {
      await _refresh(actionFailed: true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final strings = widget.controller.strings;
    final state = _state;
    final connected =
        widget.controller.snapshot.isConnected ||
        widget.controller.snapshot.isTransitional;
    final enabled = !_busy && !connected && state?.canMaintain == true;
    return ContentSection(
      icon: LucideIcons.shieldCheck,
      title: strings.get('macos_service_title'),
      subtitle: strings.get('macos_service_admin_hint'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (_busy || state == null && !_failed)
            const LinearProgressIndicator(),
          if (state != null) ...[
            Text(
              strings.get(
                !state.installed
                    ? 'macos_service_missing'
                    : !state.compatible
                    ? 'macos_service_incompatible'
                    : 'macos_service_installed',
              ),
            ),
            if (state.version.isNotEmpty)
              Text('${strings.get('macos_service_version')}: ${state.version}'),
            if (state.protectionRetained)
              Text(strings.get('macos_service_protected')),
            if (state.recoveryPending)
              Text(strings.get('macos_service_recovery')),
            if (state.tunAvailable)
              Text(strings.get('macos_service_ready'))
            else if (!state.releaseApproved && !state.testTunBuild)
              Text(strings.get('macos_service_gated'))
            else if (state.installed &&
                state.compatible &&
                !state.recoveryPending &&
                !state.preflightPassed)
              Text(strings.get('macos_service_preflight')),
          ],
          if (_failed) Text(strings.get('macos_service_failed')),
          if (_restart) Text(strings.get('macos_service_restart')),
          if (connected) Text(strings.get('macos_service_disconnect')),
          const SizedBox(height: 12),
          Wrap(
            spacing: 12,
            runSpacing: 8,
            children: [
              if (state != null)
                OutlinedButton.icon(
                  key: const ValueKey('macos-service-install'),
                  onPressed: enabled
                      ? () => _maintain(
                          state.installed
                              ? MacosServiceAction.repair
                              : MacosServiceAction.install,
                        )
                      : null,
                  icon: const Icon(LucideIcons.shieldCheck),
                  label: Text(
                    strings.get(
                      state.installed
                          ? 'macos_service_repair'
                          : 'macos_service_install',
                    ),
                  ),
                ),
              if (state?.installed == true) ...[
                OutlinedButton(
                  key: const ValueKey('macos-service-recover'),
                  onPressed: enabled
                      ? () => _maintain(MacosServiceAction.recover)
                      : null,
                  child: Text(strings.get('macos_service_restore')),
                ),
                TextButton(
                  key: const ValueKey('macos-service-uninstall'),
                  onPressed: enabled
                      ? () => _maintain(MacosServiceAction.uninstall)
                      : null,
                  child: Text(strings.get('macos_service_uninstall')),
                ),
              ],
              TextButton(
                onPressed: _busy ? null : _refresh,
                child: Text(strings.get('retry')),
              ),
            ],
          ),
        ],
      ),
    );
  }
}
