import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/chain_strings.dart';
import '../core/usque_motion.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import 'chain_source_icon.dart';
import 'common.dart';

/// The fixed exit sources, with a compact picker on narrow layouts.
///
/// The sheet and wide-layout choices show unavailable sources with their reason.
/// The compact trigger keeps the selected choice's dimensions below the heading.
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

  /// Below this width the current choice opens a sheet with all sources.
  static const double compactBelowWidth = 600;

  static bool available(EngineCapabilities? capabilities, ChainSource source) {
    if (capabilities == null) return source != ChainSource.warpWireguard;
    return switch (source) {
      ChainSource.openvpnCustom => capabilities.chainProfileImport,
      ChainSource.wireguardCustom =>
        capabilities.chainProfileImport && capabilities.chainWireguard,
      ChainSource.warpWireguard =>
        capabilities.chainProfileImport &&
            capabilities.chainWireguard &&
            capabilities.chainWarpWireguard,
      ChainSource.vpnGate => capabilities.vpnGateTcp,
    };
  }

  Future<void> _chooseSource(BuildContext context) async {
    final choice = await showModalBottomSheet<ChainSource>(
      context: context,
      useSafeArea: true,
      isScrollControlled: true,
      showDragHandle: true,
      requestFocus: true,
      sheetAnimationStyle: UsqueMotion.reduced(context)
          ? AnimationStyle.noAnimation
          : null,
      builder: (context) => ListenableBuilder(
        listenable: controller,
        builder: (context, _) {
          return SafeArea(
            top: false,
            child: SingleChildScrollView(
              padding: const EdgeInsets.fromLTRB(24, 0, 24, 24),
              child: ContentSection(
                title: controller.strings.chain('source'),
                gap: 8,
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    for (final option in ChainSource.values)
                      _sheetOption(context, option),
                  ],
                ),
              ),
            ),
          );
        },
      ),
    );
    if (context.mounted &&
        choice != null &&
        choice != source &&
        available(controller.engineCapabilities, choice)) {
      onChanged(choice);
    }
  }

  Widget _sheetOption(BuildContext context, ChainSource option) {
    final theme = Theme.of(context);
    final selected = option == source;
    final enabled =
        selected || available(controller.engineCapabilities, option);
    final foreground = !enabled
        ? theme.disabledColor
        : selected
        ? theme.colorScheme.primary
        : theme.colorScheme.onSurfaceVariant;
    return Semantics(
      inMutuallyExclusiveGroup: true,
      child: ListTile(
        key: ValueKey('chain-source-option-${option.wire}'),
        selected: selected,
        enabled: enabled,
        leading: ChainSourceIcon(source: option, size: 18, color: foreground),
        title: Text(option.label),
        trailing: selected ? const Icon(LucideIcons.check, size: 16) : null,
        subtitle: enabled
            ? null
            : Text(controller.strings.chain('unsupported')),
        onTap: enabled ? () => Navigator.of(context).pop(option) : null,
      ),
    );
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: controller,
    builder: (context, _) {
      final strings = controller.strings;
      final theme = Theme.of(context);
      Widget chip(ChainSource option, {bool compact = false}) {
        final selected = option == source;
        final enabled =
            selected || available(controller.engineCapabilities, option);
        final foreground = !enabled
            ? theme.disabledColor
            : selected
            ? theme.colorScheme.primary
            : theme.colorScheme.onSurfaceVariant;
        return ChoiceChip(
          key: ValueKey(
            compact ? 'chain-source-picker' : 'chain-source-${option.wire}',
          ),
          // The selection mark follows the name instead of covering the
          // source icon.
          showCheckmark: false,
          avatar: ChainSourceIcon(source: option, size: 18, color: foreground),
          label: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Flexible(
                child: Text(
                  option.label,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              if (selected) ...[
                const SizedBox(width: 6),
                Icon(
                  compact ? LucideIcons.chevronDown : LucideIcons.check,
                  size: 16,
                  color: foreground,
                ),
              ],
            ],
          ),
          selected: selected,
          tooltip: compact
              ? strings.chain('source')
              : enabled
              ? null
              : strings.chain('unsupported'),
          onSelected: enabled
              ? (_) {
                  if (compact) {
                    _chooseSource(context);
                  } else if (!selected) {
                    onChanged(option);
                  }
                }
              : null,
        );
      }

      return ContentSection(
        title: strings.chain('source'),
        gap: 10,
        child: LayoutBuilder(
          builder: (context, constraints) {
            if (constraints.maxWidth < compactBelowWidth) {
              return Align(
                alignment: AlignmentDirectional.centerStart,
                child: chip(source, compact: true),
              );
            }
            final chips = [
              for (final option in ChainSource.values) chip(option),
            ];
            if (MediaQuery.textScalerOf(context).scale(14) > 21) {
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
