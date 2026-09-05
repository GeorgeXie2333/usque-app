import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/direct_dns_editor.dart';
import '../widgets/save_changes_bar.dart';
import '../widgets/unsaved_changes_guard.dart';
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
  late TransportPolicy _transport;
  late IpPolicy _ipPolicy;
  late bool _killSwitch;
  late bool _allowLan;
  late DirectDnsSettings _directDns;
  final _directDnsKey = GlobalKey<DirectDnsEditorState>();
  bool _saving = false;
  String? _saveError;
  bool _loading = false;
  bool _saved = false;
  bool _validationAttempted = false;
  List<Object> _baseline = [];
  final _fieldKeys = List.generate(
    8,
    (_) => GlobalKey<FormFieldState<String>>(),
  );
  final _focus = List.generate(8, (_) => FocusNode());

  List<Object> get _values => [
    _endpointV4.text,
    _endpointV6.text,
    _port.text,
    _sni.text,
    _dnsV4.text,
    _dnsV6.text,
    _mtu.text,
    _bypass.text,
    _transport,
    _ipPolicy,
    _killSwitch,
    _allowLan,
    _directDns,
  ];
  bool get _dirty => !listEquals(_values, _baseline);
  void _edited() {
    if (_loading || _saving) return;
    setState(() {
      _saved = false;
      _saveError = null;
    });
  }

  bool get _zeroTrustEndpointIpsManaged =>
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
    _load(widget.controller.activeProfile);
  }

  void _load(UsqueProfile profile, {bool baseline = true}) {
    _loading = true;
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
    _directDns = profile.directDns;
    if (baseline) _baseline = _values;
    _loading = false;
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
    ]) {
      controller.dispose();
    }
    for (final focus in _focus) {
      focus.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final strings = widget.controller.strings;
    return UnsavedChangesGuard(
      strings: strings,
      dirty: _dirty,
      saving: _saving,
      child: SubPage(
        title: strings.get('advanced'),
        subtitle: strings.get('advanced_subtitle'),
        backLabel: strings.get('back'),
        actions: <Widget>[
          OutlinedButton.icon(
            onPressed: _saving ? null : _reset,
            icon: const Icon(LucideIcons.rotateCcw),
            label: Text(strings.get('reset_defaults')),
          ),
        ],
        bottomBar: SaveChangesBar(
          strings: strings,
          dirty: _dirty,
          saving: _saving,
          saved: _saved,
          error: _saveError,
          onSave: _save,
        ),
        child: Form(
          key: _formKey,
          autovalidateMode: _validationAttempted
              ? AutovalidateMode.onUserInteraction
              : AutovalidateMode.disabled,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: <Widget>[
              Text(strings.get('shared_network_scope')),
              const SizedBox(height: 12),
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
                        onSelectionChanged: _saving
                            ? null
                            : (selection) =>
                                  setState(() => _transport = selection.first),
                        showSelectedIcon: false,
                      ),
                      const SizedBox(height: 18),
                      _ResponsiveFields(
                        children: <Widget>[
                          TextFormField(
                            key: _fieldKeys[0],
                            focusNode: _focus[0],
                            enabled: !_saving,
                            controller: _endpointV4,
                            onChanged: (_) => _edited(),
                            readOnly: _zeroTrustEndpointIpsManaged,
                            decoration: InputDecoration(
                              labelText: strings.get('endpoint_ipv4'),
                            ),
                            validator: (value) =>
                                _validateIp(value, InternetAddressType.IPv4),
                          ),
                          TextFormField(
                            key: _fieldKeys[1],
                            focusNode: _focus[1],
                            enabled: !_saving,
                            controller: _endpointV6,
                            onChanged: (_) => _edited(),
                            readOnly: _zeroTrustEndpointIpsManaged,
                            decoration: InputDecoration(
                              labelText: strings.get('endpoint_ipv6'),
                            ),
                            validator: (value) =>
                                _validateIp(value, InternetAddressType.IPv6),
                          ),
                          TextFormField(
                            key: _fieldKeys[2],
                            focusNode: _focus[2],
                            enabled: !_saving,
                            controller: _port,
                            onChanged: (_) => _edited(),
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
                            key: _fieldKeys[3],
                            focusNode: _focus[3],
                            enabled: !_saving,
                            controller: _sni,
                            onChanged: (_) => _edited(),
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
                        isExpanded: true,
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
                        onChanged: _saving
                            ? null
                            : (value) {
                                if (value != null) {
                                  setState(() => _ipPolicy = value);
                                }
                              },
                      ),
                      const SizedBox(height: 14),
                      _ResponsiveFields(
                        children: <Widget>[
                          TextFormField(
                            key: _fieldKeys[4],
                            focusNode: _focus[4],
                            enabled: !_saving,
                            controller: _dnsV4,
                            onChanged: (_) => _edited(),
                            decoration: InputDecoration(
                              labelText: strings.get('dns_ipv4'),
                            ),
                            validator: (value) =>
                                _validateIp(value, InternetAddressType.IPv4),
                          ),
                          TextFormField(
                            key: _fieldKeys[5],
                            focusNode: _focus[5],
                            enabled: !_saving,
                            controller: _dnsV6,
                            onChanged: (_) => _edited(),
                            decoration: InputDecoration(
                              labelText: strings.get('dns_ipv6'),
                            ),
                            validator: (value) =>
                                _validateIp(value, InternetAddressType.IPv6),
                          ),
                          TextFormField(
                            key: _fieldKeys[6],
                            focusNode: _focus[6],
                            enabled: !_saving,
                            controller: _mtu,
                            onChanged: (_) => _edited(),
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
                        onChanged: _saving
                            ? null
                            : (value) => setState(() => _killSwitch = value),
                      ),
                      SwitchListTile(
                        contentPadding: EdgeInsets.zero,
                        title: Text(strings.get('allow_lan')),
                        value: _allowLan,
                        onChanged: _saving
                            ? null
                            : (value) => setState(() => _allowLan = value),
                      ),
                      const SizedBox(height: 14),
                      TextFormField(
                        key: _fieldKeys[7],
                        focusNode: _focus[7],
                        enabled: !_saving,
                        controller: _bypass,
                        onChanged: (_) => _edited(),
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
                    ],
                  ),
                  DirectDnsEditor(
                    key: _directDnsKey,
                    value: _directDns,
                    enabled: !_saving,
                    encryptedAvailable:
                        widget
                            .controller
                            .engineCapabilities
                            ?.encryptedDirectDns ??
                        false,
                    strings: strings,
                    onChanged: (value) => setState(() {
                      _directDns = value;
                      _saved = false;
                    }),
                  ),
                ],
              ),
            ],
          ),
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

  Future<void> _save() async {
    if (_saving) return;
    setState(() => _validationAttempted = true);
    if (!(_formKey.currentState?.validate() ?? false)) {
      setState(() => _saveError = widget.controller.strings.get('form_errors'));
      for (var i = 0; i < _fieldKeys.length; i++) {
        if (_fieldKeys[i].currentState?.hasError ?? false) {
          _focus[i].requestFocus();
          unawaited(Scrollable.ensureVisible(_fieldKeys[i].currentContext!));
          return;
        }
      }
      _directDnsKey.currentState?.focusFirstError();
      return;
    }
    FocusScope.of(context).unfocus();
    setState(() {
      _saved = false;
      _saving = true;
      _saveError = null;
    });
    final profile = widget.controller.activeProfile;
    final endpointIpsManaged = _zeroTrustEndpointIpsManaged;
    final saved = await widget.controller.saveNetwork(
      profile.copyWith(
        transport: _transport,
        ipPolicy: _ipPolicy,
        endpointIpv4: endpointIpsManaged
            ? profile.endpointIpv4
            : _endpointV4.text.trim(),
        endpointIpv6: endpointIpsManaged
            ? profile.endpointIpv6
            : _endpointV6.text.trim(),
        endpointPort: int.parse(_port.text),
        sni: _sni.text.trim(),
        mtu: int.parse(_mtu.text),
        dnsIpv4: _dnsV4.text.trim(),
        dnsIpv6: _dnsV6.text.trim(),
        killSwitch: _killSwitch,
        allowLan: _allowLan,
        directDns: _directDns,
        bypassCidrs: _bypass.text
            .split(RegExp(r'\r?\n'))
            .map((line) => line.trim())
            .where((line) => line.isNotEmpty)
            .toList(growable: false),
      ),
    );
    if (!mounted) return;
    setState(() {
      _saving = false;
      _saveError = saved
          ? null
          : widget.controller.strings.get('changes_failed');
      _saved = saved;
      if (saved) _baseline = _values;
    });
    if (!saved) return;
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
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(strings.get('reset_defaults_body')),
            const SizedBox(height: 12),
            Text(strings.get('reset_draft_hint')),
          ],
        ),
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
    if (!mounted || !(confirmed ?? false)) {
      return;
    }
    final current = widget.controller.activeProfile;
    var reset = current.resetAdvancedDefaults();
    if (_zeroTrustEndpointIpsManaged) {
      reset = reset.copyWith(
        endpointIpv4: current.endpointIpv4,
        endpointIpv6: current.endpointIpv6,
      );
    }
    setState(() {
      _load(reset, baseline: false);
      _saved = false;
      _saveError = null;
    });
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
