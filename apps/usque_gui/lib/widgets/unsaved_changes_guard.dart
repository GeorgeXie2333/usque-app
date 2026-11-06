import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import 'section_navigator.dart';
import 'tv_input_scope.dart';
import 'usque_dialog.dart';

/// Intercepts both the visible back link and platform back navigation.
class UnsavedChangesGuard extends StatefulWidget {
  const UnsavedChangesGuard({
    required this.strings,
    required this.dirty,
    required this.saving,
    required this.child,
    this.discardTitle,
    this.discardBody,
    super.key,
  });
  final AppStrings strings;
  final bool dirty;
  final bool saving;
  final Widget child;
  final String? discardTitle;
  final String? discardBody;
  @override
  State<UnsavedChangesGuard> createState() => UnsavedChangesGuardState();
}

class UnsavedChangesGuardState extends State<UnsavedChangesGuard> {
  bool _discarding = false;
  Future<bool>? _confirmation;
  SectionNavigatorState? _section;

  bool get canLeave => !widget.saving && (_discarding || !widget.dirty);

  /// A section departure may be cancelled by another guarded page. Its
  /// earlier confirmations must not authorize later edits or back navigation.
  bool cancelLeave() {
    if (!mounted || !_discarding) return false;
    setState(() => _discarding = false);
    return true;
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final section = SectionNavigator.maybeOf(context);
    if (section == _section) return;
    _section?.unregisterGuard(this);
    _section = section?..registerGuard(this);
  }

  @override
  void dispose() {
    _section?.unregisterGuard(this);
    super.dispose();
  }

  /// Shares the route's discard decision with navigation outside this route.
  Future<bool> confirmLeave() async {
    if (TvInputScope.maybeOf(context)?.consumeBack(context) ?? false) {
      return false;
    }
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
        title: widget.discardTitle ?? strings.get('discard_changes_title'),
        content: Text(
          widget.discardBody ?? strings.get('discard_changes_body'),
        ),
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
    if (!mounted) return false;
    if (widget.saving) {
      cancelLeave();
      await WidgetsBinding.instance.endOfFrame;
      return false;
    }
    return true;
  }

  @override
  Widget build(BuildContext context) => PopScope<Object?>(
    canPop: canLeave,
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
