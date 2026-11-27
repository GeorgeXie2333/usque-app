import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_motion.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../models/endpoint_subscriptions.dart';
import '../state/app_controller.dart';
import 'common.dart';
import 'endpoint_subscription_dialog.dart';
import 'tv_text_input.dart';
import 'usque_dialog.dart';
import 'zero_trust_endpoint_warning.dart';

enum _EndpointFilter { all, ipv4, ipv6, domain, pinned }

enum _EndpointSort { defaultOrder, lowestLatency }

/// Primary "Profiles → Subscriptions" management and fast endpoint-switching
/// view, constrained to the shared 880 px workspace column.
class EndpointSubscriptionsView extends StatefulWidget {
  const EndpointSubscriptionsView({
    required this.controller,
    required this.onAddSubscription,
    super.key,
  });

  final AppController controller;
  final VoidCallback onAddSubscription;

  @override
  State<EndpointSubscriptionsView> createState() =>
      _EndpointSubscriptionsViewState();
}

class _EndpointSubscriptionsViewState extends State<EndpointSubscriptionsView> {
  String? _focusedSubscriptionId;
  String _searchQuery = '';
  _EndpointFilter _filter = _EndpointFilter.all;
  _EndpointSort _sort = _EndpointSort.defaultOrder;
  final TextEditingController _searchController = TextEditingController();

  AppController get app => widget.controller;

  @override
  void dispose() {
    _searchController.dispose();
    super.dispose();
  }

  EndpointSubscription? _resolveFocused(
    EndpointSubscriptionCatalog catalog,
    EndpointSubscriptionTier? activeTier,
  ) {
    if (catalog.subscriptions.isEmpty) return null;
    if (_focusedSubscriptionId case final current?) {
      if (catalog.byId(current) case final found?) return found;
    }
    if (catalog.activeSubscriptionId case final activeId?) {
      if (catalog.byId(activeId) case final found?) {
        _focusedSubscriptionId = found.id;
        return found;
      }
    }
    if (activeTier != null) {
      final compatible = catalog.subscriptions
          .where((s) => s.accountTier == activeTier)
          .firstOrNull;
      if (compatible != null) {
        _focusedSubscriptionId = compatible.id;
        return compatible;
      }
    }
    _focusedSubscriptionId = catalog.subscriptions.first.id;
    return catalog.subscriptions.first;
  }

  String _relativeUpdated(AppStrings strings, int? unixMs) {
    if (unixMs == null || unixMs <= 0) {
      return strings.get('sub_last_updated_never');
    }
    final diff = DateTime.now().difference(
      DateTime.fromMillisecondsSinceEpoch(unixMs),
    );
    final String time;
    if (diff.inMinutes < 1) {
      time = '<1m';
    } else if (diff.inHours < 1) {
      time = '${diff.inMinutes}m';
    } else if (diff.inDays < 1) {
      time = '${diff.inHours}h';
    } else {
      time = '${diff.inDays}d';
    }
    return strings.get('sub_last_updated_ago').replaceAll('{time}', time);
  }

  String _refreshLabel(AppStrings strings, SubscriptionRefreshInterval value) =>
      switch (value) {
        SubscriptionRefreshInterval.off => strings.get('sub_refresh_off'),
        SubscriptionRefreshInterval.daily => strings.get('sub_refresh_daily'),
        SubscriptionRefreshInterval.weekly => strings.get('sub_refresh_weekly'),
      };

  Future<void> _chooseSubscriptionSheet(
    EndpointSubscriptionCatalog catalog,
    EndpointSubscription focused,
    EndpointSubscriptionTier? activeTier,
  ) async {
    final strings = app.strings;
    final picked = await showModalBottomSheet<String>(
      context: context,
      useSafeArea: true,
      isScrollControlled: true,
      showDragHandle: true,
      sheetAnimationStyle: UsqueMotion.reduced(context)
          ? AnimationStyle.noAnimation
          : null,
      builder: (sheetContext) => SafeArea(
        top: false,
        child: SingleChildScrollView(
          padding: const EdgeInsets.fromLTRB(24, 0, 24, 24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                strings.get('sub_choose'),
                style: Theme.of(sheetContext).textTheme.titleLarge,
              ),
              const SizedBox(height: 12),
              for (final sub in catalog.subscriptions)
                ListTile(
                  key: ValueKey('subscription-option-${sub.id}'),
                  selected: sub.id == focused.id,
                  leading: Icon(
                    sub.id == catalog.activeSubscriptionId
                        ? LucideIcons.circleDot
                        : LucideIcons.globe,
                    size: 18,
                    color: sub.id == catalog.activeSubscriptionId
                        ? Theme.of(sheetContext).colorScheme.primary
                        : null,
                  ),
                  title: Text(sub.name),
                  subtitle: Text(
                    '${strings.subscriptionTierLabel(sub.accountTier)} · ${strings.get('sub_count').replaceAll('{count}', '${sub.endpoints.length}')}',
                  ),
                  trailing: activeTier != null && sub.accountTier != activeTier
                      ? Icon(
                          LucideIcons.lock,
                          size: 16,
                          color: Theme.of(sheetContext).disabledColor,
                        )
                      : sub.id == focused.id
                      ? const Icon(LucideIcons.check, size: 18)
                      : null,
                  onTap: () => Navigator.of(sheetContext).pop(sub.id),
                ),
            ],
          ),
        ),
      ),
    );
    if (mounted && picked != null) {
      setState(() => _focusedSubscriptionId = picked);
    }
  }

  Future<bool> _confirmSceneDriftIfNeeded({bool deactivating = false}) async {
    final catalog = app.scenarios.catalog;
    final selected = catalog?.selected;
    if (selected == null || catalog?.modified == true) return true;
    final strings = app.strings;
    final live = app.snapshot.isConnected || app.snapshot.isTransitional;
    final bodyKey = deactivating
        ? (live
              ? 'sub_scene_drift_deactivate_reconnect'
              : 'sub_scene_drift_deactivate')
        : (live ? 'sub_scene_drift_reconnect' : 'sub_scene_drift_body');
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => UsqueDialog(
        icon: LucideIcons.triangleAlert,
        title: strings.get('sub_scene_drift_title'),
        content: Text(strings.get(bodyKey).replaceAll('{name}', selected.name)),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.of(dialogContext).pop(true),
            child: Text(strings.get('continue')),
          ),
        ],
      ),
    );
    return confirmed == true;
  }

  Future<void> _activateEndpoint(
    EndpointSubscription sub,
    SubscriptionEndpoint ep,
  ) async {
    final strings = app.strings;
    final subs = app.endpointSubscriptions;
    if (ep.kind == SubscriptionEndpointHostKind.domain &&
        !subs.domainEndpointsSupported) {
      ScaffoldMessenger.maybeOf(context)
        ?..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(content: Text(strings.get('sub_domain_unsupported'))),
        );
      return;
    }
    final activeSubId = app.activeProfile.endpointSubscriptionId;
    final switchingSubscription = activeSubId != sub.id;
    var ztAck =
        sub.accountTier == EndpointSubscriptionTier.zeroTrust &&
        !switchingSubscription;
    if (switchingSubscription) {
      if (sub.accountTier == EndpointSubscriptionTier.zeroTrust) {
        final accepted = await showDialog<bool>(
          context: context,
          barrierDismissible: false,
          builder: (_) => ZeroTrustEndpointWarning(strings: strings),
        );
        if (!mounted || accepted != true) return;
        ztAck = true;
      }
      // Any cross-subscription switch also modifies an unmodified scene.
      final confirmed = await _confirmSceneDriftIfNeeded();
      if (!mounted || !confirmed) return;
      // The binding may have changed while a dialog was open.
      if (app.activeProfile.endpointSubscriptionId != activeSubId) return;
    }
    final result = await subs.selectEndpoint(
      subscriptionId: sub.id,
      endpointId: ep.id,
      zeroTrustRiskAcknowledged: ztAck,
    );
    if (!mounted) return;
    if (result.errorCode case final code?) {
      ScaffoldMessenger.maybeOf(context)
        ?..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(content: Text(strings.subscriptionErrorLabel(code))),
        );
    }
  }

  Future<void> _deactivate(EndpointSubscription sub) async {
    final confirmed = await _confirmSceneDriftIfNeeded(deactivating: true);
    if (!mounted || !confirmed) return;
    final result = await app.endpointSubscriptions.deactivate();
    if (!mounted) return;
    if (result.errorCode case final code?) {
      ScaffoldMessenger.maybeOf(context)
        ?..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(content: Text(app.strings.subscriptionErrorLabel(code))),
        );
    }
  }

  Future<void> _delete(EndpointSubscription sub) async {
    final strings = app.strings;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => UsqueDialog(
        icon: LucideIcons.trash2,
        title: strings.get('sub_delete_title'),
        subtitle: sub.name,
        danger: true,
        content: Text(
          strings.get('sub_delete_body').replaceAll('{name}', sub.name),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton(
            style: dangerButtonStyle(dialogContext),
            onPressed: () => Navigator.of(dialogContext).pop(true),
            child: Text(strings.get('delete')),
          ),
        ],
      ),
    );
    if (!mounted || confirmed != true) return;
    final res = await app.endpointSubscriptions.deleteSubscription(
      id: sub.id,
      expectedRevision: sub.revision,
    );
    if (mounted && res.errorCode != null) {
      ScaffoldMessenger.maybeOf(context)
        ?..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(
            content: Text(strings.subscriptionErrorLabel(res.errorCode!)),
          ),
        );
    }
  }

  Future<void> _refreshOne(EndpointSubscription sub) async {
    final res = await app.endpointSubscriptions.refreshSubscription(sub.id);
    // A refresh the user stopped is not an error.
    if (mounted &&
        res.errorCode != null &&
        res.errorCode != 'SUBSCRIPTION_CANCELLED') {
      ScaffoldMessenger.maybeOf(context)
        ?..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(
            content: Text(app.strings.subscriptionErrorLabel(res.errorCode!)),
          ),
        );
    }
  }

  Future<void> _startProbe(
    EndpointSubscription sub, {
    List<String>? endpointIds,
  }) async {
    final res = await app.endpointSubscriptions.startProbe(
      sub.id,
      endpointIds: endpointIds,
    );
    if (mounted && res.errorCode != null) {
      ScaffoldMessenger.maybeOf(context)
        ?..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(
            content: Text(app.strings.subscriptionErrorLabel(res.errorCode!)),
          ),
        );
    }
  }

  List<SubscriptionEndpoint> _filteredEndpoints(
    EndpointSubscription sub,
    Map<String, EndpointProbeResult> probeResults,
  ) {
    final defaultPort = app.activeProfile.endpointPort;
    final query = _searchQuery.trim().toLowerCase();
    final items = sub.endpoints.where((ep) {
      switch (_filter) {
        case _EndpointFilter.all:
          break;
        case _EndpointFilter.ipv4:
          if (ep.kind != SubscriptionEndpointHostKind.ipv4) return false;
        case _EndpointFilter.ipv6:
          if (ep.kind != SubscriptionEndpointHostKind.ipv6) return false;
        case _EndpointFilter.domain:
          if (ep.kind != SubscriptionEndpointHostKind.domain) return false;
        case _EndpointFilter.pinned:
          if (!ep.pinned) return false;
      }
      if (query.isEmpty) return true;
      return ep.note.toLowerCase().contains(query) ||
          ep.host.toLowerCase().contains(query) ||
          ep.addressLabel(defaultPort).toLowerCase().contains(query);
    }).toList();

    items.sort((a, b) {
      if (_sort == _EndpointSort.lowestLatency) {
        final ra = probeResults[a.id];
        final rb = probeResults[b.id];
        final aReachable =
            ra?.status == EndpointProbeStatus.reachable && ra?.rttMs != null;
        final bReachable =
            rb?.status == EndpointProbeStatus.reachable && rb?.rttMs != null;
        if (aReachable != bReachable) return aReachable ? -1 : 1;
        if (aReachable && bReachable) {
          final cmp = ra!.rttMs!.compareTo(rb!.rttMs!);
          if (cmp != 0) return cmp;
        }
      }
      if (a.pinned != b.pinned) return a.pinned ? -1 : 1;
      return 0;
    });
    return items;
  }

  @override
  Widget build(BuildContext context) {
    final strings = app.strings;
    final theme = Theme.of(context);
    final subs = app.endpointSubscriptions;
    final catalog = subs.catalog;

    if (catalog == null) {
      if (subs.errorCode != null) {
        return Center(
          child: TextButton.icon(
            key: const ValueKey('subscriptions-retry'),
            onPressed: () => subs.refresh(force: true),
            icon: const Icon(LucideIcons.refreshCw),
            label: Text(strings.get('retry')),
          ),
        );
      }
      return const Padding(
        padding: EdgeInsets.symmetric(vertical: 32),
        child: Center(child: CircularProgressIndicator()),
      );
    }

    if (catalog.subscriptions.isEmpty) {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 40, horizontal: 16),
        child: Column(
          children: [
            Icon(
              LucideIcons.globe,
              size: 40,
              color: theme.colorScheme.onSurfaceVariant,
            ),
            const SizedBox(height: 16),
            Text(
              strings.get('sub_empty'),
              textAlign: TextAlign.center,
              style: theme.textTheme.titleLarge,
            ),
            const SizedBox(height: 8),
            Text(
              strings.get('sub_empty_hint'),
              textAlign: TextAlign.center,
              style: theme.textTheme.bodyMedium?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
              ),
            ),
            const SizedBox(height: 24),
            FilledButton.icon(
              key: const ValueKey('subscription-add'),
              onPressed: subs.busy ? null : widget.onAddSubscription,
              icon: const Icon(LucideIcons.plus),
              label: Text(strings.get('sub_add')),
            ),
          ],
        ),
      );
    }

    final activeTier =
        catalog.activeAccountTier ?? app.activeAccountSubscriptionTier;
    final focused = _resolveFocused(catalog, activeTier)!;
    final activeSubId = app.activeProfile.endpointSubscriptionId;
    final isFocusedActive = activeSubId == focused.id;
    final activeEndpointId = isFocusedActive
        ? focused.activeEndpoint?.endpoint.id
        : focused.selectedEndpointId;
    final tierCompatible =
        activeTier != null && focused.accountTier == activeTier;
    final canActivate = tierCompatible && !subs.busy && !app.busy;

    final connectedDesktopVpn =
        app.snapshot.isConnected &&
        app.activeProfile.mode == OperatingMode.vpn &&
        (Theme.of(context).platform == TargetPlatform.windows ||
            Theme.of(context).platform == TargetPlatform.linux ||
            Theme.of(context).platform == TargetPlatform.macOS);
    final connectedTunProbeBlocked =
        connectedDesktopVpn && !subs.connectedTunProbeSupported;

    final refreshing = subs.isRefreshing(focused.id);
    final probingThisSub =
        subs.probeRunning && subs.runningProbeSubscriptionId == focused.id;
    final probeResults = subs.probeResultsFor(focused.id);
    final probeProgress = subs.activeProbeProgress;
    final endpoints = _filteredEndpoints(focused, probeResults);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _buildPicker(context, catalog, focused, activeSubId, activeTier),
        const SizedBox(height: 8),
        SizedBox(
          height: 4,
          child: refreshing || subs.busy
              ? const LinearProgressIndicator()
              : null,
        ),
        const SizedBox(height: 12),
        _buildSubHeader(
          context,
          focused: focused,
          refreshing: refreshing,
          probingThisSub: probingThisSub,
          probeProgress: probeProgress,
          canActivate: canActivate,
          isFocusedActive: isFocusedActive,
          connectedTunProbeBlocked: connectedTunProbeBlocked,
        ),
        if (connectedTunProbeBlocked) ...[
          const SizedBox(height: 8),
          HintText(strings.get('sub_probe_tun_unsupported')),
        ],
        if (subs.lastProbeErrorCode case final probeErr?) ...[
          const SizedBox(height: 10),
          WarningBanner(
            title: null,
            message: strings.subscriptionErrorLabel(probeErr),
          ),
        ],
        if (activeTier == null) ...[
          const SizedBox(height: 12),
          WarningBanner(
            key: const ValueKey('subscription-verify-warning'),
            title: null,
            message: strings.get('sub_verify_account'),
          ),
        ] else if (!tierCompatible) ...[
          const SizedBox(height: 12),
          WarningBanner(
            key: const ValueKey('subscription-tier-warning'),
            title: null,
            message: strings
                .get('sub_tier_mismatch')
                .replaceAll(
                  '{tier}',
                  strings.subscriptionTierLabel(focused.accountTier),
                ),
          ),
        ],
        const SizedBox(height: 16),
        _buildToolbar(context, focused),
        const SizedBox(height: 12),
        ContentList(
          children: [
            if (focused.retainedMissingEndpoint case final retained?)
              _buildEndpointRow(
                context,
                sub: focused,
                ep: retained,
                isActive: isFocusedActive && activeEndpointId == retained.id,
                isSelectedInSub: focused.selectedEndpointId == retained.id,
                removedUpstream: true,
                canActivate: canActivate,
                probe: probeResults[retained.id],
                probingThisSub: probingThisSub,
                connectedTunProbeBlocked: connectedTunProbeBlocked,
              ),
            for (final ep in endpoints)
              _buildEndpointRow(
                context,
                sub: focused,
                ep: ep,
                isActive: isFocusedActive && activeEndpointId == ep.id,
                isSelectedInSub: focused.selectedEndpointId == ep.id,
                removedUpstream: false,
                canActivate: canActivate,
                probe: probeResults[ep.id],
                probingThisSub: probingThisSub,
                connectedTunProbeBlocked: connectedTunProbeBlocked,
              ),
            if (isFocusedActive)
              ActionRow(
                key: const ValueKey('subscription-deactivate'),
                padding: const EdgeInsets.symmetric(
                  horizontal: 14,
                  vertical: 16,
                ),
                onTap: subs.busy ? null : () => _deactivate(focused),
                child: Row(
                  children: [
                    Icon(
                      LucideIcons.rotateCcw,
                      size: 20,
                      color: theme.colorScheme.onSurfaceVariant,
                    ),
                    const SizedBox(width: 13),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            strings.get('sub_deactivate'),
                            style: theme.textTheme.titleMedium,
                          ),
                          const SizedBox(height: 4),
                          HintText(strings.get('sub_deactivate_hint')),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
          ],
        ),
      ],
    );
  }

  Widget _buildPicker(
    BuildContext context,
    EndpointSubscriptionCatalog catalog,
    EndpointSubscription focused,
    String? activeSubId,
    EndpointSubscriptionTier? activeTier,
  ) {
    final strings = app.strings;
    final theme = Theme.of(context);
    return LayoutBuilder(
      builder: (context, constraints) {
        if (constraints.maxWidth < 600) {
          return ActionRow(
            key: const ValueKey('subscription-picker-compact'),
            onTap: () => _chooseSubscriptionSheet(catalog, focused, activeTier),
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
            child: Row(
              children: [
                Icon(
                  focused.id == activeSubId
                      ? LucideIcons.circleDot
                      : LucideIcons.globe,
                  size: 18,
                  color: focused.id == activeSubId
                      ? theme.colorScheme.primary
                      : theme.colorScheme.onSurfaceVariant,
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        focused.name,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.titleSmall,
                      ),
                      Text(
                        strings.subscriptionTierLabel(focused.accountTier),
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: theme.colorScheme.onSurfaceVariant,
                        ),
                      ),
                    ],
                  ),
                ),
                const Icon(LucideIcons.chevronDown, size: 18),
              ],
            ),
          );
        }
        return Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            for (final sub in catalog.subscriptions)
              _subscriptionChip(
                context,
                sub: sub,
                focused: sub.id == focused.id,
                active: sub.id == activeSubId,
                compatible: activeTier != null && sub.accountTier == activeTier,
              ),
          ],
        );
      },
    );
  }

  Widget _subscriptionChip(
    BuildContext context, {
    required EndpointSubscription sub,
    required bool focused,
    required bool active,
    required bool compatible,
  }) {
    final theme = Theme.of(context);
    final muted = theme.colorScheme.onSurfaceVariant;
    return ChoiceChip(
      key: ValueKey('subscription-chip-${sub.id}'),
      showCheckmark: false,
      selected: focused,
      onSelected: (_) => setState(() => _focusedSubscriptionId = sub.id),
      avatar: active
          ? Icon(
              LucideIcons.circleDot,
              size: 16,
              color: theme.colorScheme.primary,
            )
          : !compatible
          ? Icon(LucideIcons.lock, size: 14, color: muted)
          : null,
      label: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Flexible(
            child: Text(
              sub.name,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: compatible ? null : TextStyle(color: muted),
            ),
          ),
          const SizedBox(width: 6),
          Text(
            sub.accountTier.shortBadge,
            textDirection: TextDirection.ltr,
            style: theme.textTheme.labelSmall?.copyWith(color: muted),
          ),
        ],
      ),
    );
  }

  Widget _buildSubHeader(
    BuildContext context, {
    required EndpointSubscription focused,
    required bool refreshing,
    required bool probingThisSub,
    required EndpointProbeProgress? probeProgress,
    required bool canActivate,
    required bool isFocusedActive,
    required bool connectedTunProbeBlocked,
  }) {
    final strings = app.strings;
    final theme = Theme.of(context);
    final subs = app.endpointSubscriptions;
    final probeEnabled =
        subs.probeSupported &&
        !connectedTunProbeBlocked &&
        (canActivate || probingThisSub);
    final probeCompleted = probeProgress?.subscriptionId == focused.id
        ? probeProgress!.completed
        : 0;
    final probeTotal = probeProgress?.subscriptionId == focused.id
        ? probeProgress!.total
        : focused.endpoints.length;

    return Wrap(
      alignment: WrapAlignment.spaceBetween,
      crossAxisAlignment: WrapCrossAlignment.center,
      spacing: 12,
      runSpacing: 12,
      children: [
        Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Wrap(
              spacing: 10,
              runSpacing: 6,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                Text(focused.name, style: theme.textTheme.titleLarge),
                StatusPill(
                  label: strings.subscriptionTierLabel(focused.accountTier),
                  tone: canActivate ? StatusTone.brand : StatusTone.neutral,
                  showIndicator: false,
                ),
                if (isFocusedActive)
                  InlineStatus(
                    label: strings.get('sub_active'),
                    tone: StatusTone.brand,
                  ),
              ],
            ),
            const SizedBox(height: 6),
            Wrap(
              spacing: 14,
              runSpacing: 4,
              children: [
                HintText(
                  strings
                      .get('sub_count')
                      .replaceAll('{count}', '${focused.endpoints.length}'),
                ),
                HintText(
                  _relativeUpdated(strings, focused.lastRefreshedAtUnixMs),
                ),
                HintText(_refreshLabel(strings, focused.autoRefresh)),
                if (focused.skippedLineCount > 0)
                  HintText(
                    strings
                        .get('sub_skipped_lines')
                        .replaceAll('{count}', '${focused.skippedLineCount}'),
                  ),
              ],
            ),
            if (focused.lastRefreshError case final refreshErr?) ...[
              const SizedBox(height: 6),
              InlineStatus(
                label: strings.subscriptionErrorLabel(refreshErr),
                tone: StatusTone.warning,
              ),
            ],
          ],
        ),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            if (refreshing)
              OutlinedButton.icon(
                key: const ValueKey('subscription-refresh-cancel'),
                onPressed: () => subs.cancelRefresh(focused.id),
                icon: const Icon(LucideIcons.x, size: 16),
                label: Text(strings.get('sub_cancel_refresh')),
              )
            else
              OutlinedButton.icon(
                key: const ValueKey('subscription-refresh'),
                onPressed: subs.busy ? null : () => _refreshOne(focused),
                icon: const Icon(LucideIcons.refreshCw, size: 16),
                label: Text(strings.get('sub_refresh_now')),
              ),
            if (probingThisSub)
              FilledButton.tonalIcon(
                key: const ValueKey('subscription-probe'),
                onPressed: subs.cancelProbe,
                icon: const Icon(LucideIcons.square, size: 16),
                label: Text(
                  strings
                      .get('sub_probe_stop')
                      .replaceAll('{completed}', '$probeCompleted')
                      .replaceAll('{total}', '$probeTotal'),
                ),
              )
            else
              OutlinedButton.icon(
                key: const ValueKey('subscription-probe'),
                onPressed: probeEnabled && !subs.probeRunning
                    ? () => _startProbe(focused)
                    : null,
                icon: const Icon(LucideIcons.activity, size: 16),
                label: Text(strings.get('sub_probe_all')),
              ),
            PopupMenuButton<String>(
              key: const ValueKey('subscription-more'),
              enabled: !subs.busy,
              onSelected: (value) {
                switch (value) {
                  case 'edit':
                    unawaited(
                      showEndpointSubscriptionDialog(
                        context,
                        controller: app,
                        existing: focused,
                      ),
                    );
                  case 'copy':
                    final messenger = ScaffoldMessenger.maybeOf(context);
                    unawaited(
                      Clipboard.setData(ClipboardData(text: focused.url)).then((
                        _,
                      ) {
                        if (!mounted) return;
                        messenger
                          ?..hideCurrentSnackBar()
                          ..showSnackBar(
                            SnackBar(
                              content: Text(strings.get('sub_url_copied')),
                            ),
                          );
                      }),
                    );
                  case 'deactivate':
                    unawaited(_deactivate(focused));
                  case 'delete':
                    unawaited(_delete(focused));
                }
              },
              itemBuilder: (_) => [
                PopupMenuItem(
                  key: const ValueKey('subscription-edit'),
                  value: 'edit',
                  child: Row(
                    children: [
                      const Icon(LucideIcons.pencil, size: 18),
                      const SizedBox(width: 12),
                      Text(strings.get('sub_edit')),
                    ],
                  ),
                ),
                PopupMenuItem(
                  value: 'copy',
                  child: Row(
                    children: [
                      const Icon(LucideIcons.copy, size: 18),
                      const SizedBox(width: 12),
                      Text(strings.get('sub_copy_url')),
                    ],
                  ),
                ),
                if (isFocusedActive)
                  PopupMenuItem(
                    value: 'deactivate',
                    child: Row(
                      children: [
                        const Icon(LucideIcons.rotateCcw, size: 18),
                        const SizedBox(width: 12),
                        Text(strings.get('sub_deactivate')),
                      ],
                    ),
                  ),
                const PopupMenuDivider(),
                PopupMenuItem(
                  key: const ValueKey('subscription-delete'),
                  value: 'delete',
                  child: Row(
                    children: [
                      Icon(
                        LucideIcons.trash2,
                        size: 18,
                        color: statusToneColor(context, StatusTone.danger),
                      ),
                      const SizedBox(width: 12),
                      Text(
                        strings.get('sub_delete'),
                        style: TextStyle(
                          color: statusToneColor(context, StatusTone.danger),
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ],
        ),
      ],
    );
  }

  Widget _buildToolbar(BuildContext context, EndpointSubscription focused) {
    final strings = app.strings;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        tvTextFormField(
          key: const ValueKey('subscription-search'),
          controller: _searchController,
          onChanged: (value) => setState(() => _searchQuery = value),
          decoration: InputDecoration(
            prefixIcon: const Icon(LucideIcons.search, size: 18),
            hintText: strings.get('sub_search_hint'),
            suffixIcon: _searchQuery.isEmpty
                ? null
                : IconButton(
                    icon: const Icon(LucideIcons.x, size: 16),
                    onPressed: () {
                      _searchController.clear();
                      setState(() => _searchQuery = '');
                    },
                  ),
          ),
        ),
        const SizedBox(height: 10),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          crossAxisAlignment: WrapCrossAlignment.center,
          alignment: WrapAlignment.spaceBetween,
          children: [
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                for (final item in [
                  (_EndpointFilter.all, strings.get('sub_filter_all')),
                  (_EndpointFilter.ipv4, 'IPv4'),
                  (_EndpointFilter.ipv6, 'IPv6'),
                  (_EndpointFilter.domain, strings.get('sub_filter_domain')),
                  (_EndpointFilter.pinned, strings.get('sub_filter_pinned')),
                ])
                  ChoiceChip(
                    key: ValueKey('subscription-filter-${item.$1.name}'),
                    showCheckmark: false,
                    selected: _filter == item.$1,
                    label: Text(item.$2),
                    onSelected: (_) => setState(() => _filter = item.$1),
                  ),
              ],
            ),
            ActionChip(
              key: const ValueKey('subscription-sort'),
              avatar: const Icon(LucideIcons.arrowUpDown, size: 15),
              label: Text(
                strings.get(
                  _sort == _EndpointSort.defaultOrder
                      ? 'sub_sort_default'
                      : 'sub_sort_latency',
                ),
              ),
              onPressed: () => setState(
                () => _sort = _sort == _EndpointSort.defaultOrder
                    ? _EndpointSort.lowestLatency
                    : _EndpointSort.defaultOrder,
              ),
            ),
          ],
        ),
      ],
    );
  }

  Widget _buildEndpointRow(
    BuildContext context, {
    required EndpointSubscription sub,
    required SubscriptionEndpoint ep,
    required bool isActive,
    required bool isSelectedInSub,
    required bool removedUpstream,
    required bool canActivate,
    required EndpointProbeResult? probe,
    required bool probingThisSub,
    required bool connectedTunProbeBlocked,
  }) {
    final strings = app.strings;
    final theme = Theme.of(context);
    final tokens = UsqueTokens.of(context);
    final subs = app.endpointSubscriptions;
    final defaultPort = app.activeProfile.endpointPort;
    final kindLabel = switch (ep.kind) {
      SubscriptionEndpointHostKind.ipv4 => 'IPv4',
      SubscriptionEndpointHostKind.ipv6 => 'IPv6',
      SubscriptionEndpointHostKind.domain => strings.get('sub_filter_domain'),
    };
    final rowKey = removedUpstream
        ? const ValueKey('sub-retained-endpoint')
        : ValueKey('sub-endpoint-${ep.id}');

    final showProbeStatus = probe != null || probingThisSub;
    // The activation row and its probe/pin buttons are siblings: TalkBack and
    // D-pad focus reach each control separately instead of a button nested in
    // a tappable row. Probe status wraps with the address line, so 200% text
    // on a narrow phone never pushes the trailing buttons off screen.
    final activation = ActionRow(
      key: rowKey,
      padding: const EdgeInsetsDirectional.fromSTEB(14, 14, 6, 14),
      onTap: canActivate && !removedUpstream
          ? () => _activateEndpoint(sub, ep)
          : null,
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: [
          ExcludeSemantics(
            child: Icon(
              ep.kind == SubscriptionEndpointHostKind.domain
                  ? LucideIcons.globe
                  : LucideIcons.network,
              size: 20,
              color: isActive || isSelectedInSub
                  ? theme.colorScheme.primary
                  : theme.colorScheme.onSurfaceVariant,
            ),
          ),
          const SizedBox(width: 13),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Wrap(
                  spacing: 10,
                  runSpacing: 4,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    Text(
                      ep.primaryLabel(defaultPort),
                      textDirection: ep.note.isEmpty ? TextDirection.ltr : null,
                      style: theme.textTheme.titleMedium,
                    ),
                    if (isActive)
                      InlineStatus(
                        label: strings.get('sub_active'),
                        tone: StatusTone.brand,
                      ),
                    if (removedUpstream)
                      InlineStatus(
                        label: strings.get('sub_removed_upstream'),
                        tone: StatusTone.warning,
                      ),
                  ],
                ),
                const SizedBox(height: 4),
                Wrap(
                  spacing: 10,
                  runSpacing: 4,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    Text(
                      ep.addressLabel(defaultPort),
                      textDirection: TextDirection.ltr,
                      style: UsqueTheme.readout(
                        context,
                        size: 12,
                      ).copyWith(color: theme.colorScheme.onSurfaceVariant),
                    ),
                    Text(
                      kindLabel,
                      style: theme.textTheme.labelSmall?.copyWith(
                        color: theme.colorScheme.onSurfaceVariant,
                      ),
                    ),
                    if (showProbeStatus)
                      _buildProbeStatus(
                        context,
                        strings,
                        probe: probe,
                        probingThisSub: probingThisSub,
                      ),
                  ],
                ),
                if (removedUpstream) ...[
                  const SizedBox(height: 4),
                  HintText(strings.get('sub_removed_upstream_hint')),
                ],
              ],
            ),
          ),
        ],
      ),
    );
    final rowContent = Row(
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        Expanded(child: activation),
        if (!removedUpstream && subs.probeSupported)
          IconButton(
            key: ValueKey('sub-endpoint-probe-${ep.id}'),
            tooltip: strings.get('sub_probe_one'),
            onPressed:
                canActivate && !subs.probeRunning && !connectedTunProbeBlocked
                ? () => _startProbe(sub, endpointIds: [ep.id])
                : null,
            icon: const Icon(LucideIcons.activity, size: 17),
          ),
        if (!removedUpstream)
          IconButton(
            key: ValueKey('sub-endpoint-pin-${ep.id}'),
            tooltip: strings.get(ep.pinned ? 'sub_unpin' : 'sub_pin'),
            onPressed: subs.busy
                ? null
                : () => unawaited(
                    subs.setPinned(
                      subscriptionId: sub.id,
                      endpointId: ep.id,
                      pinned: !ep.pinned,
                    ),
                  ),
            icon: Icon(
              LucideIcons.star,
              size: 17,
              color: ep.pinned
                  ? theme.colorScheme.primary
                  : theme.colorScheme.onSurfaceVariant,
            ),
          ),
        const SizedBox(width: 6),
      ],
    );

    return Semantics(
      selected: isActive,
      container: true,
      child: Container(
        decoration: BoxDecoration(
          color: isActive
              ? theme.colorScheme.primary.withValues(alpha: tokens.tint * 0.55)
              : null,
          border: BorderDirectional(
            start: BorderSide(
              width: 3,
              color: isActive ? theme.colorScheme.primary : Colors.transparent,
            ),
          ),
        ),
        child: rowContent,
      ),
    );
  }

  Widget _buildProbeStatus(
    BuildContext context,
    AppStrings strings, {
    required EndpointProbeResult? probe,
    required bool probingThisSub,
  }) {
    if (probe == null) {
      if (probingThisSub) {
        return const SizedBox(
          width: 16,
          height: 16,
          child: CircularProgressIndicator(strokeWidth: 2),
        );
      }
      return const SizedBox.shrink();
    }
    if (probe.status == EndpointProbeStatus.reachable) {
      final ms = probe.rttMs ?? 0;
      final proto = probe.protocol?.label;
      final rttLabel = strings.get('sub_rtt_ms').replaceAll('{ms}', '$ms');
      final text = proto == null ? rttLabel : '$rttLabel · $proto';
      final tone = ms < 120
          ? StatusTone.success
          : ms < 260
          ? StatusTone.warning
          : StatusTone.danger;
      return Text(
        text,
        style: Theme.of(context).textTheme.labelMedium?.copyWith(
          color: statusToneColor(context, tone),
          fontWeight: FontWeight.w600,
        ),
      );
    }
    final (String text, StatusTone tone) = switch (probe.status) {
      EndpointProbeStatus.reachable => ('', StatusTone.neutral),
      EndpointProbeStatus.timedOut => (
        strings.get('sub_probe_timed_out'),
        StatusTone.warning,
      ),
      EndpointProbeStatus.pinMismatch => (
        strings.get('sub_probe_pin_mismatch'),
        StatusTone.danger,
      ),
      EndpointProbeStatus.cancelled => (
        strings.get('sub_probe_cancelled'),
        StatusTone.neutral,
      ),
      EndpointProbeStatus.networkChanged || EndpointProbeStatus.failed => (
        strings.get('sub_probe_failed'),
        StatusTone.danger,
      ),
    };
    return Text(
      text,
      style: Theme.of(context).textTheme.labelMedium?.copyWith(
        color: statusToneColor(context, tone),
        fontWeight: FontWeight.w600,
      ),
    );
  }
}
