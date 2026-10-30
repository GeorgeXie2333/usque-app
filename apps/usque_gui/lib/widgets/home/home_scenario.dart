import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/usque_theme.dart';
import '../../models/scenarios.dart';
import '../../state/app_controller.dart';
import '../common.dart';

/// The selected scene, sharing the protection readout's hierarchy and alignment.
class ScenarioHomeLink extends StatelessWidget {
  const ScenarioHomeLink({
    required this.controller,
    this.compact = false,
    super.key,
  });

  final AppController controller;
  final bool compact;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: controller.scenarios,
    builder: (context, _) {
      if (!controller.scenarios.supported) return const SizedBox.shrink();
      final strings = controller.strings;
      final theme = Theme.of(context);
      final label = strings.get('scenario_current');
      final name =
          controller.scenarios.catalog?.selected?.name ??
          strings.get('scenario_custom');
      final muted = theme.colorScheme.onSurfaceVariant;
      return Padding(
        padding: EdgeInsets.only(bottom: compact ? UsqueSpace.xs : 0),
        child: ActionRow(
          key: const ValueKey('home-scene-link'),
          onTap: () => controller.openProfiles(ProfilesTab.scenes),
          padding: EdgeInsets.zero,
          child: Align(
            alignment: AlignmentDirectional.centerStart,
            child: compact
                ? ReadoutRow(
                    label: label,
                    icon: LucideIcons.layers3,
                    value: Text(
                      name,
                      textAlign: TextAlign.end,
                      style: theme.textTheme.titleSmall,
                    ),
                  )
                : Row(
                    children: [
                      ExcludeSemantics(
                        child: Icon(
                          LucideIcons.layers3,
                          size: 16,
                          color: muted,
                        ),
                      ),
                      const SizedBox(width: UsqueSpace.xs - 2),
                      Flexible(
                        child: Wrap(
                          spacing: UsqueSpace.xs - 2,
                          children: [
                            Text(
                              label,
                              style: theme.textTheme.bodySmall?.copyWith(
                                color: muted,
                              ),
                            ),
                            Text(
                              name,
                              style: theme.textTheme.bodySmall?.copyWith(
                                fontWeight: FontWeight.w600,
                              ),
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
          ),
        ),
      );
    },
  );
}
