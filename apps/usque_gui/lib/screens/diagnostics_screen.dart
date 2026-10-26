import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../models/diagnostics_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/desktop_shortcuts.dart';
import '../widgets/diagnostics/diagnostic_export_panel.dart';
import '../widgets/diagnostics/diagnostic_results.dart';
import '../widgets/diagnostics/diagnostic_run_panel.dart';
import '../widgets/listenable_selector.dart';

/// Starts a Standard run unless one is already starting, running or stopping.
/// Standard checks do not send traffic or change settings.
void startStandardDiagnostics(AppController controller) {
  final diagnostics = controller.diagnostics;
  if (diagnostics.busy) return;
  unawaited(diagnostics.start(DiagnosticMode.standard));
}

/// Starts a Standard run and shows its progress on the Diagnostics page,
/// pushed on the caller's navigator.
void openDiagnostics(BuildContext context, AppController controller) {
  startStandardDiagnostics(controller);
  unawaited(
    Navigator.of(context).push<void>(
      MaterialPageRoute<void>(
        builder: (_) => DiagnosticsScreen(controller: controller),
      ),
    ),
  );
}

/// Active checks, the connection timeline and the local diagnostic bundle.
/// Clearing all data is an application action and lives in About.
/// Sections follow their own state, so connection samples arriving every
/// second do not repaint the page.
class DiagnosticsScreen extends StatefulWidget {
  const DiagnosticsScreen({required this.controller, super.key});

  final AppController controller;

  @override
  State<DiagnosticsScreen> createState() => _DiagnosticsScreenState();
}

class _DiagnosticsScreenState extends State<DiagnosticsScreen> {
  final _rows = DiagnosticRowKeys();

  AppController get controller => widget.controller;

  @override
  void initState() {
    super.initState();
    controller.diagnostics.beginTimelineUpdates();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        controller.diagnostics.restore(silent: true);
      }
    });
  }

  @override
  void didUpdateWidget(DiagnosticsScreen oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != controller) {
      oldWidget.controller.diagnostics.endTimelineUpdates();
      controller.diagnostics.beginTimelineUpdates();
    }
  }

  @override
  void dispose() {
    controller.diagnostics.endTimelineUpdates();
    super.dispose();
  }

  void _refreshTimeline() {
    final diagnostics = controller.diagnostics;
    if (!diagnostics.timelineLoading) unawaited(diagnostics.loadTimeline());
  }

  @override
  Widget build(BuildContext context) => PageShortcut(
    activator: const SingleActivator(LogicalKeyboardKey.f5),
    onInvoke: _refreshTimeline,
    child: ListenableSelector(
      listenables: <Listenable>[controller, controller.diagnostics],
      selector: () => (
        catalog: controller.strings.catalogId,
        hasSession: controller.diagnostics.session != null,
      ),
      builder: (context, view) {
        final strings = controller.strings;
        return FocusTraversalGroup(
          policy: OrderedTraversalPolicy(),
          child: SubPage(
            // Version, source and licences live in About; this page only
            // checks and records the connection.
            title: strings.get('diagnostics'),
            subtitle: strings.get('diagnostics_page_subtitle'),
            backLabel: strings.get('back'),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                _DiagnosticsBanners(controller: controller),
                PanelStack(
                  spacing: 32,
                  children: <Widget>[
                    DiagnosticRunPanel(controller: controller),
                    if (view.hasSession)
                      DiagnosticSessionPanel(
                        controller: controller,
                        rows: _rows,
                      ),
                    LayoutBuilder(
                      builder: (context, constraints) {
                        final results = DiagnosticResultsPanel(
                          controller: controller,
                          rows: _rows,
                        );
                        final timeline = DiagnosticTimelinePanel(
                          controller: controller,
                        );
                        if (constraints.maxWidth < 840) {
                          return PanelStack(
                            children: <Widget>[results, timeline],
                          );
                        }
                        return Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: <Widget>[
                            Expanded(flex: 6, child: results),
                            const SizedBox(width: 16),
                            Expanded(flex: 5, child: timeline),
                          ],
                        );
                      },
                    ),
                    DiagnosticExportPanel(controller: controller),
                  ],
                ),
              ],
            ),
          ),
        );
      },
    ),
  );
}

class _DiagnosticsBanners extends StatelessWidget {
  const _DiagnosticsBanners({required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) => ListenableSelector(
    listenables: <Listenable>[controller, controller.diagnostics],
    selector: () {
      final snapshot = controller.snapshot;
      final diagnostics = controller.diagnostics;
      return (
        catalog: controller.strings.catalogId,
        error: controller.lastError,
        diagnosticsError: diagnostics.lastError,
        notice: controller.lastNotice,
        statusDegraded:
            controller.snapshotStreamDegraded &&
            (snapshot.isConnected || snapshot.isTransitional),
        eventsDegraded: diagnostics.eventStreamDegraded && diagnostics.isActive,
      );
    },
    builder: (context, view) {
      final strings = controller.strings;
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          BannerSlot(
            child: view.error == null
                ? null
                : WarningBanner(
                    title: strings.get('error_generic'),
                    message: view.error!,
                    danger: true,
                    onDismiss: controller.clearError,
                  ),
          ),
          BannerSlot(
            child: view.diagnosticsError == null
                ? null
                : WarningBanner(
                    title: strings.get('diag_operation_failed'),
                    message: view.diagnosticsError!,
                    danger: true,
                    onDismiss: controller.diagnostics.clearError,
                  ),
          ),
          BannerSlot(
            child: view.notice == null
                ? null
                : WarningBanner(
                    title: strings.get('notice'),
                    message: view.notice!,
                    onDismiss: controller.clearNotice,
                  ),
          ),
          BannerSlot(
            child: view.statusDegraded
                ? WarningBanner(
                    title: strings.get('status_stream_degraded'),
                    message: strings.get('status_stream_degraded_body'),
                  )
                : null,
          ),
          BannerSlot(
            child: view.eventsDegraded
                ? WarningBanner(
                    title: strings.get('diag_event_stream_degraded'),
                    message: strings.get('diag_event_stream_degraded_body'),
                  )
                : null,
          ),
        ],
      );
    },
  );
}
