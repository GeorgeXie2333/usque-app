import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/app_strings.dart';
import '../../core/connection_presentation.dart';
import '../../models/app_models.dart';
import '../../models/diagnostics_models.dart';
import '../../state/app_controller.dart';
import '../common.dart';
import '../listenable_selector.dart';
import '../usque_dialog.dart';

/// Asks before a Deep run, which may send probes or reconnect.
Future<bool> confirmDeepDiagnostics(
  BuildContext context,
  AppStrings strings,
) async {
  final confirmed = await showDialog<bool>(
    context: context,
    builder: (context) => UsqueDialog(
      icon: LucideIcons.microscope,
      title: strings.get('nq_doctor_deep_title'),
      content: Text(strings.get('nq_doctor_deep_body')),
      actions: <Widget>[
        TextButton(
          onPressed: () => Navigator.pop(context, false),
          child: Text(strings.get('cancel')),
        ),
        FilledButton(
          onPressed: () => Navigator.pop(context, true),
          child: Text(strings.get('nq_doctor_deep_run')),
        ),
      ],
    ),
  );
  return confirmed == true;
}

/// Mode choice, Start and Cancel. An active or requested run's mode wins
/// over the local choice, so the control always shows what is running.
class DiagnosticRunPanel extends StatefulWidget {
  const DiagnosticRunPanel({required this.controller, super.key});
  final AppController controller;

  @override
  State<DiagnosticRunPanel> createState() => _DiagnosticRunPanelState();
}

class _DiagnosticRunPanelState extends State<DiagnosticRunPanel> {
  DiagnosticMode _selectedMode = DiagnosticMode.standard;

  AppController get controller => widget.controller;

  Future<void> _start(DiagnosticMode mode) async {
    if (mode == DiagnosticMode.deep &&
        !await confirmDeepDiagnostics(context, controller.strings)) {
      return;
    }
    await controller.diagnostics.start(mode);
  }

  @override
  Widget build(BuildContext context) => ListenableSelector(
    listenables: <Listenable>[controller, controller.diagnostics],
    selector: () {
      final diagnostics = controller.diagnostics;
      final session = diagnostics.session;
      return (
        catalog: controller.strings.catalogId,
        phase: controller.snapshot.phase,
        busy: diagnostics.busy,
        canCancel: diagnostics.canCancel,
        state: diagnostics.state,
        runningMode: session?.isActive == true
            ? session!.mode
            : diagnostics.requestedMode,
      );
    },
    builder: (context, view) {
      final strings = controller.strings;
      final presentation = ConnectionPresentation.of(view.phase);
      final selectedMode = view.runningMode ?? _selectedMode;
      final cancelling = view.state == DiagnosticsControllerState.cancelling;
      return ContentSection(
        icon: LucideIcons.stethoscope,
        title: strings.get('diag_run_title'),
        subtitle: strings.get('diag_run_subtitle'),
        trailing: InlineStatus(
          label: strings.get(presentation.labelKey),
          tone: presentation.tone,
          icon: switch (view.phase) {
            ConnectionPhase.connected ||
            ConnectionPhase.degraded => LucideIcons.circleCheck,
            ConnectionPhase.error => LucideIcons.circleX,
            _ => LucideIcons.circle,
          },
        ),
        children: <Widget>[
          SegmentedButton<DiagnosticMode>(
            segments: <ButtonSegment<DiagnosticMode>>[
              ButtonSegment<DiagnosticMode>(
                value: DiagnosticMode.standard,
                icon: const Icon(LucideIcons.gauge),
                label: Text(strings.get('diag_mode_standard')),
              ),
              ButtonSegment<DiagnosticMode>(
                value: DiagnosticMode.deep,
                icon: const Icon(LucideIcons.microscope),
                label: Text(strings.get('diag_mode_deep')),
              ),
            ],
            selected: <DiagnosticMode>{selectedMode},
            onSelectionChanged: view.busy
                ? null
                : (values) => setState(() => _selectedMode = values.first),
            showSelectedIcon: false,
          ),
          if (selectedMode == DiagnosticMode.deep) ...<Widget>[
            const SizedBox(height: 12),
            WarningBanner(
              title: strings.get('diag_deep_title'),
              message: strings.get(
                controller.snapshot.isConnected
                    ? 'diag_deep_connected'
                    : 'diag_deep_disconnected',
              ),
            ),
          ],
          const SizedBox(height: 16),
          Row(
            children: <Widget>[
              Expanded(
                child: FilledButton.icon(
                  onPressed: view.busy ? null : () => _start(selectedMode),
                  icon: view.state == DiagnosticsControllerState.starting
                      ? const SizedBox.square(
                          dimension: 17,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(LucideIcons.play),
                  label: Text(strings.get('diag_start')),
                ),
              ),
              if (view.canCancel || cancelling) ...<Widget>[
                const SizedBox(width: 10),
                OutlinedButton.icon(
                  onPressed: cancelling ? null : controller.diagnostics.cancel,
                  icon: const Icon(LucideIcons.square),
                  label: Text(strings.get('cancel')),
                ),
              ],
            ],
          ),
        ],
      );
    },
  );
}
