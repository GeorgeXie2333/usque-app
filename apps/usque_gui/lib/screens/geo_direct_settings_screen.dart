import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/iso_countries.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/country_flag.dart';
import '../widgets/routing_rules_editor.dart';
import '../widgets/save_changes_bar.dart';
import '../widgets/unsaved_changes_guard.dart';

class GeoDirectSettingsScreen extends StatefulWidget {
  const GeoDirectSettingsScreen({required this.controller, super.key});
  final AppController controller;
  @override
  State<GeoDirectSettingsScreen> createState() =>
      _GeoDirectSettingsScreenState();
}

class _GeoDirectSettingsScreenState extends State<GeoDirectSettingsScreen> {
  final _search = TextEditingController();
  late Set<String> _enabled;
  late RoutingSettings _routing;
  late String _baseline;
  late String _countriesBaseline;
  late RoutingSettings _routingBaseline;
  late String _accountId;
  bool _saving = false;
  bool _saved = false;
  String? _error;
  List<String> _backendErrorIds = [];
  String get _draft =>
      '${_orderedCountries(_enabled).join(',')}|${jsonEncode(_routing.toMap())}';
  bool get _dirty => _draft != _baseline;
  bool get _customAvailable =>
      widget.controller.engineCapabilities?.routingRules ?? false;

  @override
  void initState() {
    super.initState();
    final profile = widget.controller.activeProfile;
    _accountId = profile.id;
    _load(profile);
    widget.controller.refreshGeoRules();
  }

  void _load(UsqueProfile profile) {
    _enabled = profile.geoDirectCountries.toSet();
    _routing = profile.routing;
    if (!_customAvailable && _routing.rules.isEmpty) {
      _routing = RoutingSettings(
        rules: [...profile.bypassCidrs, ...profile.bypassDomains]
            .map((target) => RoutingRule.create(target, RoutingAction.direct))
            .toList(),
      );
    }
    _baseline = _draft;
    _routingBaseline = _routing;
    _countriesBaseline = _orderedCountries(_enabled).join(',');
  }

  @override
  void dispose() {
    _search.dispose();
    super.dispose();
  }

  void _edited(RoutingSettings value) => setState(() {
    _routing = value;
    _saved = false;
    _error = null;
    _backendErrorIds = [];
  });

  String _issue(String key, List<String> ids) {
    final positions = ids
        .map((id) => _routing.rules.indexWhere((rule) => rule.id == id) + 1)
        .where((index) => index > 0)
        .join(', ');
    return '${widget.controller.strings.get(key)}${positions.isEmpty ? '' : ' ($positions)'}';
  }

  Future<void> _save() async {
    if (_saving) return;
    final controller = widget.controller;
    if (controller.activeProfile.id != _accountId) {
      setState(() => _error = controller.strings.get('changes_failed'));
      return;
    }
    RoutingSettings normalized;
    try {
      normalized = _routing.validate().settings;
    } on RoutingRuleError catch (error) {
      setState(() {
        _error = _issue(error.key, error.ids);
        _backendErrorIds = error.ids;
      });
      return;
    }
    if (!_customAvailable && _routing != _routingBaseline) return;
    setState(() {
      _saving = true;
      _error = null;
    });
    final saved = await controller.saveNetwork(
      controller.activeProfile.copyWith(
        routing: _customAvailable
            ? normalized
            : controller.activeProfile.routing,
        geoDirectCountries: _orderedCountries(_enabled),
      ),
      changedFields: [
        if (_orderedCountries(_enabled).join(',') != _countriesBaseline)
          'geo_direct_countries',
        if (_customAvailable && _routing != _routingBaseline) 'routing',
      ],
    );
    if (!mounted) return;
    setState(() {
      _saving = false;
      _saved = saved;
      if (saved) {
        _load(controller.activeProfile);
        _backendErrorIds = [];
      } else {
        _backendErrorIds = controller.networkSettings.routingErrorIds;
        _error = _backendErrorIds.isEmpty
            ? controller.lastError ?? controller.strings.get('changes_failed')
            : _issue(
                controller.networkSettings.routingErrorKey ??
                    'routing_conflict',
                _backendErrorIds,
              );
      }
    });
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.controller,
    builder: (context, _) {
      final controller = widget.controller;
      final strings = controller.strings;
      final appliedAds =
          (controller.snapshot.isConnected ||
              controller.snapshot.isTransitional) &&
          (controller
                  .networkSettings
                  .state
                  ?.appliedProfile
                  ?.routing
                  .adsEnabled ??
              false);
      final activeAds = controller.snapshot.adsRuleRevision;
      final adsPending =
          appliedAds &&
          controller.geoRules.hasAds &&
          activeAds != controller.geoRules.adsRevision;
      String? validationError;
      var errorIds = _backendErrorIds;
      List<RoutingNotice> notices = [];
      try {
        notices = _routing.validate().notices;
      } on RoutingRuleError catch (error) {
        validationError = _issue(error.key, error.ids);
        errorIds = error.ids;
      }
      return UnsavedChangesGuard(
        strings: strings,
        dirty: _dirty,
        saving: _saving,
        child: SubPage(
          contentWidth: 880,
          title: strings.get('geo_direct'),
          backLabel: strings.get('back'),
          bottomBar: SaveChangesBar(
            strings: strings,
            dirty: _dirty,
            saving: _saving,
            saved: _saved,
            error: _error,
            validationError: validationError,
            statusLabel:
                controller.networkSettings.unconfirmed ||
                    (_error == null &&
                        (!_dirty ||
                            controller.networkSettings.saveError != null))
                ? controller.networkSettingsMessage
                : null,
            onReconnect: controller.networkSettingsCanReconnect
                ? controller.retry
                : null,
            onSave: validationError == null ? _save : null,
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              BannerSlot(
                child: controller.lastError == null
                    ? null
                    : WarningBanner(
                        title: strings.get('error_generic'),
                        message: controller.lastError!,
                        danger: true,
                        onDismiss: controller.clearError,
                      ),
              ),
              BannerSlot(
                child: controller.lastNotice == null
                    ? null
                    : WarningBanner(
                        title: strings.get('notice'),
                        message: controller.lastNotice!,
                        onDismiss: controller.clearNotice,
                      ),
              ),
              ContentSection(
                child: RoutingRulesEditor(
                  value: _routing,
                  strings: strings,
                  errorIds: errorIds,
                  onChanged: _customAvailable && !_saving ? _edited : null,
                ),
              ),
              for (final notice in notices.take(8))
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: Text(_issue(notice.key, notice.ids)),
                ),
              const SizedBox(height: 24),
              ContentSection(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    SwitchListTile(
                      key: const ValueKey('routing-ads'),
                      contentPadding: EdgeInsets.zero,
                      title: Text(strings.get('routing_ads')),
                      subtitle: Text(
                        strings.get(
                          controller.geoRules.hasAds
                              ? 'routing_ads_ready'
                              : 'routing_ads_unavailable',
                        ),
                      ),
                      value: _routing.adsEnabled,
                      onChanged: _customAvailable && !_saving
                          ? (value) => _edited(
                              RoutingSettings(
                                rules: _routing.rules,
                                adsEnabled: value,
                              ),
                            )
                          : null,
                    ),
                    if (appliedAds)
                      Text(
                        "${strings.get('routing_current')}: ${strings.get(activeAds.isEmpty ? 'routing_ads_unavailable' : 'active')}",
                      ),
                    if (adsPending) Text(strings.get('routing_pending')),
                    Align(
                      alignment: AlignmentDirectional.centerStart,
                      child: TextButton.icon(
                        onPressed: controller.geoProgress != null
                            ? null
                            : controller.updateAllGeoRules,
                        icon: const Icon(LucideIcons.refreshCw),
                        label: Text(strings.get('geo_update_all')),
                      ),
                    ),
                  ],
                ),
              ),
              const SizedBox(height: 24),
              Text(
                strings.get('bypass_countries'),
                style: Theme.of(context).textTheme.titleMedium,
              ),
              const SizedBox(height: 12),
              ContentSection(child: _buildRulesPanel(context)),
            ],
          ),
        ),
      );
    },
  );

  Widget _buildRulesPanel(BuildContext context) {
    final controller = widget.controller;
    final strings = controller.strings;
    final progress = controller.geoProgress;
    final updating = progress != null && progress.total > 0;
    final query = _search.text.trim().toLowerCase();
    final byCode = <String, GeoRulesEntry>{
      for (final entry in controller.geoRules.entries) entry.countryCode: entry,
    };
    final countries = kIsoCountries
        .where((country) {
          if (query.isEmpty) {
            return true;
          }
          return country.code.toLowerCase().contains(query) ||
              country.name.toLowerCase().contains(query);
        })
        .toList(growable: false);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Wrap(
          spacing: 12,
          runSpacing: 10,
          crossAxisAlignment: WrapCrossAlignment.center,
          alignment: WrapAlignment.spaceBetween,
          children: <Widget>[
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Text('GeoSite', style: Theme.of(context).textTheme.titleSmall),
                const SizedBox(height: 3),
                Text(
                  _globalGeoSiteLabel(strings, controller.geoRules),
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: Theme.of(context).colorScheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
            FilledButton.tonalIcon(
              onPressed: updating ? null : controller.updateAllGeoRules,
              icon: const Icon(LucideIcons.refreshCw),
              label: Text(
                updating
                    ? strings
                          .get('geo_updating')
                          .replaceAll('{current}', '${progress.completed}')
                          .replaceAll('{total}', '${progress.total}')
                    : strings.get('geo_update_all'),
              ),
            ),
          ],
        ),
        const SizedBox(height: 14),
        _PrivacyNote(message: strings.get('geo_direct_help')),
        Padding(
          padding: const EdgeInsets.symmetric(vertical: 18),
          child: Divider(height: 1, color: UsqueTokens.of(context).hairline),
        ),
        TextField(
          key: const ValueKey('bypass-country-search'),
          controller: _search,
          onChanged: (_) => setState(() {}),
          decoration: InputDecoration(
            labelText: strings.get('geo_search'),
            prefixIcon: const Icon(LucideIcons.search),
          ),
        ),
        const SizedBox(height: 10),
        ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 480),
          child: ListView.separated(
            shrinkWrap: true,
            itemCount: countries.length,
            separatorBuilder: (context, _) =>
                Divider(height: 1, color: UsqueTokens.of(context).hairline),
            itemBuilder: (context, index) {
              final country = countries[index];
              final entry = byCode[country.code];
              final hasGeoip = entry?.hasGeoip ?? false;
              final ready = hasGeoip && (entry?.hasGeosite ?? false);
              final enabled = _enabled.contains(country.code);
              final date = _entryDate(entry);
              return ListTile(
                contentPadding: EdgeInsets.zero,
                leading: CountryFlag(countryCode: country.code),
                title: Text(
                  '${country.code}  ${country.name}',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                subtitle: Text(
                  ready
                      ? (date ?? strings.get('geo_downloaded'))
                      : strings.get('geo_not_downloaded'),
                ),
                trailing: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: <Widget>[
                    IconButton(
                      tooltip: strings.get(
                        hasGeoip ? 'geo_update' : 'geo_download',
                      ),
                      onPressed: updating
                          ? null
                          : () => controller.downloadGeoRules(country.code),
                      icon: Icon(
                        hasGeoip ? LucideIcons.refreshCw : LucideIcons.download,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Semantics(
                      label: '${strings.get('geo_enable')} ${country.code}',
                      child: Switch(
                        key: ValueKey('routing-country-${country.code}'),
                        value: enabled,
                        onChanged: !_saving && (ready || enabled)
                            ? (value) {
                                setState(() {
                                  _saved = false;
                                  _error = null;
                                  if (value) {
                                    _enabled.add(country.code);
                                  } else {
                                    _enabled.remove(country.code);
                                  }
                                });
                              }
                            : null,
                      ),
                    ),
                  ],
                ),
              );
            },
          ),
        ),
      ],
    );
  }

  String _globalGeoSiteLabel(AppStrings strings, GeoRulesList rules) {
    if (!rules.hasGlobalGeosite) {
      return strings.get('geo_not_downloaded');
    }
    if (rules.globalGeositeUpdatedUnixMilliseconds <= 0) {
      return strings.get('geo_downloaded');
    }
    final time = DateTime.fromMillisecondsSinceEpoch(
      rules.globalGeositeUpdatedUnixMilliseconds,
    ).toLocal().toString().split('.').first;
    return strings.get('geo_last_updated').replaceAll('{current}', time);
  }

  String? _entryDate(GeoRulesEntry? entry) {
    if (entry == null || entry.lastUpdatedUnixMilliseconds <= 0) {
      return null;
    }
    return DateTime.fromMillisecondsSinceEpoch(
      entry.lastUpdatedUnixMilliseconds,
    ).toLocal().toString().split('.').first;
  }
}

class _PrivacyNote extends StatelessWidget {
  const _PrivacyNote({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    final color = Theme.of(context).colorScheme.onSurfaceVariant;
    return Semantics(
      container: true,
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Padding(
            padding: const EdgeInsets.only(top: 1),
            child: Icon(LucideIcons.info, size: 16, color: color),
          ),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              message,
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: color),
            ),
          ),
        ],
      ),
    );
  }
}

List<String> _orderedCountries(Set<String> enabled) {
  final countries = enabled.toList()..sort();
  if (countries.remove('CN')) {
    countries.insert(0, 'CN');
  }
  return countries;
}
