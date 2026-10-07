import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/iso_countries.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../models/bypass_targets.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/country_flag.dart';
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
  final TextEditingController _search = TextEditingController();
  late Set<String> _enabled;
  bool _saving = false;
  bool _saved = false;
  String? _error;
  final _targets = TextEditingController();
  late String _baseline;
  late String _targetsBaseline;
  late String _countriesBaseline;
  late String _accountId;
  String get _draft =>
      '${_orderedCountries(_enabled).join(',')}|${_targets.text}';
  bool get _dirty => _draft != _baseline;
  bool get _customAvailable =>
      widget.controller.engineCapabilities?.customBypass ?? false;
  void _edited() => setState(() {
    _error = null;
    _saved = false;
  });

  @override
  void initState() {
    super.initState();
    _enabled = widget.controller.activeProfile.geoDirectCountries.toSet();
    final profile = widget.controller.activeProfile;
    _accountId = profile.id;
    _targets.text = [
      ...profile.bypassCidrs,
      ...profile.bypassDomains,
    ].join('\n');
    _baseline = _draft;
    _targetsBaseline = _targets.text;
    _countriesBaseline = _orderedCountries(_enabled).join(',');
    widget.controller.refreshGeoRules();
  }

  @override
  void dispose() {
    _search.dispose();
    _targets.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    if (_saving) return;
    final controller = widget.controller;
    final strings = controller.strings;
    if (!_customAvailable && _targets.text != _targetsBaseline) {
      setState(() => _error = strings.get('bypass_unsupported'));
      return;
    }
    BypassTargets targets;
    try {
      targets = BypassTargets.parse(_targets.text);
    } on BypassTargetError catch (error) {
      setState(
        () => _error = strings
            .get('bypass_line_error')
            .replaceAll('{line}', '${error.line}')
            .replaceAll('{reason}', strings.get(error.messageKey)),
      );
      return;
    }
    final profile = controller.activeProfile;
    if (profile.id != _accountId) {
      setState(() => _error = strings.get('changes_failed'));
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });
    final saved = await controller.saveNetwork(
      profile.copyWith(
        geoDirectCountries: _orderedCountries(_enabled),
        bypassCidrs: _customAvailable ? targets.cidrs : profile.bypassCidrs,
        bypassDomains: _customAvailable
            ? targets.domains
            : profile.bypassDomains,
      ),
      changedFields: [
        if (_orderedCountries(_enabled).join(',') != _countriesBaseline)
          'geo_direct_countries',
        if (_customAvailable && _targets.text != _targetsBaseline) ...[
          'split_exclusions',
          'bypass_domains',
        ],
      ],
    );
    if (!mounted) return;
    setState(() {
      _saving = false;
      _saved = saved;
      if (saved) {
        final stored = controller.activeProfile;
        _enabled = stored.geoDirectCountries.toSet();
        _targets.text = [
          ...stored.bypassCidrs,
          ...stored.bypassDomains,
        ].join('\n');
        _baseline = _draft;
        _targetsBaseline = _targets.text;
        _countriesBaseline = _orderedCountries(_enabled).join(',');
      } else {
        _error = controller.lastError ?? strings.get('changes_failed');
        final entry = controller.networkSettings.invalidBypassDomainEntry;
        if (entry != null && entry > 0 && entry <= targets.domainLines.length) {
          _error = strings
              .get('bypass_line_error')
              .replaceAll('{line}', '${targets.domainLines[entry - 1]}')
              .replaceAll('{reason}', strings.get('invalid_dns_name'));
        }
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.controller,
      builder: (context, _) {
        final strings = widget.controller.strings;
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
              validationError: _error,
              statusLabel:
                  !_dirty ||
                      widget.controller.networkSettings.unconfirmed ||
                      widget.controller.networkSettings.saveError != null
                  ? widget.controller.networkSettingsMessage
                  : null,
              onReconnect: widget.controller.networkSettingsCanReconnect
                  ? widget.controller.retry
                  : null,
              onSave: _save,
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                BannerSlot(
                  child: widget.controller.lastError == null
                      ? null
                      : WarningBanner(
                          title: strings.get('error_generic'),
                          message: widget.controller.lastError!,
                          danger: true,
                          onDismiss: widget.controller.clearError,
                        ),
                ),
                BannerSlot(
                  child: widget.controller.lastNotice == null
                      ? null
                      : WarningBanner(
                          title: strings.get('notice'),
                          message: widget.controller.lastNotice!,
                          onDismiss: widget.controller.clearNotice,
                        ),
                ),
                ContentSection(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Text(
                        strings.get('bypass_custom'),
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                      const SizedBox(height: 12),
                      TextField(
                        key: const ValueKey('bypass-targets'),
                        controller: _targets,
                        readOnly: !_customAvailable || _saving,
                        onChanged: (_) => _edited(),
                        minLines: 4,
                        maxLines: 8,
                        autocorrect: false,
                        enableSuggestions: false,
                        decoration: InputDecoration(
                          labelText: strings.get('bypass_custom'),
                          hintText: '192.0.2.0/24\n2001:db8::1\nexample.com',
                          helperText: strings.get('bypass_targets_hint'),
                          helperMaxLines: 5,
                          alignLabelWithHint: true,
                        ),
                      ),
                      if (!_customAvailable)
                        Padding(
                          padding: const EdgeInsets.only(top: 8),
                          child: Text(strings.get('bypass_unsupported')),
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
  }

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
