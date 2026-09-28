import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import '../core/chain_strings.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart' show ConnectionPhase;
import '../models/chain_exit_models.dart';
import '../state/app_controller.dart';
import 'chain_source_icon.dart';
import 'common.dart';
import 'vpn_gate_entry.dart';

/// Proxy-page shortcut to the chain proxy. Mirrors [VpnGateEntry]'s shape so
/// the card reads the same whichever source is selected.
class ChainProxyEntry extends StatelessWidget {
  const ChainProxyEntry({
    required this.controller,
    required this.onOpen,
    super.key,
  });
  final AppController controller;
  final VoidCallback onOpen;
  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: controller,
    builder: (context, _) {
      final profile = controller.activeProfile;
      final current = controller.snapshot.chainExit.currentProfile;
      final source = current?.source ?? profile.chainSource;
      final strings = controller.strings;
      final theme = Theme.of(context);
      final tokens = UsqueTokens.of(context);
      final stage = source == ChainSource.vpnGate
          ? controller.snapshot.vpnGate.stage
          : controller.snapshot.chainExit.stage;
      final connecting = const {
        'connecting_warp',
        'connecting_server',
        'negotiating',
        'configuring_network',
        'reconnecting',
      }.contains(stage);
      final connected = stage == 'connected' && controller.snapshot.isConnected;
      final disconnecting =
          controller.snapshot.phase == ConnectionPhase.disconnecting &&
          (current != null ||
              controller.snapshot.vpnGate.server != null ||
              connecting);
      final disabled =
          !profile.chainEnabled && !connecting && !connected && !disconnecting;
      final state = disabled
          ? 'disabled'
          : disconnecting
          ? 'disconnecting'
          : connected
          ? 'connected'
          : connecting
          ? 'connecting'
          : stage == 'error'
          ? 'error'
          : 'enabled_idle';
      if (source == ChainSource.vpnGate && !disabled) {
        return VpnGateEntry(
          controller: controller,
          onOpen: onOpen,
          title: strings.chain('title'),
          chainEntry: true,
          entryKey: const ValueKey('proxy-chain-proxy-entry'),
        );
      }
      final tone = switch (state) {
        'connected' => StatusTone.success,
        'error' => StatusTone.danger,
        'connecting' || 'disconnecting' => StatusTone.brand,
        _ => StatusTone.neutral,
      };
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
                  strings.chain('manage'),
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
            key: const ValueKey('proxy-chain-proxy-entry'),
            onTap: onOpen,
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
                    alignment: Alignment.center,
                    decoration: BoxDecoration(
                      color: theme.colorScheme.primary.withValues(
                        alpha: tokens.tint,
                      ),
                      borderRadius: BorderRadius.circular(UsqueRadii.control),
                    ),
                    child: disabled
                        ? Icon(
                            LucideIcons.link,
                            size: compact ? 22 : 24,
                            color: theme.colorScheme.primary,
                          )
                        : ChainSourceIcon(
                            source: source,
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
                          Text(
                            strings.chain('title'),
                            style: theme.textTheme.titleLarge,
                          ),
                          if (!disabled) Text(source.label),
                          Semantics(
                            liveRegion: true,
                            child: Container(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 8,
                                vertical: 4,
                              ),
                              decoration: BoxDecoration(
                                color: statusColor.withValues(
                                  alpha: tokens.tint,
                                ),
                                borderRadius: BorderRadius.circular(
                                  UsqueRadii.chip,
                                ),
                              ),
                              child: InlineStatus(
                                label: strings.chain(state),
                                tone: tone,
                              ),
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 8),
                      if (current != null) ...[
                        Text(
                          strings.chain('current'),
                          style: theme.textTheme.bodySmall?.copyWith(
                            color: theme.colorScheme.onSurfaceVariant,
                          ),
                        ),
                        const SizedBox(height: 4),
                        Text(
                          '${current.host}:${current.port} · ${current.transportLabel}',
                          style: theme.textTheme.bodyMedium?.copyWith(
                            fontFamily: UsqueFonts.mono,
                          ),
                        ),
                        if (connected) ...[
                          const SizedBox(height: 6),
                          Text(
                            'WARP → ${current.name}',
                            style: theme.textTheme.bodySmall?.copyWith(
                              color: theme.colorScheme.onSurfaceVariant,
                            ),
                          ),
                        ],
                      ] else
                        Text(
                          strings.chain('subtitle'),
                          style: theme.textTheme.bodyMedium?.copyWith(
                            color: theme.colorScheme.onSurfaceVariant,
                          ),
                        ),
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
    },
  );
}
