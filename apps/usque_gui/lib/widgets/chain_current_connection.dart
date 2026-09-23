import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/chain_strings.dart';
import '../core/vpn_gate_presentation.dart';
import '../core/warp_strings.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import 'chain_source_icon.dart';
import 'common.dart';
import 'controller_selector.dart';
import 'vpn_gate_summary.dart';

/// Runtime state of the chain, independent of the draft being edited.
class ChainCurrentConnection extends StatelessWidget {
  const ChainCurrentConnection({
    required this.controller,
    required this.configuredEnabled,
    super.key,
  });
  final AppController controller;
  final bool configuredEnabled;

  @override
  Widget build(BuildContext context) =>
      ControllerSelector<
        (
          ChainExitStatus,
          VpnGateStatus,
          ConnectionPhase,
          String?,
          String?,
          String,
        )
      >(
        controller: controller,
        selector: (app) => (
          app.snapshot.chainExit,
          app.snapshot.vpnGate,
          app.snapshot.phase,
          app.snapshot.warning,
          app.lastError,
          app.strings.catalogId,
        ),
        builder: (context, _) => _build(context),
      );

  Widget _build(BuildContext context) {
    final strings = controller.strings;
    final theme = Theme.of(context);
    final snapshot = controller.snapshot;
    final chain = snapshot.chainExit;
    final current = chain.currentProfile;
    final gateView =
        current == null &&
            (snapshot.vpnGate.stage != 'disabled' ||
                controller.activeProfile.chainSource == ChainSource.vpnGate)
        ? VpnGatePresentation(snapshot, configuredEnabled: configuredEnabled)
        : null;
    final gateServer = gateView?.server;
    final hasSession =
        current != null || gateServer != null || snapshot.isTransitional;
    final stateKey =
        snapshot.phase == ConnectionPhase.disconnecting && hasSession
        ? 'disconnecting'
        : chain.stage == 'error' ||
              snapshot.phase == ConnectionPhase.error && configuredEnabled
        ? 'error'
        : !hasSession
        ? configuredEnabled
              ? 'disconnected'
              : 'disabled'
        : chain.stage == 'connected' && snapshot.isConnected
        ? 'connected'
        : snapshot.isTransitional
        ? 'connecting'
        : 'disconnected';
    final statusLabel = gateView != null
        ? switch (gateView.statusKey) {
            'gate_disabled' => strings.chain('disabled'),
            'gate_no_connection' => strings.chain('disconnected'),
            _ => strings.get(gateView.statusKey),
          }
        : strings.chain(stateKey);
    final tone = gateView != null
        ? gateView.failed
              ? StatusTone.danger
              : gateView.connected
              ? StatusTone.success
              : gateView.busy
              ? StatusTone.brand
              : StatusTone.neutral
        : switch (stateKey) {
            'connected' => StatusTone.success,
            'error' => StatusTone.danger,
            'connecting' || 'disconnecting' => StatusTone.brand,
            _ => StatusTone.neutral,
          };
    final failure = chain.failure;
    String reason(String value) {
      final key = 'failure_$value';
      final label = strings.chain(key);
      return label == strings.chain('invalid_configuration') ? value : label;
    }

    return ContentSection(
      key: const ValueKey('chain-current-connection'),
      title: strings.chain('current'),
      gap: 8,
      child: PanelStack(
        spacing: 6,
        children: [
          Semantics(
            liveRegion: true,
            child: InlineStatus(label: statusLabel, tone: tone),
          ),
          if (gateView?.warpKey case final warpKey?)
            Text(
              'WARP: ${strings.get(warpKey)}',
              style: theme.textTheme.bodyMedium?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
              ),
            ),
          if (current != null)
            Row(
              children: [
                ChainSourceIcon(
                  source: current.source,
                  size: 18,
                  color: theme.colorScheme.onSurfaceVariant,
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text('${current.source.label} · ${current.name}'),
                ),
              ],
            )
          else if (gateServer != null) ...[
            Row(
              children: [
                Icon(
                  LucideIcons.globe,
                  size: 18,
                  color: theme.colorScheme.onSurfaceVariant,
                ),
                const SizedBox(width: 8),
                const Expanded(child: Text('VPN Gate')),
              ],
            ),
            VpnGateNodeIdentity(server: gateServer),
          ],
          if (chain.attemptingEndpoint case final endpoint?)
            ReadoutRow(
              label: strings.chain('attempting'),
              stackWhenNarrow: true,
              value: MonoValue(
                value:
                    '${endpoint.label} (${chain.attemptCount}/${chain.candidateCount})',
              ),
            ),
          if (chain.activeEndpoint case final endpoint?)
            ReadoutRow(
              label: strings.chain('actual_endpoint'),
              stackWhenNarrow: true,
              value: MonoValue(value: endpoint),
            ),
          if (chain.attemptFailures.isNotEmpty)
            Text(
              '${strings.chain('attempt_failures')}: ${chain.attemptFailures.map(reason).join(', ')}',
              style: theme.textTheme.bodySmall?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
              ),
            ),
          if (chain.warpObservation case final observed?)
            for (final family in ['ipv4', 'ipv6'])
              Builder(
                builder: (context) {
                  final value =
                      observed[family] as Map<Object?, Object?>? ?? const {};
                  return Text(
                    'Cloudflare · ${family.toUpperCase()} · ${value['country'] ?? strings.warp('unknown')} · ${value['exit_ip'] ?? '—'} · ${value['colo'] ?? '—'} · ${value['response_ms'] ?? '—'} ms\n${strings.warp('observed')}: ${observed['checked_at']}',
                  );
                },
              ),
          if (failure != null)
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: WarningBanner(
                key: const ValueKey('chain-failure'),
                title: strings.chain('error'),
                message: failure == 'authentication'
                    ? strings.chain('authentication_failed')
                    : strings
                          .chain('failure_reason')
                          .replaceAll('{reason}', reason(failure)),
                danger: true,
              ),
            ),
          if (chain.dnsUnavailable)
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: WarningBanner(
                title: strings.chain('dns_unavailable_title'),
                message: strings.chain('dns_unavailable'),
              ),
            ),
          if (gateView?.failed == true)
            if ((snapshot.warning ??
                    controller.lastError ??
                    snapshot.vpnGate.failure)
                case final error?)
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: TextButton.icon(
                  onPressed: () => showVpnGateError(context, strings, error),
                  icon: const Icon(LucideIcons.info, size: 18),
                  label: Text(strings.get('gate_error_details')),
                ),
              ),
        ],
      ),
    );
  }
}
