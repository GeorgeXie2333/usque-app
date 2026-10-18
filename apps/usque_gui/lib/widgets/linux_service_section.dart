import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/user_facing_errors.dart';
import '../services/engine_client.dart';
import '../state/app_controller.dart';
import 'common.dart';

/// Explicit maintenance only. Merely opening this section never installs,
/// repairs, starts a service, or connects a VPN.
class LinuxServiceSection extends StatefulWidget {
  const LinuxServiceSection({required this.controller, super.key});

  final AppController controller;

  @override
  State<LinuxServiceSection> createState() => _LinuxServiceSectionState();
}

class _LinuxServiceSectionState extends State<LinuxServiceSection> {
  LinuxServiceState? _state;
  Object? _error;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    try {
      final state = await widget.controller.getLinuxServiceState();
      if (mounted) setState(() => _state = state);
    } on Object catch (error) {
      if (mounted) setState(() => _error = error);
    }
  }

  Future<void> _maintain() async {
    final state = _state;
    if (_busy || state == null) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await widget.controller.maintainLinuxService(install: state.canInstall);
      await _refresh();
    } on Object catch (error) {
      // Retain the observed state and action after cancellation/failure so a
      // subsequent user action can retry without restarting onboarding.
      if (mounted) setState(() => _error = error);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final strings = widget.controller.strings;
    final state = _state;
    final actionable =
        state != null &&
        (state.canInstall || state.compatible && state.canRepair);
    return ContentSection(
      icon: LucideIcons.shieldCheck,
      title: strings.get('linux_service_title'),
      subtitle: strings.get(
        state?.compatible == true
            ? 'linux_service_installed'
            : state?.canInstall == true
            ? 'linux_service_install_hint'
            : 'linux_service_reinstall',
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (_error case final error?)
            Padding(
              padding: const EdgeInsets.only(bottom: 12),
              child: Text(userFacingError(strings, error)),
            ),
          if (state == null && _error == null)
            const LinearProgressIndicator()
          else if (state == null)
            TextButton(onPressed: _refresh, child: Text(strings.get('retry')))
          else if (actionable)
            OutlinedButton.icon(
              key: const ValueKey('linux-service-maintain'),
              onPressed: _busy ? null : _maintain,
              icon: _busy
                  ? const SizedBox.square(
                      dimension: 18,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(LucideIcons.shieldCheck),
              label: Text(
                strings.get(
                  state.canInstall
                      ? 'linux_service_install'
                      : 'linux_service_repair',
                ),
              ),
            ),
        ],
      ),
    );
  }
}
