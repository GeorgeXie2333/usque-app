import 'package:flutter/material.dart';

import '../core/chain_strings.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import 'chain_source_icon.dart';
import 'common.dart';

/// The three fixed exit sources as one row of choices.
///
/// Every option stays visible, so a source the engine cannot provide is shown
/// disabled with its reason instead of being discovered after selection.
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
      return ContentSection(
        title: strings.chain('source'),
        gap: 12,
        child: Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            for (final option in ChainSource.values)
              Builder(
                builder: (context) {
                  final selected = option == source;
                  final enabled =
                      selected ||
                      available(controller.engineCapabilities, option);
                  return ChoiceChip(
                    key: ValueKey('chain-source-${option.wire}'),
                    avatar: ChainSourceIcon(
                      source: option,
                      size: 18,
                      color: !enabled
                          ? theme.disabledColor
                          : selected
                          ? theme.colorScheme.primary
                          : theme.colorScheme.onSurfaceVariant,
                    ),
                    label: Text(option.label),
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
          ],
        ),
      );
    },
  );
}
