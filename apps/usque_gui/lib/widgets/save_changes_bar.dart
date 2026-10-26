import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_motion.dart';
import '../core/usque_theme.dart';
import 'common.dart';
import 'desktop_shortcuts.dart';

/// A persistent action area outside the form's scrollable content.
class SaveChangesBar extends StatelessWidget {
  const SaveChangesBar({
    required this.strings,
    required this.dirty,
    required this.saving,
    required this.onSave,
    this.error,
    this.validationError,
    this.saved = false,
    this.savedLabel,
    this.idleHint,
    this.statusLabel,
    this.onReconnect,
    this.onRefresh,
    this.refreshing = false,
    this.saveLabel,
    this.summary,
    this.activity,
    this.saveButtonKey,
    this.contentWidth = PageFrame.maxContentWidth,
    this.matchPageGutter = false,
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
  final VoidCallback? onRefresh;
  final bool refreshing;

  /// Contextual label for the primary action, such as an explicit reconnect.
  final String? saveLabel;

  /// What the pending edit will do, shown above the status line.
  final Widget? summary;

  /// Source-specific preparation progress or error details above the action.
  final Widget? activity;
  final Key? saveButtonKey;
  final double contentWidth;
  final bool matchPageGutter;
  final String? error;

  /// Current form validation takes precedence over an earlier engine result.
  final String? validationError;
  final VoidCallback? onSave;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final message =
        validationError ??
        statusLabel ??
        error ??
        (saving
            ? strings.get('saving_changes')
            : dirty
            // A summary already names the pending edit; repeating that edits
            // exist would only add a line.
            ? summary == null
                  ? strings.get('unsaved_changes')
                  : null
            : saved
            ? savedLabel ?? strings.get('changes_applied')
            : idleHint ?? strings.get('changes_apply_hint'));
    final save = saving || (!dirty && error == null) ? null : onSave;
    return PageShortcut(
      activator: const SingleActivator(LogicalKeyboardKey.keyS, control: true),
      onInvoke: save,
      child: _buildBar(context, theme, message, save),
    );
  }

  Widget _buildBar(
    BuildContext context,
    ThemeData theme,
    String? message,
    VoidCallback? onSaveEnabled,
  ) {
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
            padding: EdgeInsets.symmetric(
              horizontal: matchPageGutter
                  ? MediaQuery.sizeOf(context).width < 600
                        ? 16
                        : 32
                  : 20,
              vertical: 12,
            ),
            child: Center(
              heightFactor: 1,
              child: ConstrainedBox(
                constraints: BoxConstraints(maxWidth: contentWidth),
                child: LayoutBuilder(
                  builder: (context, constraints) {
                    final statusText = message == null
                        ? null
                        : Semantics(
                            liveRegion: true,
                            child: Text(
                              message,
                              style: theme.textTheme.bodyMedium?.copyWith(
                                color: validationError != null || error != null
                                    ? theme.colorScheme.error
                                    : theme.colorScheme.onSurfaceVariant,
                              ),
                            ),
                          );
                    final status = summary == null
                        ? statusText!
                        : Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              Semantics(liveRegion: true, child: summary!),
                              if (statusText != null) ...[
                                const SizedBox(height: 6),
                                statusText,
                              ],
                            ],
                          );
                    final saveButton = FilledButton.icon(
                      key: saveButtonKey,
                      onPressed: onSaveEnabled,
                      icon: saving
                          ? const SizedBox.square(
                              dimension: 18,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Icon(LucideIcons.check, size: 18),
                      label: Text(
                        saving
                            ? strings.get('saving_changes')
                            : saveLabel ?? strings.get('save_changes'),
                      ),
                    );
                    final save = onReconnect == null && onRefresh == null
                        ? saveButton
                        : Wrap(
                            spacing: 8,
                            runSpacing: 8,
                            alignment: WrapAlignment.end,
                            children: [
                              if (onRefresh != null)
                                OutlinedButton(
                                  onPressed: refreshing ? null : onRefresh,
                                  child: Text(strings.get('settings_refresh')),
                                )
                              else if (onReconnect != null)
                                OutlinedButton(
                                  onPressed: onReconnect,
                                  child: Text(
                                    strings.get('settings_reconnect'),
                                  ),
                                ),
                              saveButton,
                            ],
                          );
                    Widget withActivity(Widget controls) => activity == null
                        ? controls
                        : Column(
                            mainAxisSize: MainAxisSize.min,
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              activity!,
                              const SizedBox(height: 8),
                              controls,
                            ],
                          );
                    if (constraints.maxWidth < 520 ||
                        MediaQuery.textScalerOf(context).scale(14) > 21) {
                      return withActivity(
                        Column(
                          mainAxisSize: MainAxisSize.min,
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [status, const SizedBox(height: 8), save],
                        ),
                      );
                    }
                    return withActivity(
                      Row(
                        children: [
                          Expanded(child: status),
                          const SizedBox(width: 16),
                          save,
                        ],
                      ),
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

/// Reveals a page's [SaveChangesBar] only while it has something to apply or
/// report. A clean form would otherwise reserve the bottom of the page for a
/// disabled button.
class RevealedSaveBar extends StatelessWidget {
  const RevealedSaveBar({
    required this.visible,
    required this.child,
    super.key,
  });

  final bool visible;
  final Widget child;

  /// Whether a bar with these inputs has an edit, result or action to show.
  static bool hasContent({
    required bool dirty,
    required bool saving,
    bool saved = false,
    String? error,
    String? validationError,
    String? statusLabel,
    VoidCallback? onReconnect,
    VoidCallback? onRefresh,
  }) =>
      dirty ||
      saving ||
      saved ||
      error != null ||
      validationError != null ||
      statusLabel != null ||
      onReconnect != null ||
      onRefresh != null;

  @override
  Widget build(BuildContext context) => AnimatedSwitcher(
    duration: UsqueMotion.of(context, UsqueMotion.fast),
    switchInCurve: UsqueMotion.standard,
    switchOutCurve: UsqueMotion.exit,
    transitionBuilder: (child, animation) => SizeTransition(
      sizeFactor: animation,
      alignment: AlignmentDirectional.topStart,
      child: child,
    ),
    child: visible
        ? KeyedSubtree(key: const ValueKey('revealed-save-bar'), child: child)
        : const SizedBox(width: double.infinity),
  );
}
