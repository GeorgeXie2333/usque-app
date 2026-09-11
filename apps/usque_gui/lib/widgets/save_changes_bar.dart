import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_theme.dart';
import 'common.dart';

/// A persistent action area outside the form's scrollable content.
class SaveChangesBar extends StatelessWidget {
  const SaveChangesBar({
    required this.strings,
    required this.dirty,
    required this.saving,
    required this.onSave,
    this.error,
    this.saved = false,
    this.savedLabel,
    this.idleHint,
    this.statusLabel,
    this.onReconnect,
    super.key,
  });

  final AppStrings strings;
  final bool dirty;
  final bool saving;
  final bool saved;
  final String? savedLabel;
  final String? idleHint;
  final String? statusLabel;
  final VoidCallback? onReconnect;
  final String? error;
  final VoidCallback? onSave;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final message =
        statusLabel ??
        error ??
        (saving
            ? strings.get('saving_changes')
            : dirty
            ? strings.get('unsaved_changes')
            : saved
            ? savedLabel ?? strings.get('changes_applied')
            : idleHint ?? strings.get('changes_apply_hint'));
    return Material(
      color: theme.colorScheme.surface,
      child: DecoratedBox(
        decoration: BoxDecoration(
          border: Border(
            top: BorderSide(color: UsqueTokens.of(context).hairline),
          ),
        ),
        child: SafeArea(
          top: false,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
            child: Center(
              heightFactor: 1,
              child: ConstrainedBox(
                constraints: const BoxConstraints(
                  maxWidth: PageFrame.maxContentWidth,
                ),
                child: LayoutBuilder(
                  builder: (context, constraints) {
                    final status = Semantics(
                      liveRegion: true,
                      child: Text(
                        message,
                        style: theme.textTheme.bodyMedium?.copyWith(
                          color: error != null
                              ? theme.colorScheme.error
                              : theme.colorScheme.onSurfaceVariant,
                        ),
                      ),
                    );
                    final saveButton = FilledButton.icon(
                      onPressed: saving || (!dirty && error == null)
                          ? null
                          : onSave,
                      icon: saving
                          ? const SizedBox.square(
                              dimension: 18,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Icon(LucideIcons.check, size: 18),
                      label: Text(
                        strings.get(saving ? 'saving_changes' : 'save_changes'),
                      ),
                    );
                    final save = onReconnect == null
                        ? saveButton
                        : Wrap(
                            spacing: 8,
                            runSpacing: 8,
                            alignment: WrapAlignment.end,
                            children: [
                              if (onReconnect != null)
                                OutlinedButton(
                                  onPressed: onReconnect,
                                  child: Text(
                                    strings.get('settings_reconnect'),
                                  ),
                                ),
                              saveButton,
                            ],
                          );
                    if (constraints.maxWidth < 520 ||
                        MediaQuery.textScalerOf(context).scale(14) > 21) {
                      return Column(
                        mainAxisSize: MainAxisSize.min,
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [status, const SizedBox(height: 8), save],
                      );
                    }
                    return Row(
                      children: [
                        Expanded(child: status),
                        const SizedBox(width: 16),
                        save,
                      ],
                    );
                  },
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
