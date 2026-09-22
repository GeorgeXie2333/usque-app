import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import '../core/chain_strings.dart';
import '../models/app_models.dart' show ConnectionPhase;
import '../models/chain_exit_models.dart';
import '../state/app_controller.dart';
import 'chain_source_icon.dart';
import 'common.dart';
import 'vpn_gate_entry.dart';

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
          : 'disconnected';
      if (source == ChainSource.vpnGate && !disabled) {
        return VpnGateEntry(
          controller: controller,
          onOpen: onOpen,
          title: strings.chain('title'),
          entryKey: const ValueKey('proxy-chain-proxy-entry'),
        );
      }
      return Panel(
        key: const ValueKey('proxy-chain-proxy-entry'),
        padding: const EdgeInsets.all(20),
        onTap: onOpen,
        child: Row(
          children: [
            if (disabled)
              const Icon(LucideIcons.link)
            else
              ChainSourceIcon(source: source),
            const SizedBox(width: 16),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    strings.chain('title'),
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                  const SizedBox(height: 8),
                  if (!disabled) Text(source.label),
                  Text(strings.chain(state)),
                  if (current != null && connected)
                    Text('WARP → ${current.name}')
                  else if (disabled)
                    Text(strings.chain('subtitle')),
                ],
              ),
            ),
            const Icon(LucideIcons.chevronRight),
          ],
        ),
      );
    },
  );
}
