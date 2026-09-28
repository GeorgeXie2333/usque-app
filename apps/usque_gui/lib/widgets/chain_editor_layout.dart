import 'package:flutter/material.dart';

/// Source editors supply their draft controls and body to the chain page.
/// The page owns the common heading, switch, runtime state and scroll frame.
typedef ChainPageBuilder =
    Widget Function({
      required bool enabled,
      required ValueChanged<bool>? onEnabledChanged,
      required Widget bottomBar,
      required List<Widget> slivers,
      Widget? warning,
    });

/// One choice-row grammar for imported configurations and public nodes.
/// A surrounding RadioGroup owns selection and keyboard navigation.
class ChainExitTile<T> extends StatelessWidget {
  const ChainExitTile({
    required this.value,
    required this.selected,
    required this.enabled,
    required this.title,
    required this.subtitle,
    this.trailing,
    this.tileKey,
    super.key,
  });

  final T value;
  final bool selected, enabled;
  final Widget title, subtitle;
  final Widget? trailing;
  final Key? tileKey;

  @override
  Widget build(BuildContext context) => RadioListTile<T>(
    key: tileKey,
    value: value,
    selected: selected,
    enabled: enabled,
    contentPadding: EdgeInsets.zero,
    controlAffinity: ListTileControlAffinity.leading,
    selectedTileColor: Theme.of(
      context,
    ).colorScheme.primary.withValues(alpha: .06),
    title: title,
    subtitle: subtitle,
    secondary: trailing,
  );
}
