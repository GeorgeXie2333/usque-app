import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/quality_presentation.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/controller_selector.dart';
import '../widgets/listenable_selector.dart';
import '../widgets/network_quality/quality_sections.dart';
import 'diagnostics_screen.dart';

/// Live, local-only connection quality. Each section follows only the values
/// it shows, so a new sample repaints the trends rather than the whole page.
class NetworkQualityScreen extends StatelessWidget {
  const NetworkQualityScreen({required this.controller, super.key});
  final AppController controller;

  @override
  Widget build(BuildContext context) => ControllerSelector(
    controller: controller,
    selector: (app) => (
      catalog: app.strings.catalogId,
      gate:
          app.snapshot.isConnected &&
          app.snapshot.chainExit.currentProfile == null &&
          app.snapshot.vpnGate.connected,
      l4: qualityLinkOf(app).l4,
      supported: app.engineCapabilities?.networkQuality ?? false,
    ),
    builder: (context, view) {
      final s = controller.strings;
      return FocusTraversalGroup(
        policy: ReadingOrderTraversalPolicy(),
        child: SubPage(
          title: s.get('network_quality'),
          backLabel: s.get('back'),
          subtitle: s.get(view.gate ? 'gate_quality_scope' : 'nq_subtitle'),
          actions: <Widget>[_DoctorAction(controller: controller)],
          child: PanelStack(
            children: <Widget>[
              if (view.l4) QualityL4Panel(controller: controller),
              if (!view.supported)
                WarningBanner(
                  title: s.get('nq_unsupported'),
                  message: s.get('nq_capability_missing'),
                ),
              QualityOverview(controller: controller),
              QualityTrendsHeading(controller: controller),
              QualityTrends(controller: controller),
              QualityAdvancedMetrics(
                controller: controller,
                onDiagnose: () => openDiagnostics(context, controller),
              ),
              Text(s.get('nq_doctor_help')),
              Text(
                s.get('nq_doctor_evidence'),
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ],
          ),
        ),
      );
    },
  );
}

/// Opens Diagnostics with a Standard run. It is the page's primary action
/// only when the quality level calls for investigation.
class _DoctorAction extends StatelessWidget {
  const _DoctorAction({required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) => ListenableSelector(
    listenables: <Listenable>[
      controller,
      controller.quality,
      controller.diagnostics,
    ],
    selector: () => (
      catalog: controller.strings.catalogId,
      busy: controller.diagnostics.busy,
      emphasized: QualityLevelPresentation.of(
        connected: controller.snapshot.isConnected,
        stale: controller.quality.stale,
        level: controller.quality.latest?.level,
      ).needsDiagnosis,
    ),
    builder: (context, view) {
      const key = ValueKey<String>('network-doctor-standard');
      final onPressed = view.busy
          ? null
          : () => openDiagnostics(context, controller);
      const icon = Icon(LucideIcons.stethoscope);
      final label = Text(controller.strings.get('nq_doctor'));
      return view.emphasized
          ? FilledButton.icon(
              key: key,
              onPressed: onPressed,
              icon: icon,
              label: label,
            )
          : OutlinedButton.icon(
              key: key,
              onPressed: onPressed,
              icon: icon,
              label: label,
            );
    },
  );
}
