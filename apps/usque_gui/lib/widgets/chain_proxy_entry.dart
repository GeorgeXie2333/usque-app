import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/chain_strings.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import 'chain_exit_summary.dart';
import 'common.dart';

/// Proxy-page row for the chain proxy, drawn as a [LinkRow] like every other
/// row that opens a subpage. Every source reads through one summary, so a VPN
/// Gate node and a saved configuration share wording and placement.
class ChainProxyEntry extends StatelessWidget {
  const ChainProxyEntry({
    required this.controller,
    required this.onOpen,
    super.key,
  });
  final AppController controller;
  final VoidCallback onOpen;

  @override
  Widget build(BuildContext context) => ChainExitSummaryBuilder(
    controller: controller,
    section: AppSection.proxy,
    builder: (context, view) {
      final strings = controller.strings;
      final label = view.statusLabel(strings);
      final chosen = view.live || view.selection != null;
      return KeyedSubtree(
        key: const ValueKey('proxy-chain-proxy-section'),
        child: LinkRow(
          key: const ValueKey('proxy-chain-proxy-entry'),
          icon: LucideIcons.link,
          title: strings.chain('title'),
          // Guidance stays until an exit is chosen, even for an enabled chain.
          subtitle: chosen ? null : strings.chain('subtitle'),
          details: chosen || view.enabled
              ? ChainSelectionText(
                  view,
                  key: const ValueKey('proxy-chain-selection'),
                )
              : null,
          status: label == null
              ? null
              : Semantics(
                  liveRegion: true,
                  child: InlineStatus(label: label, tone: view.tone),
                ),
          onTap: onOpen,
        ),
      );
    },
  );
}
