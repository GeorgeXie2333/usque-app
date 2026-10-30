import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import '../models/bypass_targets.dart';
import 'common.dart';

/// Localized name of a rule action; the wire names stay in the model.
String routingActionLabel(AppStrings strings, RoutingAction action) =>
    strings.get(switch (action) {
      RoutingAction.direct => 'routing_action_direct',
      RoutingAction.reject => 'routing_action_reject',
      RoutingAction.proxy => 'routing_action_proxy',
    });

/// Status presentation for a rule action. The label carries the meaning;
/// the icon and its tone only reinforce it.
InlineStatus routingActionStatus(AppStrings strings, RoutingAction action) =>
    InlineStatus(
      label: routingActionLabel(strings, action),
      tone: switch (action) {
        RoutingAction.direct => StatusTone.neutral,
        RoutingAction.reject => StatusTone.danger,
        RoutingAction.proxy => StatusTone.brand,
      },
      icon: switch (action) {
        RoutingAction.direct => LucideIcons.arrowUpRight,
        RoutingAction.reject => LucideIcons.ban,
        RoutingAction.proxy => LucideIcons.waypoints,
      },
    );

class RoutingRulesEditor extends StatelessWidget {
  const RoutingRulesEditor({
    required this.value,
    required this.strings,
    required this.onChanged,
    this.errorIds = const [],
    this.errorMessage,
    this.overlapIds = const {},
    this.grouped = false,
    super.key,
  });
  final RoutingSettings value;

  /// Draws the rules as a Settings-style [ContentGroup] of inset rows;
  /// otherwise they sit under a heading inside another form.
  final bool grouped;
  final AppStrings strings;
  final ValueChanged<RoutingSettings>? onChanged;
  final List<String> errorIds;

  /// Why the rules in [errorIds] cannot be applied.
  final String? errorMessage;

  /// Rules that overlap a rule with a different action.
  final Set<String> overlapIds;

  Future<void> _edit(
    BuildContext context, {
    RoutingRule? existing,
    bool batch = false,
  }) async {
    var text = existing?.target ?? '';
    var action = existing?.action ?? RoutingAction.reject;
    String? error;
    final result = await showDialog<List<RoutingRule>>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, update) => AlertDialog(
          title: Text(
            strings.get(
              existing != null
                  ? 'edit'
                  : batch
                  ? 'routing_batch'
                  : 'routing_add',
            ),
          ),
          content: SizedBox(
            width: 440,
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  DropdownButtonFormField<RoutingAction>(
                    initialValue: action,
                    style: FieldDropdown.valueStyle(context),
                    iconSize: FieldDropdown.iconSize,
                    decoration: FieldDropdown.decoration(
                      context,
                      labelText: strings.get('routing_action'),
                    ),
                    items: RoutingAction.values
                        .map(
                          (value) => DropdownMenuItem(
                            value: value,
                            child: Text(routingActionLabel(strings, value)),
                          ),
                        )
                        .toList(),
                    onChanged: (value) => update(() => action = value!),
                  ),
                  const SizedBox(height: 16),
                  TextFormField(
                    key: const ValueKey('routing-target-input'),
                    initialValue: text,
                    onChanged: (value) => text = value,
                    minLines: batch ? 4 : 1,
                    maxLines: batch ? 8 : 1,
                    autocorrect: false,
                    enableSuggestions: false,
                    decoration: InputDecoration(
                      labelText: strings.get('routing_target'),
                      hintText: batch
                          ? 'example.com\n192.0.2.0/24\n2001:db8::1'
                          : 'example.com',
                      errorText: error,
                      errorMaxLines: 4,
                    ),
                  ),
                ],
              ),
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: Text(strings.get('cancel')),
            ),
            FilledButton(
              onPressed: () {
                try {
                  if (text.length > 131072) {
                    update(() => error = strings.get('bypass_limit'));
                    return;
                  }
                  final lines = text.split('\n');
                  final added = lines
                      .where((line) => line.trim().isNotEmpty)
                      .take(513)
                      .length;
                  if (value.rules.length + added - (existing == null ? 0 : 1) >
                      512) {
                    update(() => error = strings.get('bypass_limit'));
                    return;
                  }
                  final rules = <RoutingRule>[];
                  for (var index = 0; index < lines.length; index++) {
                    if (lines[index].trim().isEmpty) continue;
                    try {
                      rules.add(
                        RoutingRule.create(
                          lines[index],
                          action,
                          id: existing?.id,
                        ),
                      );
                    } on BypassTargetError catch (problem) {
                      update(
                        () => error = strings
                            .get('bypass_line_error')
                            .replaceAll('{line}', '${index + 1}')
                            .replaceAll(
                              '{reason}',
                              strings.get(problem.messageKey),
                            ),
                      );
                      return;
                    }
                  }
                  if (rules.isEmpty) {
                    update(() => error = strings.get('invalid_dns_name'));
                    return;
                  }
                  Navigator.pop(context, rules);
                } on Object {
                  update(() => error = strings.get('invalid_dns_name'));
                }
              },
              child: Text(strings.get('save')),
            ),
          ],
        ),
      ),
    );
    if (result == null || onChanged == null) return;
    final rules = [...value.rules];
    if (existing == null) {
      rules.addAll(result);
    } else {
      final index = rules.indexWhere((rule) => rule.id == existing.id);
      if (index >= 0) rules[index] = result.single;
    }
    onChanged!(RoutingSettings(rules: rules, adsEnabled: value.adsEnabled));
  }

  @override
  Widget build(BuildContext context) {
    final padding = grouped
        ? const EdgeInsets.symmetric(horizontal: RowInsets.horizontal)
        : EdgeInsets.zero;
    // Specificity, not position, decides which rule wins, so rows carry no
    // numbering.
    final rows = [for (final rule in value.rules) _row(context, rule, padding)];
    final actions = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Wrap(
          spacing: 12,
          runSpacing: 8,
          children: [
            FilledButton.tonalIcon(
              key: const ValueKey('routing-add'),
              onPressed: onChanged == null ? null : () => _edit(context),
              icon: const Icon(LucideIcons.plus),
              label: Text(strings.get('routing_add')),
            ),
            TextButton(
              key: const ValueKey('routing-batch'),
              onPressed: onChanged == null
                  ? null
                  : () => _edit(context, batch: true),
              child: Text(strings.get('routing_batch')),
            ),
          ],
        ),
        if (onChanged == null)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(strings.get('bypass_unsupported')),
          ),
      ],
    );
    if (grouped) {
      return ContentGroup(
        title: strings.get('routing_rules'),
        hint: strings.get('routing_priority'),
        children: [
          ...rows,
          Padding(padding: RowInsets.row, child: actions),
        ],
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ContentHeading(
          icon: LucideIcons.listFilter,
          title: strings.get('routing_rules'),
          subtitle: strings.get('routing_priority'),
        ),
        const SizedBox(height: 12),
        for (final row in rows) ...[row, const Divider(height: 1)],
        const SizedBox(height: 12),
        actions,
      ],
    );
  }

  Widget _row(BuildContext context, RoutingRule rule, EdgeInsets padding) =>
      ListTile(
        key: ValueKey('routing-rule-${rule.id}'),
        contentPadding: padding,
        title: Text(
          rule.target,
          style: errorIds.contains(rule.id)
              ? TextStyle(color: Theme.of(context).colorScheme.error)
              : null,
        ),
        subtitle: Padding(
          padding: const EdgeInsets.only(top: 4),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              routingActionStatus(strings, rule.action),
              if (errorIds.contains(rule.id)) ...[
                const SizedBox(height: 4),
                Text(
                  errorMessage ?? strings.get('error_generic'),
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
              ] else if (overlapIds.contains(rule.id)) ...[
                const SizedBox(height: 4),
                HintText(strings.get('routing_overlap')),
              ],
            ],
          ),
        ),
        onTap: onChanged == null ? null : () => _edit(context, existing: rule),
        trailing: PopupMenuButton<String>(
          enabled: onChanged != null,
          tooltip: strings.get('edit'),
          onSelected: (action) {
            if (action == 'edit') {
              _edit(context, existing: rule);
            } else {
              onChanged?.call(
                RoutingSettings(
                  rules: [
                    for (final other in value.rules)
                      if (other.id != rule.id) other,
                  ],
                  adsEnabled: value.adsEnabled,
                ),
              );
            }
          },
          itemBuilder: (_) => [
            PopupMenuItem(value: 'edit', child: Text(strings.get('edit'))),
            PopupMenuItem(value: 'delete', child: Text(strings.get('delete'))),
          ],
        ),
      );
}
