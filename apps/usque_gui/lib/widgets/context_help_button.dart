import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import 'usque_dialog.dart';

/// Opens supplementary information without adding it to the page body.
class ContextHelpButton extends StatelessWidget {
  const ContextHelpButton({
    required this.title,
    required this.message,
    required this.strings,
    super.key,
  });

  final String title;
  final String message;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) => IconButton(
    tooltip: title,
    constraints: const BoxConstraints(minWidth: 48, minHeight: 48),
    icon: const Icon(LucideIcons.info, size: 20),
    onPressed: () => showDialog<void>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: LucideIcons.info,
        title: title,
        content: Text(message),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: Text(strings.get('close')),
          ),
        ],
      ),
    ),
  );
}
