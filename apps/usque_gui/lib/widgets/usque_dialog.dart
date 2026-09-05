import 'package:flutter/material.dart';

import '../core/usque_theme.dart';

/// The one dialog shape in the app.
///
/// A quiet leading icon and aligned title keep the content primary. Destructive
/// operations retain their semantic danger color and explicit confirmation.
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
      titlePadding: const EdgeInsets.fromLTRB(24, 24, 24, 0),
      contentPadding: const EdgeInsets.fromLTRB(24, 24, 24, 0),
      actionsPadding: const EdgeInsets.fromLTRB(16, 24, 16, 16),
      title: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Padding(
            padding: const EdgeInsets.only(top: 2),
            child: Icon(icon, size: 24, color: accent),
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

/// Related dialog fields use spacing, not a nested card surface.
class DialogGroup extends StatelessWidget {
  const DialogGroup({
    required this.child,
    this.padding = const EdgeInsets.symmetric(vertical: 16),
    super.key,
  });

  final Widget child;
  final EdgeInsetsGeometry padding;

  @override
  Widget build(BuildContext context) => Padding(padding: padding, child: child);
}
