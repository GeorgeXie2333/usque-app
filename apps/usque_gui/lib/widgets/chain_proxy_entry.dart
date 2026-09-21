import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import '../core/chain_strings.dart';
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
      if (source == ChainSource.vpnGate) {
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
                  Text(source.label),
                  if (current != null && controller.snapshot.isConnected)
                    Text('WARP → ${current.name}')
                  else
                    Text(
                      strings.chain(
                        profile.chainEnabled ? 'disconnected' : 'subtitle',
                      ),
                    ),
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
