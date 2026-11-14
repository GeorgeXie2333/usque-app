import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/usque_theme.dart';
import '../../models/scenarios.dart';
import '../../screens/profiles_hub_screen.dart' show previewAndUseScenario;
import '../../state/app_controller.dart';
import '../common.dart';

/// The selected scene, sharing the protection readout's hierarchy and alignment.
/// With saved scenes it opens a switcher in place; Profiles stays one entry
/// away, and is the only destination while the library is empty or unread.
class ScenarioHomeLink extends StatelessWidget {
  const ScenarioHomeLink({
    required this.controller,
    this.compact = false,
    super.key,
  });

  final AppController controller;
  final bool compact;

  Future<void> _open(BuildContext context) async {
    final strings = controller.strings;
    final state = controller.scenarios.catalog;
    final scenes = state?.scenarios ?? const <ConnectionScenario>[];
    if (scenes.isEmpty) {
      controller.openProfiles();
      return;
    }
    final theme = Theme.of(context);
    final box = context.findRenderObject()! as RenderBox;
    final overlay =
        Overlay.of(context).context.findRenderObject()! as RenderBox;
    final busy = controller.scenarios.busy;
    Widget entry(IconData icon, String label, {Color? color}) => Row(
      children: [
        Icon(icon, size: 18, color: color),
        const SizedBox(width: 12),
        Flexible(
          child: Text(label, maxLines: 1, overflow: TextOverflow.ellipsis),
        ),
      ],
    );
    final picked = await showMenu<Object>(
      context: context,
      position: RelativeRect.fromRect(
        box.localToGlobal(Offset.zero, ancestor: overlay) & box.size,
        Offset.zero & overlay.size,
      ),
      items: [
        for (final scene in scenes)
          PopupMenuItem<Object>(
            key: ValueKey('home-scene-${scene.id}'),
            value: scene,
            // The selected scene is marked, never switched to itself; one
            // that cannot be used is explained on Profiles.
            enabled:
                !busy &&
                scene.id != state!.selectedId &&
                !state.invalidIds.contains(scene.id) &&
                (scene.boundAccountId == null ||
                    controller.profiles.any(
                      (a) => a.id == scene.boundAccountId,
                    )),
            child: scene.id == state!.selectedId
                ? entry(
                    LucideIcons.circleDot,
                    scene.name,
                    color: theme.colorScheme.primary,
                  )
                : entry(LucideIcons.circle, scene.name),
          ),
        const PopupMenuDivider(),
        PopupMenuItem<Object>(
          key: const ValueKey('home-scene-manage'),
          value: true,
          child: entry(LucideIcons.layers3, strings.get('profiles')),
        ),
      ],
    );
    if (!context.mounted) return;
    if (picked is ConnectionScenario) {
      await previewAndUseScenario(context, controller, picked, direct: true);
    } else if (picked == true) {
      controller.openProfiles();
    }
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: controller.scenarios,
    builder: (context, _) {
      if (!controller.scenarios.supported) {
        return const SizedBox.shrink();
      }
      final strings = controller.strings;
      final theme = Theme.of(context);
      final label = strings.get('scenario_current');
      final name =
          controller.scenarios.catalog?.selected?.name ??
          strings.get('scenario_custom');
      final muted = theme.colorScheme.onSurfaceVariant;
      return ActionRow(
        key: const ValueKey('home-scene-link'),
        onTap: () => _open(context),
        padding: EdgeInsets.zero,
        minHeight: compact ? 48 : 56,
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
                      child: Icon(LucideIcons.layers3, size: 16, color: muted),
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
      );
    },
  );
}
