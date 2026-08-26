import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/iso_countries.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/usque_dialog.dart';

class AdvancedSettingsScreen extends StatefulWidget {
  const AdvancedSettingsScreen({required this.controller, super.key});

  final AppController controller;

  @override
  State<AdvancedSettingsScreen> createState() => _AdvancedSettingsScreenState();
}

class _AdvancedSettingsScreenState extends State<AdvancedSettingsScreen> {
  final GlobalKey<FormState> _formKey = GlobalKey<FormState>();
  late final TextEditingController _endpointV4;
  late final TextEditingController _endpointV6;
  late final TextEditingController _port;
  late final TextEditingController _sni;
  late final TextEditingController _mtu;
  late final TextEditingController _dnsV4;
  late final TextEditingController _dnsV6;
  late final TextEditingController _bypass;
  late final TextEditingController _geoSearch;
  late TransportPolicy _transport;
  late IpPolicy _ipPolicy;
  late bool _killSwitch;
  late bool _allowLan;
  late Set<String> _geoEnabled;

  bool get _zeroTrustEndpointManaged =>
      widget.controller
          .identityStatus(widget.controller.activeProfile.id)
          .provider ==
      IdentityProvider.zeroTrust;

  @override
  void initState() {
    super.initState();
    _endpointV4 = TextEditingController();
    _endpointV6 = TextEditingController();
    _port = TextEditingController();
    _sni = TextEditingController();
    _mtu = TextEditingController();
    _dnsV4 = TextEditingController();
    _dnsV6 = TextEditingController();
    _bypass = TextEditingController();
    _geoSearch = TextEditingController();
    _load(widget.controller.activeProfile);
    widget.controller.refreshGeoRules();
  }

  void _load(UsqueProfile profile) {
    _endpointV4.text = profile.endpointIpv4;
    _endpointV6.text = profile.endpointIpv6;
    _port.text = profile.endpointPort.toString();
    _sni.text = profile.sni;
    _mtu.text = profile.mtu.toString();
    _dnsV4.text = profile.dnsIpv4;
    _dnsV6.text = profile.dnsIpv6;
    _bypass.text = profile.bypassCidrs.join('\n');
    _transport = profile.transport;
    _ipPolicy = profile.ipPolicy;
    _killSwitch = profile.killSwitch;
    _allowLan = profile.allowLan;
    _geoEnabled = profile.geoDirectCountries.toSet();
  }

  @override
  void dispose() {
    for (final controller in <TextEditingController>[
      _endpointV4,
      _endpointV6,
      _port,
      _sni,
      _mtu,
      _dnsV4,
      _dnsV6,
      _bypass,
      _geoSearch,
    ]) {
      controller.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final strings = widget.controller.strings;
    return SubPage(
      title: strings.get('advanced'),
      subtitle: strings.get('advanced_subtitle'),
      backLabel: strings.get('back'),
      actions: <Widget>[
        OutlinedButton.icon(
          onPressed: _reset,
          icon: const Icon(LucideIcons.rotateCcw),
          label: Text(strings.get('reset_defaults')),
        ),
        FilledButton.icon(
          onPressed: _save,
          icon: const Icon(LucideIcons.save),
          label: Text(strings.get('save')),
        ),
      ],
      child: Form(
        key: _formKey,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            WarningBanner(
              title: strings.get('advanced'),
              message: strings.get('advanced_warning'),
            ),
            const SizedBox(height: 16),
            PanelStack(
              children: <Widget>[
                SectionPanel(
                  icon: LucideIcons.cable,
                  title: strings.get('transport'),
                  subtitle: _zeroTrustEndpointManaged
                      ? strings.get('zero_trust_endpoint_managed')
                      : null,
                  gap: 20,
                  children: <Widget>[
                    SegmentedButton<TransportPolicy>(
                      segments: <ButtonSegment<TransportPolicy>>[
                        ButtonSegment<TransportPolicy>(
                          value: TransportPolicy.automatic,
                          label: Text(strings.get('automatic')),
                        ),
                        ButtonSegment<TransportPolicy>(
                          value: TransportPolicy.http3,
                          label: Text(strings.get('http3')),
                        ),
                        ButtonSegment<TransportPolicy>(
                          value: TransportPolicy.http2,
                          label: Text(strings.get('http2')),
                        ),
                      ],
                      selected: <TransportPolicy>{_transport},
                      onSelectionChanged: (selection) =>
                          setState(() => _transport = selection.first),
                      showSelectedIcon: false,
                    ),
                    const SizedBox(height: 18),
                    _ResponsiveFields(
                      children: <Widget>[
                        TextFormField(
                          controller: _endpointV4,
                          readOnly: _zeroTrustEndpointManaged,
                          decoration: InputDecoration(
                            labelText: strings.get('endpoint_ipv4'),
                          ),
                          validator: (value) =>
                              _validateIp(value, InternetAddressType.IPv4),
                        ),
                        TextFormField(
                          controller: _endpointV6,
                          readOnly: _zeroTrustEndpointManaged,
                          decoration: InputDecoration(
                            labelText: strings.get('endpoint_ipv6'),
                          ),
                          validator: (value) =>
                              _validateIp(value, InternetAddressType.IPv6),
                        ),
                        TextFormField(
                          controller: _port,
                          readOnly: _zeroTrustEndpointManaged,
                          keyboardType: TextInputType.number,
                          inputFormatters: <TextInputFormatter>[
                            FilteringTextInputFormatter.digitsOnly,
                          ],
                          decoration: InputDecoration(
                            labelText: strings.get('port'),
                          ),
                          validator: _validatePort,
                        ),
                        TextFormField(
                          controller: _sni,
                          readOnly: _zeroTrustEndpointManaged,
                          keyboardType: TextInputType.url,
                          decoration: InputDecoration(
                            labelText: strings.get('sni'),
                          ),
                          validator: _validateSni,
                        ),
                      ],
                    ),
                  ],
                ),
                SectionPanel(
                  icon: LucideIcons.network,
                  title: strings.get('ip_dns'),
                  gap: 20,
                  children: <Widget>[
                    DropdownButtonFormField<IpPolicy>(
                      initialValue: _ipPolicy,
                      decoration: InputDecoration(
                        labelText: strings.get('ip_policy'),
                      ),
                      items: IpPolicy.values
                          .map(
                            (value) => DropdownMenuItem<IpPolicy>(
                              value: value,
                              child: Text(_ipPolicyLabel(value)),
                            ),
                          )
                          .toList(growable: false),
                      onChanged: (value) {
                        if (value != null) {
                          setState(() => _ipPolicy = value);
                        }
                      },
                    ),
                    const SizedBox(height: 14),
                    _ResponsiveFields(
                      children: <Widget>[
                        TextFormField(
                          controller: _dnsV4,
                          decoration: InputDecoration(
                            labelText: strings.get('dns_ipv4'),
                          ),
                          validator: (value) =>
                              _validateIp(value, InternetAddressType.IPv4),
                        ),
                        TextFormField(
                          controller: _dnsV6,
                          decoration: InputDecoration(
                            labelText: strings.get('dns_ipv6'),
                          ),
                          validator: (value) =>
                              _validateIp(value, InternetAddressType.IPv6),
                        ),
                        TextFormField(
                          controller: _mtu,
                          keyboardType: TextInputType.number,
                          inputFormatters: <TextInputFormatter>[
                            FilteringTextInputFormatter.digitsOnly,
                          ],
                          decoration: InputDecoration(
                            labelText: strings.get('mtu'),
                          ),
                          validator: _validateMtu,
                        ),
                      ],
                    ),
                  ],
                ),
                SectionPanel(
                  icon: LucideIcons.shieldCheck,
                  title: strings.get('routing_protection'),
                  gap: 10,
                  children: <Widget>[
                    SwitchListTile(
                      contentPadding: EdgeInsets.zero,
                      title: Text(strings.get('kill_switch')),
                      subtitle: Text(strings.get('kill_switch_help')),
                      value: _killSwitch,
                      onChanged: (value) => setState(() => _killSwitch = value),
                    ),
                    SwitchListTile(
                      contentPadding: EdgeInsets.zero,
                      title: Text(strings.get('allow_lan')),
                      value: _allowLan,
                      onChanged: (value) => setState(() => _allowLan = value),
                    ),
                    const SizedBox(height: 14),
                    TextFormField(
                      controller: _bypass,
                      minLines: 3,
                      maxLines: 6,
                      style: UsqueTheme.mono(context),
                      decoration: InputDecoration(
                        labelText: strings.get('bypass_cidrs'),
                        hintText: strings.get('bypass_hint'),
                        alignLabelWithHint: true,
                      ),
                      validator: _validateCidrs,
                    ),
                    const SizedBox(height: 18),
                    _GeoDirectPanel(
                      controller: widget.controller,
                      enabled: _geoEnabled,
                      search: _geoSearch,
                      onEnabledChanged: (next) =>
                          setState(() => _geoEnabled = next),
                    ),
                  ],
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  String _ipPolicyLabel(IpPolicy value) {
    final strings = widget.controller.strings;
    return strings.get(switch (value) {
      IpPolicy.automatic => 'automatic',
      IpPolicy.preferIpv4 => 'prefer_ipv4',
      IpPolicy.preferIpv6 => 'prefer_ipv6',
      IpPolicy.ipv4Only => 'ipv4_only',
      IpPolicy.ipv6Only => 'ipv6_only',
    });
  }

  String? _validateIp(String? value, InternetAddressType expected) {
    final address = InternetAddress.tryParse(value?.trim() ?? '');
    return address == null || address.type != expected
        ? widget.controller.strings.get('invalid_address')
        : null;
  }

  String? _validatePort(String? value) {
    final port = int.tryParse(value ?? '');
    return port == null || port < 1 || port > 65535 ? '1–65535' : null;
  }

  String? _validateMtu(String? value) {
    final mtu = int.tryParse(value ?? '');
    return mtu == null || mtu < 1280 || mtu > 9000 ? '1280–9000' : null;
  }

  String? _validateSni(String? value) {
    final normalized = value?.trim() ?? '';
    final valid = RegExp(
      r'^(?=.{1,253}$)(?!-)(?:[a-zA-Z0-9-]{1,63}\.)+[a-zA-Z0-9-]{2,63}$',
    ).hasMatch(normalized);
    return valid ? null : widget.controller.strings.get('invalid_dns_name');
  }

  String? _validateCidrs(String? value) {
    final cidrs = (value ?? '')
        .split(RegExp(r'\r?\n'))
        .map((line) => line.trim())
        .where((line) => line.isNotEmpty);
    for (final cidr in cidrs) {
      final parts = cidr.split('/');
      final address = InternetAddress.tryParse(parts.first);
      final prefix = parts.length == 2 ? int.tryParse(parts[1]) : null;
      final maximum = address?.type == InternetAddressType.IPv4 ? 32 : 128;
      if (parts.length != 2 ||
          address == null ||
          prefix == null ||
          prefix < 0 ||
          prefix > maximum) {
        return '${widget.controller.strings.get('invalid_cidr')}: $cidr';
      }
    }
    return null;
  }

  void _save() {
    if (!(_formKey.currentState?.validate() ?? false)) {
      return;
    }
    final profile = widget.controller.activeProfile;
    final endpointManaged = _zeroTrustEndpointManaged;
    widget.controller.updateNetwork(
      profile.copyWith(
        transport: _transport,
        ipPolicy: _ipPolicy,
        endpointIpv4: endpointManaged
            ? profile.endpointIpv4
            : _endpointV4.text.trim(),
        endpointIpv6: endpointManaged
            ? profile.endpointIpv6
            : _endpointV6.text.trim(),
        endpointPort: endpointManaged
            ? profile.endpointPort
            : int.parse(_port.text),
        sni: endpointManaged ? profile.sni : _sni.text.trim(),
        mtu: int.parse(_mtu.text),
        dnsIpv4: _dnsV4.text.trim(),
        dnsIpv6: _dnsV6.text.trim(),
        killSwitch: _killSwitch,
        allowLan: _allowLan,
        bypassCidrs: _bypass.text
            .split(RegExp(r'\r?\n'))
            .map((line) => line.trim())
            .where((line) => line.isNotEmpty)
            .toList(growable: false),
        geoDirectCountries: _orderedGeoCountries(_geoEnabled),
      ),
    );
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(widget.controller.strings.get('saved'))),
    );
  }

  Future<void> _reset() async {
    final strings = widget.controller.strings;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: LucideIcons.rotateCcw,
        title: strings.get('reset_defaults'),
        width: 420,
        content: Text(strings.get('reset_defaults_body')),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(strings.get('reset')),
          ),
        ],
      ),
    );
    if (!(confirmed ?? false)) {
      return;
    }
    final current = widget.controller.activeProfile;
    var reset = current.resetAdvancedDefaults();
    if (_zeroTrustEndpointManaged) {
      reset = reset.copyWith(
        endpointIpv4: current.endpointIpv4,
        endpointIpv6: current.endpointIpv6,
        endpointPort: current.endpointPort,
        sni: current.sni,
      );
    }
    widget.controller.updateNetwork(reset);
    setState(() => _load(reset));
  }
}

List<String> _orderedGeoCountries(Set<String> enabled) {
  final countries = enabled.toList();
  countries.sort();
  if (countries.remove('CN')) {
    countries.insert(0, 'CN');
  }
  return countries;
}

class _GeoDirectPanel extends StatefulWidget {
  const _GeoDirectPanel({
    required this.controller,
    required this.enabled,
    required this.search,
    required this.onEnabledChanged,
  });

  final AppController controller;
  final Set<String> enabled;
  final TextEditingController search;
  final ValueChanged<Set<String>> onEnabledChanged;

  @override
  State<_GeoDirectPanel> createState() => _GeoDirectPanelState();
}

class _GeoDirectPanelState extends State<_GeoDirectPanel> {
  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.controller,
      builder: (context, _) => _buildPanel(context),
    );
  }

  Widget _buildPanel(BuildContext context) {
    final controller = widget.controller;
    final enabled = widget.enabled;
    final search = widget.search;
    final strings = controller.strings;
    final progress = controller.geoProgress;
    final updating = progress != null && progress.total > 0;
    final cached = controller.geoRules.hasCachedRules;
    final query = search.text.trim().toLowerCase();
    final byCode = <String, GeoRulesEntry>{
      for (final entry in controller.geoRules.entries) entry.countryCode: entry,
    };
    final countries = kIsoCountries.where((country) {
      if (query.isEmpty) {
        return true;
      }
      return country.code.toLowerCase().contains(query) ||
          country.name.toLowerCase().contains(query);
    }).toList(growable: false);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Text(
          strings.get('geo_direct'),
          style: Theme.of(context).textTheme.titleSmall,
        ),
        const SizedBox(height: 8),
        Text(
          strings.get('geo_direct_help'),
          style: Theme.of(context).textTheme.bodyMedium?.copyWith(
            color: Theme.of(context).colorScheme.onSurfaceVariant,
          ),
        ),
        const SizedBox(height: 12),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: <Widget>[
            FilledButton.tonalIcon(
              onPressed: !cached || updating || controller.busy
                  ? null
                  : controller.updateAllGeoRules,
              icon: const Icon(LucideIcons.refreshCw),
              label: Text(
                updating
                    ? strings
                          .get('geo_updating')
                          .replaceAll(
                            '{current}',
                            '${progress.completed}',
                          )
                          .replaceAll('{total}', '${progress.total}')
                    : strings.get('geo_update_all'),
              ),
            ),
            Text(
              _lastUpdateLabel(strings, controller.geoRules),
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ],
        ),
        const SizedBox(height: 12),
        TextField(
          controller: search,
          onChanged: (_) => setState(() {}),
          decoration: InputDecoration(
            labelText: strings.get('geo_search'),
            prefixIcon: const Icon(LucideIcons.search),
          ),
        ),
        const SizedBox(height: 8),
        ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 360),
          child: ListView.builder(
            shrinkWrap: true,
            itemCount: countries.length,
            itemBuilder: (context, index) {
              final country = countries[index];
              final entry = byCode[country.code];
              final hasGeoip = entry?.hasGeoip ?? false;
              final date = _entryDate(entry);
              return ListTile(
                contentPadding: EdgeInsets.zero,
                dense: true,
                title: Text('${country.code}  ${country.name}'),
                subtitle: Text(
                  hasGeoip
                      ? (date == null
                            ? strings.get('geo_downloaded')
                            : date)
                      : strings.get('geo_not_downloaded'),
                ),
                trailing: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: <Widget>[
                    IconButton(
                      tooltip: strings.get('geo_download'),
                      onPressed: updating || controller.busy
                          ? null
                          : () => controller.downloadGeoRules(country.code),
                      icon: const Icon(LucideIcons.download),
                    ),
                    if (hasGeoip)
                      IconButton(
                        tooltip: strings.get('geo_update'),
                        onPressed: updating || controller.busy
                            ? null
                            : () => controller.downloadGeoRules(country.code),
                        icon: const Icon(LucideIcons.refreshCw),
                      ),
                    Switch(
                      value: enabled.contains(country.code),
                      onChanged: hasGeoip
                          ? (value) {
                              final next = {...enabled};
                              if (value) {
                                next.add(country.code);
                              } else {
                                next.remove(country.code);
                              }
                              widget.onEnabledChanged(next);
                            }
                          : null,
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

  String _lastUpdateLabel(AppStrings strings, GeoRulesList rules) {
    if (rules.lastSuccessfulUpdateUnixMilliseconds <= 0) {
      return strings.get('geo_never_updated');
    }
    final time = DateTime.fromMillisecondsSinceEpoch(
      rules.lastSuccessfulUpdateUnixMilliseconds,
    );
    return strings.get('geo_last_updated').replaceAll(
      '{current}',
      time.toLocal().toString().split('.').first,
    );
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

class _ResponsiveFields extends StatelessWidget {
  const _ResponsiveFields({required this.children});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final width = constraints.maxWidth >= 620
            ? (constraints.maxWidth - 12) / 2
            : constraints.maxWidth;
        return Wrap(
          spacing: 12,
          runSpacing: 12,
          children: children
              .map((child) => SizedBox(width: width, child: child))
              .toList(growable: false),
        );
      },
    );
  }
}
