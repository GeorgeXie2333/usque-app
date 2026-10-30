import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../state/app_controller.dart';
import 'common.dart';

class AdsRulesSection extends StatelessWidget {
  const AdsRulesSection({
    required this.controller,
    required this.enabled,
    required this.onChanged,
    super.key,
  });

  final AppController controller;
  final bool enabled;
  final ValueChanged<bool>? onChanged;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final saved = controller.activeProfile.routing.adsEnabled;
    final active =
        controller.snapshot.isConnected &&
        controller.snapshot.adsRuleRevision.isNotEmpty;
    final ready = controller.geoRules.hasAds;
    final status = controller.adsRulesBusy
        ? 'ads_preparing'
        : enabled != saved
        ? enabled
              ? 'ads_apply_on'
              : 'ads_apply_off'
        : !saved
        ? 'ads_auto_hint'
        : active
        ? 'ads_active'
        : !ready
        ? 'ads_missing'
        : controller.snapshot.isConnected || controller.snapshot.isTransitional
        ? 'ads_applying'
        : 'ads_ready';
    final failed = controller.adsRulesFailed && enabled;
    final busy = controller.geoOperationActive || controller.adsRulesBusy;
    final timestamp = controller.geoRules.globalGeositeUpdatedUnixMilliseconds;
    final updated = timestamp > 0
        ? DateTime.fromMillisecondsSinceEpoch(timestamp)
        : null;
    return RowTileTheme(
      child: ContentGroup(
        title: strings.get('routing_protection'),
        children: [
          ContentSection(
            padding: RowInsets.tile,
            children: [
              SwitchListTile(
                key: const ValueKey('routing-ads'),
                contentPadding: EdgeInsets.zero,
                secondary: const Icon(LucideIcons.megaphoneOff),
                title: Text(strings.get('routing_ads')),
                // Whether blocking is actually in effect is a state, not a
                // hint, so it reads as one; the other lines explain next steps.
                subtitle: switch (status) {
                  'ads_active' => Align(
                    alignment: AlignmentDirectional.centerStart,
                    child: InlineStatus(
                      label: strings.get(status),
                      tone: StatusTone.success,
                    ),
                  ),
                  'ads_missing' => Align(
                    alignment: AlignmentDirectional.centerStart,
                    child: InlineStatus(
                      label: strings.get(status),
                      tone: StatusTone.warning,
                    ),
                  ),
                  _ => Text(strings.get(status)),
                },
                value: enabled,
                onChanged: onChanged,
              ),
              Padding(
                padding: const EdgeInsetsDirectional.only(
                  start: RowInsets.textIndent,
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    if (failed)
                      HintText(
                        strings.get(
                          ready || active
                              ? 'ads_failed_old'
                              : 'ads_failed_missing',
                        ),
                      ),
                    if (enabled &&
                        !busy &&
                        (failed || saved && !ready) &&
                        controller.supportsLiveAds)
                      TextButton.icon(
                        key: const ValueKey('ads-retry'),
                        onPressed: () =>
                            controller.updateAdsRules(manual: true),
                        icon: const Icon(LucideIcons.refreshCw),
                        label: Text(strings.get('retry')),
                      ),
                    ExpansionTile(
                      key: const ValueKey('ads-details'),
                      tilePadding: EdgeInsets.zero,
                      childrenPadding: EdgeInsets.zero,
                      title: Text(strings.get('ads_details')),
                      expandedCrossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        HintText(strings.get('ads_auto_hint')),
                        HintText(strings.get('ads_source')),
                        if (updated != null)
                          HintText(
                            '${strings.get('ads_updated')}: ${_date(context, updated)}',
                          ),
                        if (controller.adsLastChecked case final checked?)
                          HintText(
                            '${strings.get('ads_checked')}: ${_date(context, checked)}',
                          ),
                        if (!controller.supportsLiveAds)
                          HintText(strings.get('bypass_unsupported')),
                        TextButton.icon(
                          key: const ValueKey('ads-update'),
                          onPressed: busy || !controller.supportsLiveAds
                              ? null
                              : () => controller.updateAdsRules(manual: true),
                          icon: const Icon(LucideIcons.refreshCw),
                          label: Text(strings.get('ads_update')),
                        ),
                      ],
                    ),
                  ],
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }

  String _date(BuildContext context, DateTime date) {
    final local = date.toLocal();
    final material = MaterialLocalizations.of(context);
    return '${material.formatMediumDate(local)} ${material.formatTimeOfDay(TimeOfDay.fromDateTime(local))}';
  }
}
