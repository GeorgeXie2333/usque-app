import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import 'usque_dialog.dart';

/// Intercepts both the visible back link and platform back navigation.
class UnsavedChangesGuard extends StatefulWidget {
  const UnsavedChangesGuard({
    required this.strings,
    required this.dirty,
    required this.saving,
    required this.child,
    super.key,
  });
  final AppStrings strings;
  final bool dirty;
  final bool saving;
  final Widget child;
  @override
  State<UnsavedChangesGuard> createState() => _UnsavedChangesGuardState();
}

class _UnsavedChangesGuardState extends State<UnsavedChangesGuard> {
  bool _discarding = false;
  bool _confirming = false;

  Future<void> _confirmDiscard() async {
    if (_confirming || widget.saving || !widget.dirty) return;
    _confirming = true;
    final strings = widget.strings;
    final discard = await showDialog<bool>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: LucideIcons.filePen,
        title: strings.get('discard_changes_title'),
        content: Text(strings.get('discard_changes_body')),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(strings.get('discard_changes')),
          ),
          FilledButton(
            autofocus: true,
            onPressed: () => Navigator.pop(context, false),
            child: Text(strings.get('keep_editing')),
          ),
        ],
      ),
    );
    _confirming = false;
    if (!mounted || discard != true || widget.saving) return;
    setState(() => _discarding = true);
    // Pop only after the updated PopScope registration permits it.
    await WidgetsBinding.instance.endOfFrame;
    if (mounted && Navigator.of(context).canPop()) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) => PopScope<Object?>(
    canPop: _discarding || !widget.dirty && !widget.saving,
    onPopInvokedWithResult: (didPop, _) async {
      if (!didPop) await _confirmDiscard();
    },
    child: widget.child,
  );
}
