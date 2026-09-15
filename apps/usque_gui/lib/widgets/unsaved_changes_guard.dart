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
  State<UnsavedChangesGuard> createState() => UnsavedChangesGuardState();
}

class UnsavedChangesGuardState extends State<UnsavedChangesGuard> {
  bool _discarding = false;
  Future<bool>? _confirmation;

  /// Shares the route's discard decision with navigation outside this route.
  Future<bool> confirmLeave() async {
    if (widget.saving) return false;
    if (_discarding || !widget.dirty) return true;
    final pending = _confirmation;
    if (pending != null) return pending;
    final confirmation = _confirmDiscard();
    _confirmation = confirmation;
    try {
      return await confirmation;
    } finally {
      _confirmation = null;
    }
  }

  Future<bool> _confirmDiscard() async {
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
    if (!mounted || discard != true || widget.saving) return false;
    setState(() => _discarding = true);
    // Pop only after the updated PopScope registration permits it.
    await WidgetsBinding.instance.endOfFrame;
    return mounted && !widget.saving;
  }

  @override
  Widget build(BuildContext context) => PopScope<Object?>(
    canPop: _discarding || !widget.dirty && !widget.saving,
    onPopInvokedWithResult: (didPop, _) async {
      if (didPop) return;
      final navigator = Navigator.of(context);
      if (await confirmLeave() && mounted && navigator.mounted) {
        if (navigator.canPop()) navigator.pop();
      }
    },
    child: widget.child,
  );
}
