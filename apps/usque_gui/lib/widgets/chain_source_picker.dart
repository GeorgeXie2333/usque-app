import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/chain_strings.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import 'chain_source_icon.dart';
import 'common.dart';

/// The three fixed exit sources as a row of choices.
///
/// Every option stays visible, so a source the engine cannot provide is shown
/// disabled with its reason instead of being discovered after selection. On
/// phones and other narrow layouts the choices stack one per line; wider
/// layouts keep the wrapping row.
class ChainSourcePicker extends StatelessWidget {
  const ChainSourcePicker({
    required this.controller,
    required this.source,
    required this.onChanged,
    super.key,
  });
  final AppController controller;
  final ChainSource source;
  final ValueChanged<ChainSource> onChanged;

  /// Below this width each choice takes its own line.
  static const double stackBelowWidth = 600;

  static bool available(EngineCapabilities? capabilities, ChainSource source) {
    if (capabilities == null) return true;
    return switch (source) {
      ChainSource.openvpnCustom => capabilities.chainProfileImport,
      ChainSource.wireguardCustom =>
        capabilities.chainProfileImport && capabilities.chainWireguard,
      ChainSource.vpnGate => capabilities.vpnGateTcp,
    };
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: controller,
    builder: (context, _) {
      final strings = controller.strings;
      final theme = Theme.of(context);
      final chips = [
        for (final option in ChainSource.values)
          Builder(
            builder: (context) {
              final selected = option == source;
              final enabled =
                  selected || available(controller.engineCapabilities, option);
              final foreground = !enabled
                  ? theme.disabledColor
                  : selected
                  ? theme.colorScheme.primary
                  : theme.colorScheme.onSurfaceVariant;
              return ChoiceChip(
                key: ValueKey('chain-source-${option.wire}'),
                // The selection mark follows the name instead of covering the
                // source icon.
                showCheckmark: false,
                avatar: ChainSourceIcon(
                  source: option,
                  size: 18,
                  color: foreground,
                ),
                label: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(option.label),
                    if (selected) ...[
                      const SizedBox(width: 6),
                      Icon(LucideIcons.check, size: 16, color: foreground),
                    ],
                  ],
                ),
                selected: selected,
                tooltip: enabled ? null : strings.chain('unsupported'),
                onSelected: enabled
                    ? (_) {
                        if (!selected) onChanged(option);
                      }
                    : null,
              );
            },
          ),
      ];
      return ContentSection(
        title: strings.chain('source'),
        gap: 10,
        child: LayoutBuilder(
          builder: (context, constraints) {
            final stacked =
                constraints.maxWidth < stackBelowWidth ||
                MediaQuery.textScalerOf(context).scale(14) > 21;
            if (stacked) {
              return Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                spacing: 8,
                children: chips,
              );
            }
            return Wrap(spacing: 8, runSpacing: 8, children: chips);
          },
        ),
      );
    },
  );
}
