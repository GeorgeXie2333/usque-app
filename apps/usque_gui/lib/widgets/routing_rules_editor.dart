import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import '../models/bypass_targets.dart';
import 'common.dart';

class RoutingRulesEditor extends StatelessWidget {
  const RoutingRulesEditor({
    required this.value,
    required this.strings,
    required this.onChanged,
    this.errorIds = const [],
    super.key,
  });
  final RoutingSettings value;
  final AppStrings strings;
  final ValueChanged<RoutingSettings>? onChanged;
  final List<String> errorIds;

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
                            child: Text(value.name.toUpperCase()),
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
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ContentHeading(
          icon: LucideIcons.listFilter,
          title: strings.get('routing_rules'),
          subtitle: strings.get('routing_priority'),
        ),
        const SizedBox(height: 12),
        for (var index = 0; index < value.rules.length; index++) ...[
          ListTile(
            key: ValueKey('routing-rule-${value.rules[index].id}'),
            contentPadding: EdgeInsets.zero,
            title: Text(
              '${index + 1}. ${value.rules[index].target}',
              style: errorIds.contains(value.rules[index].id)
                  ? TextStyle(color: Theme.of(context).colorScheme.error)
                  : null,
            ),
            subtitle: Text(
              '${value.rules[index].action.name.toUpperCase()}${errorIds.contains(value.rules[index].id) ? ' · ${strings.get('error_generic')}' : ''}',
            ),
            onTap: onChanged == null
                ? null
                : () => _edit(context, existing: value.rules[index]),
            trailing: PopupMenuButton<String>(
              enabled: onChanged != null,
              tooltip: strings.get('edit'),
              onSelected: (action) {
                if (action == 'edit') {
                  _edit(context, existing: value.rules[index]);
                } else {
                  onChanged?.call(
                    RoutingSettings(
                      rules: [...value.rules]..removeAt(index),
                      adsEnabled: value.adsEnabled,
                    ),
                  );
                }
              },
              itemBuilder: (_) => [
                PopupMenuItem(value: 'edit', child: Text(strings.get('edit'))),
                PopupMenuItem(
                  value: 'delete',
                  child: Text(strings.get('delete')),
                ),
              ],
            ),
          ),
          const Divider(height: 1),
        ],
        const SizedBox(height: 12),
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
  }
}
