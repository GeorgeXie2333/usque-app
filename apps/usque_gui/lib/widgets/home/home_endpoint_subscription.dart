import 'dart:async';

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/app_strings.dart';
import '../../core/usque_motion.dart';
import '../../core/usque_theme.dart';
import '../../models/endpoint_subscriptions.dart';
import '../../state/app_controller.dart';
import '../common.dart';
import '../tv_text_input.dart';
import '../usque_dialog.dart';

/// Compact Home row shown only when the active account uses an endpoint
/// subscription, enabling single-click switching within that subscription.
class EndpointSubscriptionHomeLink extends StatelessWidget {
  const EndpointSubscriptionHomeLink({
    required this.controller,
    this.compact = false,
    super.key,
  });

  final AppController controller;
  final bool compact;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: controller,
    builder: (context, _) {
      final subs = controller.endpointSubscriptions;
      final activeSubId = controller.activeProfile.endpointSubscriptionId;
      if (!subs.supported || activeSubId == null) {
        return const SizedBox.shrink();
      }
      final strings = controller.strings;
      final theme = Theme.of(context);
      final muted = theme.colorScheme.onSurfaceVariant;
      final sub = subs.catalog?.byId(activeSubId);
      final active = sub?.activeEndpoint;
      final subName = sub?.name ?? strings.get('endpoint_subscription_mode');
      final defaultPort = controller.activeProfile.endpointPort;
      final epLabel = active == null
          ? strings.get('sub_active')
          : active.endpoint.primaryLabel(defaultPort);
      final valueText = '$subName · $epLabel';
      final removedUpstream = active?.removedUpstream == true;
      final busy = controller.busy || subs.busy;
      final onTap = busy
          ? null
          : () => _openQuickSwitch(context, sub, defaultPort);

      // Same geometry as the scene row above it: no inset, so the icon,
      // label and value line up with every other Home readout row.
      final valueStyle = compact
          ? theme.textTheme.titleSmall
          : theme.textTheme.bodySmall?.copyWith(fontWeight: FontWeight.w600);
      final value = Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (removedUpstream) ...[
            Icon(
              LucideIcons.triangleAlert,
              size: 15,
              color: statusToneColor(context, StatusTone.warning),
            ),
            const SizedBox(width: 4),
          ],
          Flexible(
            child: Text(
              valueText,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              textAlign: TextAlign.end,
              style: valueStyle,
            ),
          ),
        ],
      );
      final row = ActionRow(
        key: const ValueKey('home-endpoint-subscription'),
        padding: EdgeInsets.zero,
        // Phones: a plain readout row in the list rhythm (row + gap), so the
        // card spacing is unchanged when the row appears. Desktop keeps the
        // scene link's height.
        minHeight: compact ? 0 : 56,
        onTap: onTap,
        child: Align(
          alignment: AlignmentDirectional.centerStart,
          child: compact
              ? ReadoutRow(
                  label: strings.get('endpoint_section'),
                  icon: LucideIcons.globe,
                  value: value,
                )
              : Row(
                  children: [
                    ExcludeSemantics(
                      child: Icon(LucideIcons.globe, size: 16, color: muted),
                    ),
                    const SizedBox(width: UsqueSpace.xs - 2),
                    Text(
                      strings.get('endpoint_section'),
                      style: theme.textTheme.bodySmall?.copyWith(color: muted),
                    ),
                    const SizedBox(width: UsqueSpace.xs - 2),
                    Flexible(child: value),
                  ],
                ),
        ),
      );
      // On phones the readout rows below keep the same rhythm as above.
      return compact
          ? Padding(
              padding: const EdgeInsets.only(bottom: UsqueSpace.m),
              child: row,
            )
          : row;
    },
  );

  Future<void> _openQuickSwitch(
    BuildContext context,
    EndpointSubscription? sub,
    int defaultPort,
  ) async {
    final subs = controller.endpointSubscriptions;
    if (sub == null) {
      controller.openEndpointSubscriptions();
      return;
    }
    final useSheet = MediaQuery.sizeOf(context).width < 600;
    final selected = useSheet
        ? await showModalBottomSheet<String>(
            context: context,
            useSafeArea: true,
            isScrollControlled: true,
            showDragHandle: true,
            sheetAnimationStyle: UsqueMotion.reduced(context)
                ? AnimationStyle.noAnimation
                : null,
            builder: (sheetContext) => _QuickSwitchContent(
              controller: controller,
              sub: sub,
              defaultPort: defaultPort,
              inSheet: true,
              onManage: () {
                Navigator.of(sheetContext).pop();
                controller.openEndpointSubscriptions();
              },
            ),
          )
        : await showDialog<String>(
            context: context,
            builder: (dialogContext) => UsqueDialog(
              icon: LucideIcons.globe,
              title: controller.strings.get('sub_switch_quick'),
              subtitle: sub.name,
              width: 520,
              scrollable: false,
              content: _QuickSwitchContent(
                controller: controller,
                sub: sub,
                defaultPort: defaultPort,
                inSheet: false,
                onManage: () {
                  Navigator.of(dialogContext).pop();
                  controller.openEndpointSubscriptions();
                },
              ),
              actions: [
                TextButton(
                  onPressed: () => Navigator.of(dialogContext).pop(),
                  child: Text(controller.strings.get('close')),
                ),
              ],
            ),
          );
    if (!context.mounted || selected == null) return;
    // Quick switch only moves within the bound subscription. If it was
    // deactivated or replaced while the picker was open, selecting would
    // re-bind without the Zero Trust warning; stop instead.
    if (controller.activeProfile.endpointSubscriptionId != sub.id) return;
    if (selected == sub.activeEndpoint?.endpoint.id) return;
    final endpoint = sub.endpoints.where((e) => e.id == selected).firstOrNull;
    if (endpoint != null &&
        endpoint.kind == SubscriptionEndpointHostKind.domain &&
        !subs.domainEndpointsSupported) {
      ScaffoldMessenger.maybeOf(context)
        ?..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(
            content: Text(controller.strings.get('sub_domain_unsupported')),
          ),
        );
      return;
    }
    final res = await subs.selectEndpoint(
      subscriptionId: sub.id,
      endpointId: selected,
      // Moving within the bound subscription needs no new confirmation. If
      // the binding changed meanwhile, the Engine requires the warning again.
      zeroTrustRiskAcknowledged: false,
    );
    if (context.mounted && res.errorCode != null) {
      ScaffoldMessenger.maybeOf(context)
        ?..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(
            content: Text(
              controller.strings.subscriptionErrorLabel(res.errorCode!),
            ),
          ),
        );
    }
  }
}

class _QuickSwitchContent extends StatefulWidget {
  const _QuickSwitchContent({
    required this.controller,
    required this.sub,
    required this.defaultPort,
    required this.inSheet,
    required this.onManage,
  });

  final AppController controller;
  final EndpointSubscription sub;
  final int defaultPort;
  final bool inSheet;
  final VoidCallback onManage;

  @override
  State<_QuickSwitchContent> createState() => _QuickSwitchContentState();
}

class _QuickSwitchContentState extends State<_QuickSwitchContent> {
  String _query = '';

  @override
  Widget build(BuildContext context) {
    final strings = widget.controller.strings;
    final theme = Theme.of(context);
    final subs = widget.controller.endpointSubscriptions;
    final sub = widget.sub;
    final activeId = sub.activeEndpoint?.endpoint.id;
    final probeResults = subs.probeResultsFor(sub.id);
    final q = _query.trim().toLowerCase();

    final sorted = List<SubscriptionEndpoint>.from(sub.endpoints)
      ..sort((a, b) {
        if (a.id == activeId && b.id != activeId) return -1;
        if (b.id == activeId && a.id != activeId) return 1;
        if (a.pinned != b.pinned) return a.pinned ? -1 : 1;
        return 0;
      });
    final filtered = q.isEmpty
        ? sorted
        : sorted
              .where(
                (ep) =>
                    ep.note.toLowerCase().contains(q) ||
                    ep.host.toLowerCase().contains(q) ||
                    ep
                        .addressLabel(widget.defaultPort)
                        .toLowerCase()
                        .contains(q),
              )
              .toList(growable: false);

    final body = Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (widget.inSheet) ...[
          Padding(
            padding: const EdgeInsets.only(bottom: 12),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  strings.get('sub_switch_quick'),
                  style: theme.textTheme.titleLarge,
                ),
                const SizedBox(height: 2),
                Text(
                  sub.name,
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: theme.colorScheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ),
        ],
        if (sub.endpoints.length > 8) ...[
          tvTextFormField(
            key: const ValueKey('home-sub-quick-search'),
            onChanged: (value) => setState(() => _query = value),
            decoration: InputDecoration(
              prefixIcon: const Icon(LucideIcons.search, size: 18),
              hintText: strings.get('sub_search_hint'),
            ),
          ),
          const SizedBox(height: 12),
        ],
        Flexible(
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (sub.retainedMissingEndpoint case final retained?) ...[
                  ListTile(
                    key: ValueKey('home-sub-retained-${retained.id}'),
                    selected: activeId == retained.id,
                    leading: Icon(
                      LucideIcons.triangleAlert,
                      color: statusToneColor(context, StatusTone.warning),
                    ),
                    title: Text(retained.primaryLabel(widget.defaultPort)),
                    subtitle: Text(strings.get('sub_removed_upstream')),
                    trailing: const Icon(LucideIcons.check, size: 18),
                    onTap: () => Navigator.of(context).pop(retained.id),
                  ),
                ],
                for (final ep in filtered)
                  ListTile(
                    key: ValueKey('home-sub-ep-${ep.id}'),
                    selected: ep.id == activeId,
                    leading: Icon(
                      ep.id == activeId
                          ? LucideIcons.circleDot
                          : ep.pinned
                          ? LucideIcons.star
                          : LucideIcons.globe,
                      size: 18,
                      color: ep.id == activeId
                          ? theme.colorScheme.primary
                          : theme.colorScheme.onSurfaceVariant,
                    ),
                    title: Text(
                      ep.primaryLabel(widget.defaultPort),
                      textDirection: ep.note.isEmpty ? TextDirection.ltr : null,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                    subtitle: ep.note.isNotEmpty
                        ? Text(
                            ep.addressLabel(widget.defaultPort),
                            textDirection: TextDirection.ltr,
                            style: UsqueTheme.readout(context, size: 12),
                          )
                        : null,
                    trailing: _trailingRtt(
                      context,
                      strings,
                      probeResults[ep.id],
                      ep.id == activeId,
                    ),
                    onTap: () => Navigator.of(context).pop(ep.id),
                  ),
              ],
            ),
          ),
        ),
        const Divider(height: 1),
        ListTile(
          key: const ValueKey('home-sub-manage'),
          leading: const Icon(LucideIcons.slidersHorizontal, size: 18),
          title: Text(strings.get('sub_manage')),
          trailing: const Icon(LucideIcons.chevronRightDir, size: 18),
          onTap: widget.onManage,
        ),
      ],
    );

    if (widget.inSheet) {
      return SafeArea(
        top: false,
        child: ConstrainedBox(
          constraints: BoxConstraints(
            maxHeight: MediaQuery.sizeOf(context).height * 0.72,
          ),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(20, 0, 20, 16),
            child: body,
          ),
        ),
      );
    }
    return ConstrainedBox(
      constraints: const BoxConstraints(maxHeight: 420),
      child: body,
    );
  }

  Widget? _trailingRtt(
    BuildContext context,
    AppStrings strings,
    EndpointProbeResult? probe,
    bool active,
  ) {
    if (probe != null &&
        probe.status == EndpointProbeStatus.reachable &&
        probe.rttMs != null) {
      final ms = probe.rttMs!;
      final tone = ms < 120
          ? StatusTone.success
          : ms < 260
          ? StatusTone.warning
          : StatusTone.danger;
      return Text(
        strings.get('sub_rtt_ms').replaceAll('{ms}', '$ms'),
        style: Theme.of(context).textTheme.labelMedium?.copyWith(
          color: statusToneColor(context, tone),
        ),
      );
    }
    return active ? const Icon(LucideIcons.check, size: 18) : null;
  }
}
