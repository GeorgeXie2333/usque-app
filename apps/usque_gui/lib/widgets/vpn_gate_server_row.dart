import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import 'country_flag.dart';

/// A source observation's age, never a measurement of local connectivity.
String vpnGateObservationAge(
  DateTime? value,
  DateTime now,
  AppStrings strings,
) {
  if (value == null) return '—';
  final age = now.difference(value);
  // Clock skew must not turn an observation into a negative age or fresh data.
  if (age.isNegative) return '—';
  if (age.inMinutes == 0) return strings.get('gate_just_now');
  final (key, count) = age.inDays > 0
      ? ('gate_days_ago', age.inDays)
      : age.inHours > 0
      ? ('gate_hours_ago', age.inHours)
      : ('gate_minutes_ago', age.inMinutes);
  return strings.get(key).replaceAll('{count}', '$count');
}

class VpnGateServerRow extends StatefulWidget {
  const VpnGateServerRow({
    required this.server,
    required this.strings,
    required this.now,
    required this.selected,
    this.onSelect,
    this.onFavorite,
    this.onUpdateFavorite,
    super.key,
  });

  final VpnGateServer server;
  final AppStrings strings;
  final DateTime now;
  final bool selected;
  final VoidCallback? onSelect, onFavorite, onUpdateFavorite;

  @override
  State<VpnGateServerRow> createState() => _VpnGateServerRowState();
}

class _VpnGateServerRowState extends State<VpnGateServerRow> {
  bool _expanded = false;

  String _time(DateTime? value) =>
      value?.toLocal().toString().split('.').first ?? '—';

  String _age(DateTime? value) =>
      vpnGateObservationAge(value, widget.now, widget.strings);

  @override
  Widget build(BuildContext context) {
    final server = widget.server;
    final strings = widget.strings;
    final metadata = server.pool;
    final favorite = server.favorite;
    final enabled = widget.onSelect != null;
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final secondary = enabled ? scheme.onSurfaceVariant : theme.disabledColor;
    final style = theme.textTheme.bodySmall!.copyWith(color: secondary);
    final expired =
        metadata?.checkedAt != null &&
        widget.now.difference(metadata!.checkedAt!) > const Duration(hours: 12);
    final tcpStatus = strings.get(switch (metadata?.tcpStatus) {
      'reachable' => 'gate_tcp_reachable',
      'unreachable' => 'gate_tcp_unreachable',
      _ => 'gate_tcp_unknown',
    });

    Widget tag(String key) => Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      decoration: BoxDecoration(
        color: scheme.onSurfaceVariant.withValues(alpha: 0.06),
        borderRadius: BorderRadius.circular(6),
        border: Border.all(color: UsqueTokens.of(context).hairline),
      ),
      child: Text(strings.get(key), style: style),
    );

    Widget observedTime(String label, DateTime? time) => Tooltip(
      message: '${strings.get('gate_local_time')}: ${_time(time)}',
      excludeFromSemantics: true,
      child: Text('$label: ${_age(time)}', style: style),
    );

    final detailsButton = Semantics(
      expanded: _expanded,
      child: TextButton.icon(
        key: ValueKey('vpn-gate-details-${server.id}'),
        style: TextButton.styleFrom(foregroundColor: scheme.onSurfaceVariant),
        onPressed: () => setState(() => _expanded = !_expanded),
        iconAlignment: IconAlignment.end,
        icon: Icon(
          _expanded ? LucideIcons.chevronUp : LucideIcons.chevronDown,
          size: 16,
        ),
        label: Text(strings.get('gate_node_details')),
      ),
    );

    final sourceSummary = <Widget>[
      if (metadata != null) ...[
        if (!metadata.inPool) tag('gate_pool_absent_short'),
        observedTime(strings.get('gate_last_seen_short'), metadata.lastSeenAt),
      ],
    ];
    final tcpSummary = <Widget>[
      if (metadata != null)
        Tooltip(
          message:
              '${strings.get('gate_local_time')}: ${_time(metadata.checkedAt)}',
          excludeFromSemantics: true,
          child: Text(
            '${strings.get('gate_tcp_probe')}: $tcpStatus · ${_age(metadata.checkedAt)}',
            style: style,
          ),
        ),
      if (expired) tag('gate_probe_expired'),
    ];

    Widget summary(List<Widget> children) => Wrap(
      spacing: 12,
      runSpacing: 6,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: children,
    );

    return FocusTraversalGroup(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(
                child: ListTile(
                  key: ValueKey('vpn-gate-node-${server.id}'),
                  contentPadding: const EdgeInsetsDirectional.only(
                    start: 16,
                    end: 8,
                  ),
                  minLeadingWidth: 24,
                  horizontalTitleGap: 16,
                  minTileHeight: 72,
                  minVerticalPadding: 8,
                  titleAlignment: ListTileTitleAlignment.top,
                  enabled: enabled,
                  selected: widget.selected,
                  leading: Icon(
                    widget.selected
                        ? LucideIcons.circleCheck
                        : LucideIcons.circle,
                    size: 24,
                  ),
                  title: Row(
                    children: [
                      CountryFlag(
                        countryCode: server.countryCode,
                        enabled: enabled,
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          '${server.countryCode ?? '—'} · ${server.ip}',
                          style: const TextStyle(fontFamily: UsqueFonts.mono),
                        ),
                      ),
                    ],
                  ),
                  subtitle: Text(
                    '${server.hostname}\n${strings.get('gate_score')}: ${server.score ?? '—'} · ${server.pingMs == null ? '—' : '${server.pingMs} ms'} · ${server.speedBps == null ? '—' : '${(server.speedBps! / 1000000).toStringAsFixed(1)} Mbps'}',
                  ),
                  isThreeLine: true,
                  onTap: widget.onSelect,
                ),
              ),
              Padding(
                padding: const EdgeInsetsDirectional.only(end: 8),
                child: IconButton(
                  key: ValueKey('vpn-gate-favorite-${server.id}'),
                  tooltip: strings.get(
                    favorite == null
                        ? 'gate_favorite_add'
                        : 'gate_favorite_remove',
                  ),
                  isSelected: favorite != null,
                  onPressed: widget.onFavorite,
                  icon: Icon(
                    favorite == null ? LucideIcons.star : LucideIcons.starOff,
                  ),
                ),
              ),
            ],
          ),
          if (metadata != null || favorite?.latestConfigSha256 != null)
            Padding(
              // Matches ListTile's 16px inset + 24px leading + 16px title gap.
              padding: const EdgeInsetsDirectional.only(
                start: 56,
                end: 16,
                bottom: 12,
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (metadata != null) ...[
                    LayoutBuilder(
                      builder: (context, constraints) {
                        final scale =
                            MediaQuery.textScalerOf(context).scale(13) / 13;
                        if (constraints.maxWidth / scale >= 640) {
                          return Row(
                            children: [
                              Expanded(
                                child: summary([
                                  ...sourceSummary,
                                  ...tcpSummary,
                                ]),
                              ),
                              const SizedBox(width: 8),
                              detailsButton,
                            ],
                          );
                        }
                        return Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            summary(sourceSummary),
                            const SizedBox(height: 6),
                            if (constraints.maxWidth / scale >= 240)
                              Row(
                                children: [
                                  Expanded(child: summary(tcpSummary)),
                                  const SizedBox(width: 8),
                                  detailsButton,
                                ],
                              )
                            else ...[
                              summary(tcpSummary),
                              Align(
                                alignment: AlignmentDirectional.centerEnd,
                                child: detailsButton,
                              ),
                            ],
                          ],
                        );
                      },
                    ),
                    if (_expanded)
                      Container(
                        key: ValueKey('vpn-gate-observations-${server.id}'),
                        margin: const EdgeInsets.only(top: 8),
                        padding: const EdgeInsets.all(12),
                        decoration: BoxDecoration(
                          border: Border.all(
                            color: UsqueTokens.of(context).hairline,
                          ),
                          borderRadius: BorderRadius.circular(8),
                        ),
                        child: DefaultTextStyle(
                          style: style,
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            spacing: 6,
                            children: [
                              Text(strings.get('gate_local_time')),
                              Text(
                                '${strings.get('gate_first_seen')}: ${_time(metadata.firstSeenAt)}',
                              ),
                              Text(
                                '${strings.get('gate_last_seen')}: ${_time(metadata.lastSeenAt)}',
                              ),
                              Text(
                                '${strings.get('gate_tcp_probe')}: $tcpStatus · ${_time(metadata.checkedAt)}',
                              ),
                              if (expired) Text(strings.get('gate_probe_old')),
                            ],
                          ),
                        ),
                      ),
                  ],
                  if (favorite?.latestConfigSha256 != null)
                    Padding(
                      padding: const EdgeInsets.only(top: 4),
                      child: Wrap(
                        spacing: 8,
                        runSpacing: 4,
                        crossAxisAlignment: WrapCrossAlignment.center,
                        children: [
                          Text(
                            strings.get(
                              favorite!.configSha256 == server.configSha256
                                  ? 'gate_favorite_new'
                                  : 'gate_favorite_old',
                            ),
                            style: theme.textTheme.bodySmall?.copyWith(
                              color: scheme.onSurfaceVariant,
                            ),
                          ),
                          TextButton.icon(
                            key: ValueKey('vpn-gate-update-${server.id}'),
                            onPressed: widget.onUpdateFavorite,
                            icon: const Icon(LucideIcons.refreshCw),
                            label: Text(strings.get('gate_favorite_update')),
                          ),
                        ],
                      ),
                    ),
                ],
              ),
            ),
          Divider(
            height: 1,
            indent: 16,
            endIndent: 16,
            color: UsqueTokens.of(context).hairline,
          ),
        ],
      ),
    );
  }
}
