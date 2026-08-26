import 'package:flutter/material.dart';

import '../core/usque_theme.dart';

/// The one dialog shape in the app.
///
/// Material centres the icon above the title; Usque puts it in a tinted tile on
/// the leading edge instead, so a dialog reads like every other panel: a marked
/// header, a hairline rule, then the work.
class UsqueDialog extends StatelessWidget {
  const UsqueDialog({
    required this.icon,
    required this.title,
    required this.content,
    required this.actions,
    this.subtitle,
    this.width = 480,
    this.danger = false,
    this.scrollable = true,
    super.key,
  });

  final IconData icon;
  final String title;
  final String? subtitle;
  final Widget content;
  final List<Widget> actions;

  /// Preferred width. Narrow viewports clamp it down to the dialog's own room.
  final double width;

  /// Tints the header for destructive work.
  final bool danger;

  final bool scrollable;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final UsqueTokens tokens = UsqueTokens.of(context);
    final Color accent = danger ? tokens.danger : theme.colorScheme.primary;

    return AlertDialog(
      titlePadding: const EdgeInsets.fromLTRB(22, 22, 22, 0),
      contentPadding: const EdgeInsets.fromLTRB(22, 18, 22, 0),
      actionsPadding: const EdgeInsets.fromLTRB(18, 18, 18, 16),
      title: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Container(
            width: 38,
            height: 38,
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: accent.withValues(alpha: tokens.tint),
              borderRadius: BorderRadius.circular(UsqueRadii.chip),
            ),
            child: Icon(icon, size: 19, color: accent),
          ),
          const SizedBox(width: 13),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Text(title, style: theme.textTheme.titleLarge),
                if (subtitle case final subtitle?) ...<Widget>[
                  const SizedBox(height: 4),
                  Text(
                    subtitle,
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: theme.colorScheme.onSurfaceVariant,
                    ),
                  ),
                ],
              ],
            ),
          ),
        ],
      ),
      content: SizedBox(
        width: width,
        child: scrollable ? SingleChildScrollView(child: content) : content,
      ),
      actions: actions,
    );
  }
}

/// Hairline group used inside dialogs to fence off a set of related fields.
class DialogGroup extends StatelessWidget {
  const DialogGroup({
    required this.child,
    this.padding = const EdgeInsets.all(14),
    super.key,
  });

  final Widget child;
  final EdgeInsetsGeometry padding;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    return Material(
      color: theme.colorScheme.surfaceContainerLow,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(UsqueRadii.card),
        side: BorderSide(color: UsqueTokens.of(context).hairline),
      ),
      clipBehavior: Clip.antiAlias,
      child: Padding(padding: padding, child: child),
    );
  }
}
