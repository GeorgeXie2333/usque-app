import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/usque_motion.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/context_help_button.dart';
import '../widgets/controller_selector.dart';
import '../widgets/direct_dns_editor.dart';
import '../widgets/save_changes_bar.dart';
import '../widgets/unsaved_changes_guard.dart';
import '../widgets/usque_dialog.dart';
import '../widgets/warp_dns_editor.dart';
import '../widgets/zero_trust_endpoint_warning.dart';

class AdvancedSettingsScreen extends StatefulWidget {
  const AdvancedSettingsScreen({
    required this.controller,
    this.revealKillSwitch = false,
    super.key,
  });

  final AppController controller;

  /// Scrolls the Kill Switch draft switch into view once the page is laid out.
  final bool revealKillSwitch;

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
  late TransportPolicy _transport;
  late DataPlaneMode _dataPlane;
  late CongestionControlAlgorithm _congestionControl;
  late IpPolicy _ipPolicy;
  late EndpointSelection _endpointSelection;
  late bool _killSwitch;
  late bool _allowLan;
  late bool _disableQuic;
  late DirectDnsSettings _directDns;
  late WarpDnsSettings _warpDns;
  late DnsMode _dnsMode;
  late ProxySettings _proxy;
  final _directDnsKey = GlobalKey<DirectDnsEditorState>();
  final _warpDnsKey = GlobalKey<WarpDnsEditorState>();
  int _dnsResetRevision = 0;
  final _killSwitchKey = GlobalKey();
  bool _saving = false;
  String? _endpointAcknowledgedAccount;
  bool _endpointWarningOpen = false;
  int _endpointEditGeneration = 0;
  String? _saveError;
  String? _validationError;
  bool _loading = false;
  bool _saved = false;
  bool _validationAttempted = false;
  List<Object> _baseline = [];
  late String _editingAccountId;
  final _fieldKeys = List.generate(
    7,
    (_) => GlobalKey<FormFieldState<String>>(),
  );
  final _focus = List.generate(9, (_) => FocusNode());

  List<Object> get _values => [
    !_zeroTrustEndpointIpsManaged &&
            _endpointSelection == EndpointSelection.automatic
        ? widget.controller.activeProfile.endpointIpv4
        : _endpointV4.text,
    !_zeroTrustEndpointIpsManaged &&
            _endpointSelection == EndpointSelection.automatic
        ? widget.controller.activeProfile.endpointIpv6
        : _endpointV6.text,
    _port.text,
    _sni.text,
    _warpDns.mode == WarpDnsMode.plain
        ? _dnsV4.text
        : widget.controller.activeProfile.dnsIpv4,
    _warpDns.mode == WarpDnsMode.plain
        ? _dnsV6.text
        : widget.controller.activeProfile.dnsIpv6,
    _mtu.text,
    _transport,
    _dataPlane,
    _congestionControl,
    _ipPolicy,
    _killSwitch,
    _allowLan,
    _disableQuic,
    _directDns,
    _dnsMode,
    _proxy.socksListeners.join('\n'),
    _proxy.httpListeners.join('\n'),
    _proxy.dnsMode,
    _proxy.dnsIpv4,
    _proxy.dnsIpv6,
    _proxy.systemProxy,
    _endpointSelection,
    _warpDns,
  ];
  bool get _dirty => !listEquals(_values, _baseline);
  void _edited() {
    if (_loading || _saving) return;
    setState(() {
      _saved = false;
      _validationError = null;
      _saveError = null;
    });
  }

  bool get _zeroTrustEndpointIpsManaged =>
      widget.controller
          .identityStatus(widget.controller.activeProfile.id)
          .provider ==
      IdentityProvider.zeroTrust;

  bool get _ztEndpointEditingSupported =>
      widget.controller.engineCapabilities?.zeroTrustEndpointEditing ?? false;

  bool get _ztRegisteredEndpointsAvailable {
    final status = widget.controller.identityStatus(
      widget.controller.activeProfile.id,
    );
    return InternetAddress.tryParse(status.registeredEndpointIpv4)?.type ==
            InternetAddressType.IPv4 &&
        InternetAddress.tryParse(status.registeredEndpointIpv6)?.type ==
            InternetAddressType.IPv6;
  }

  bool get _ztEndpointsUnlocked =>
      _ztEndpointEditingSupported &&
      _ztRegisteredEndpointsAvailable &&
      _endpointAcknowledgedAccount == widget.controller.activeProfile.id;

  void _accountChanged() {
    if (widget.controller.activeProfile.id != _editingAccountId) {
      _endpointAcknowledgedAccount = null;
      _endpointEditGeneration++;
    }
  }

  Future<void> _unlockZtEndpoints() async {
    if (_saving ||
        _endpointWarningOpen ||
        !_ztEndpointEditingSupported ||
        !_ztRegisteredEndpointsAvailable) {
      return;
    }
    final account = widget.controller.activeProfile.id;
    final generation = _endpointEditGeneration;
    if (account != _editingAccountId) return;
    _endpointWarningOpen = true;
    final confirmed = await showDialog<bool>(
      context: context,
      barrierDismissible: false,
      builder: (_) =>
          ZeroTrustEndpointWarning(strings: widget.controller.strings),
    );
    _endpointWarningOpen = false;
    if (!mounted ||
        confirmed != true ||
        generation != _endpointEditGeneration ||
        account != widget.controller.activeProfile.id ||
        !_zeroTrustEndpointIpsManaged ||
        !_ztEndpointEditingSupported ||
        !_ztRegisteredEndpointsAvailable) {
      return;
    }
    setState(() => _endpointAcknowledgedAccount = account);
    _focus[0].requestFocus();
  }

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
    _load(widget.controller.activeProfile);
    widget.controller.addListener(_accountChanged);
    if (widget.revealKillSwitch) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        final target = _killSwitchKey.currentContext;
        if (!mounted || target == null) return;
        Scrollable.ensureVisible(
          target,
          alignment: 0.2,
          duration: UsqueMotion.reduced(context)
              ? Duration.zero
              : UsqueMotion.gentle,
          curve: UsqueMotion.emphasized,
        );
      });
    }
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
    _transport = profile.transport;
    _dataPlane = profile.dataPlane;
    _congestionControl = profile.congestionControl;
    _ipPolicy = profile.ipPolicy;
    _endpointSelection = profile.endpointSelection;
    _killSwitch = profile.killSwitch;
    _allowLan = profile.allowLan;
    _disableQuic = profile.disableQuic;
    _directDns = profile.directDns;
    _warpDns = profile.warpDns;
    _dnsMode = profile.dnsMode;
    _proxy = profile.proxy;
    if (baseline) {
      _baseline = _values;
      _editingAccountId = profile.id;
    }
    _loading = false;
  }

  @override
  void dispose() {
    widget.controller.removeListener(_accountChanged);
    for (final controller in <TextEditingController>[
      _endpointV4,
      _endpointV6,
      _port,
      _sni,
      _mtu,
      _dnsV4,
      _dnsV6,
    ]) {
      controller.dispose();
    }
    for (final focus in _focus) {
      focus.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) =>
      ControllerSelector<
        ({
          EngineCapabilities? capabilities,
          bool managedQuic,
          String accountId,
          ProfileIdentityStatus identity,
        })
      >(
        controller: widget.controller,
        selector: (controller) {
          final profile = controller.activeProfile;
          return (
            capabilities: controller.engineCapabilities,
            accountId: profile.id,
            identity: controller.identityStatus(profile.id),
            managedQuic: profile.chainEnabled && profile.chainSource.isProxy,
          );
        },
        builder: (context, view) =>
            _buildSettings(context, managedQuic: view.managedQuic),
      );

  Widget _buildSettings(BuildContext context, {required bool managedQuic}) {
    final strings = widget.controller.strings;
    final l4Available =
        widget.controller.engineCapabilities?.l4Available ?? false;
    return UnsavedChangesGuard(
      strings: strings,
      dirty: _dirty,
      saving: _saving,
      child: SubPage(
        title: strings.get('advanced'),
        contentWidth: 880,
        subtitle: strings.get('shared_network_scope'),
        backLabel: strings.get('back'),
        actions: <Widget>[
          OutlinedButton.icon(
            onPressed: _saving ? null : _reset,
            icon: const Icon(LucideIcons.rotateCcw),
            label: Text(strings.get('reset_defaults')),
          ),
        ],
        bottomBar: AnimatedBuilder(
          animation: widget.controller,
          builder: (context, _) => SaveChangesBar(
            strings: strings,
            dirty: _dirty,
            saving: _saving,
            saved: _saved,
            statusLabel:
                !_dirty ||
                    widget.controller.networkSettings.unconfirmed ||
                    widget.controller.networkSettings.saveError != null
                ? widget.controller.networkSettingsMessage
                : null,
            onReconnect: widget.controller.networkSettingsCanReconnect
                ? widget.controller.retry
                : null,
            error: _saveError,
            validationError: _validationError,
            contentWidth: 880,
            matchPageGutter: true,
            onSave: _save,
          ),
        ),
        child: Form(
          key: _formKey,
          autovalidateMode: _validationAttempted
              ? AutovalidateMode.onUserInteraction
              : AutovalidateMode.disabled,
          // Everyday protection comes first; the caution introduces the
          // settings that can stop the tunnel from connecting.
          child: PanelStack(
            spacing: 32,
            children: <Widget>[
              _protectionSection(managedQuic: managedQuic),
              WarningBanner(
                title: null,
                message: strings.get('advanced_warning'),
              ),
              _warpDnsSection(),
              DirectDnsEditor(
                key: _directDnsKey,
                value: _directDns,
                resetRevision: _dnsResetRevision,
                enabled: !_saving,
                encryptedAvailable:
                    widget.controller.engineCapabilities?.encryptedDirectDns ??
                    false,
                strings: strings,
                onChanged: (value) => setState(() {
                  _directDns = value;
                  _saved = false;
                  _validationError = null;
                }),
              ),
              _endpointSection(),
              _transportSection(l4Available: l4Available),
            ],
          ),
        ),
      ),
    );
  }

  Widget _protectionSection({required bool managedQuic}) {
    final strings = widget.controller.strings;
    final quicBlocking =
        widget.controller.engineCapabilities?.applicationQuicBlocking == true;
    return ContentSection(
      icon: LucideIcons.shield,
      title: strings.get('routing_protection'),
      gap: 10,
      child: RowTileTheme(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            SwitchListTile(
              key: _killSwitchKey,
              contentPadding: EdgeInsets.zero,
              secondary: const Icon(LucideIcons.shieldCheck),
              title: Text(strings.get('kill_switch')),
              subtitle: Text(
                strings.get(
                  defaultTargetPlatform == TargetPlatform.android
                      ? 'kill_switch_help_android'
                      : 'kill_switch_help',
                ),
              ),
              value: _killSwitch,
              onChanged: _saving
                  ? null
                  : (value) => setState(() => _killSwitch = value),
            ),
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              secondary: const Icon(LucideIcons.house),
              title: Text(strings.get('allow_lan')),
              value: _allowLan,
              onChanged: _saving
                  ? null
                  : (value) => setState(() => _allowLan = value),
            ),
            SwitchListTile(
              key: const ValueKey('disable-quic-switch'),
              focusNode: _focus[8],
              contentPadding: EdgeInsets.zero,
              secondary: const Icon(LucideIcons.ban),
              title: Text(strings.get('disable_quic')),
              subtitle: Text(
                managedQuic
                    ? strings.get('disable_quic_managed')
                    : quicBlocking
                    ? strings.get('disable_quic_help')
                    : strings.get('disable_quic_unsupported'),
              ),
              // The exit policy is effective without changing the
              // user's saved preference or this page's manual draft.
              value: managedQuic || _disableQuic,
              onChanged: _saving || managedQuic || !quicBlocking
                  ? null
                  : (value) => setState(() {
                      _disableQuic = value;
                      _saved = false;
                      _validationError = null;
                      _saveError = null;
                    }),
            ),
          ],
        ),
      ),
    );
  }

  Widget _warpDnsSection() {
    final strings = widget.controller.strings;
    return ContentSection(
      icon: LucideIcons.globeLock,
      title: strings.get('warp_dns_type'),
      gap: 20,
      children: <Widget>[
        WarpDnsEditor(
          key: _warpDnsKey,
          value: _warpDns,
          resetRevision: _dnsResetRevision,
          enabled: !_saving,
          encryptedAvailable:
              widget.controller.engineCapabilities?.encryptedWarpDns ?? false,
          strings: strings,
          onChanged: (value) => setState(() {
            _warpDns = value;
            _saved = false;
            _validationError = null;
            _saveError = null;
          }),
        ),
        if (_warpDns.mode == WarpDnsMode.plain) ...<Widget>[
          const SizedBox(height: 12),
          _ResponsiveFields(
            children: <Widget>[
              TextFormField(
                key: _fieldKeys[4],
                focusNode: _focus[4],
                enabled: !_saving,
                controller: _dnsV4,
                onChanged: (_) => _edited(),
                decoration: InputDecoration(labelText: strings.get('dns_ipv4')),
                validator: (value) =>
                    _validateIp(value, InternetAddressType.IPv4),
              ),
              TextFormField(
                key: _fieldKeys[5],
                focusNode: _focus[5],
                enabled: !_saving,
                controller: _dnsV6,
                onChanged: (_) => _edited(),
                decoration: InputDecoration(labelText: strings.get('dns_ipv6')),
                validator: (value) =>
                    _validateIp(value, InternetAddressType.IPv6),
              ),
            ],
          ),
        ],
      ],
    );
  }

  Widget _endpointSection() {
    final strings = widget.controller.strings;
    final automaticAvailable =
        widget.controller.engineCapabilities?.automaticEndpoints ?? false;
    final managed = _zeroTrustEndpointIpsManaged;
    final addresses = managed || _endpointSelection == EndpointSelection.custom;
    return ContentSection(
      icon: LucideIcons.mapPin,
      title: strings.get('endpoint_section'),
      gap: 20,
      children: <Widget>[
        if (!managed) ...<Widget>[
          LayoutBuilder(
            // Two short choices fit side by side on a phone; only very
            // narrow layouts and large text stack them.
            builder: (context, constraints) => Semantics(
              container: true,
              label: strings.get('endpoint_selection'),
              child: SegmentedButton<EndpointSelection>(
                key: const ValueKey('endpoint-selection'),
                direction:
                    constraints.maxWidth < 300 ||
                        MediaQuery.textScalerOf(context).scale(1) > 1.3
                    ? Axis.vertical
                    : Axis.horizontal,
                segments: [
                  ButtonSegment(
                    value: EndpointSelection.automatic,
                    label: Text(strings.get('endpoint_automatic')),
                    enabled: automaticAvailable,
                  ),
                  ButtonSegment(
                    value: EndpointSelection.custom,
                    label: Text(strings.get('endpoint_custom')),
                  ),
                ],
                selected: {_endpointSelection},
                onSelectionChanged: _saving
                    ? null
                    : (values) {
                        setState(() {
                          _endpointSelection = values.first;
                        });
                        _edited();
                      },
                showSelectedIcon: false,
              ),
            ),
          ),
          const SizedBox(height: 8),
          HintText(
            automaticAvailable
                ? strings.get(
                    _endpointSelection == EndpointSelection.automatic
                        ? 'endpoint_automatic_help'
                        : 'endpoint_custom_help',
                  )
                : strings.get('endpoint_unsupported'),
          ),
          const SizedBox(height: 16),
        ],
        if (managed && !_ztEndpointsUnlocked) ...<Widget>[
          if (_ztEndpointEditingSupported && _ztRegisteredEndpointsAvailable)
            Align(
              alignment: AlignmentDirectional.centerStart,
              child: OutlinedButton.icon(
                key: const ValueKey('zt-endpoint-edit'),
                onPressed: _saving ? null : _unlockZtEndpoints,
                icon: const Icon(LucideIcons.pencil),
                label: Text(strings.get('zero_trust_endpoint_edit')),
              ),
            ),
          const SizedBox(height: 8),
          HintText(
            strings.get(
              !_ztEndpointEditingSupported
                  ? 'zero_trust_endpoint_unsupported'
                  : !_ztRegisteredEndpointsAvailable
                  ? 'zero_trust_metadata_missing'
                  : 'zero_trust_endpoint_risk_locked',
            ),
          ),
          const SizedBox(height: 16),
        ],
        _ResponsiveFields(
          children: <Widget>[
            if (addresses)
              TextFormField(
                key: _fieldKeys[0],
                focusNode: _focus[0],
                enabled: !_saving,
                controller: _endpointV4,
                onChanged: (_) => _edited(),
                readOnly: managed && !_ztEndpointsUnlocked,
                decoration: InputDecoration(
                  labelText: strings.get('endpoint_ipv4'),
                ),
                validator: (value) =>
                    _validateIp(value, InternetAddressType.IPv4),
              ),
            if (addresses)
              TextFormField(
                key: _fieldKeys[1],
                focusNode: _focus[1],
                enabled: !_saving,
                controller: _endpointV6,
                onChanged: (_) => _edited(),
                readOnly: managed && !_ztEndpointsUnlocked,
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
              decoration: InputDecoration(labelText: strings.get('port')),
              validator: _validatePort,
            ),
            if (_dataPlane == DataPlaneMode.l4Proxy)
              TextFormField(
                key: ValueKey(managed),
                initialValue: managed
                    ? 'zt-masque-proxy.cloudflareclient.com'
                    : 'consumer-masque-proxy.cloudflareclient.com',
                readOnly: true,
                decoration: InputDecoration(
                  labelText: strings.get('sni'),
                  helperText: strings.get('l4_sni_identity'),
                  helperMaxLines: 6,
                ),
              )
            else
              TextFormField(
                key: _fieldKeys[3],
                focusNode: _focus[3],
                enabled: !_saving,
                controller: _sni,
                onChanged: (_) => _edited(),
                keyboardType: TextInputType.url,
                decoration: InputDecoration(labelText: strings.get('sni')),
                validator: _validateSni,
              ),
          ],
        ),
        const SizedBox(height: 12),
        DropdownButtonFormField<IpPolicy>(
          style: FieldDropdown.valueStyle(context),
          iconSize: FieldDropdown.iconSize,
          initialValue: _ipPolicy,
          isExpanded: true,
          decoration: FieldDropdown.decoration(
            context,
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
      ],
    );
  }

  Widget _transportSection({required bool l4Available}) {
    final strings = widget.controller.strings;
    return ContentSection(
      icon: LucideIcons.cable,
      title: strings.get('transport'),
      gap: 20,
      children: <Widget>[
        LayoutBuilder(
          builder: (context, constraints) => SegmentedButton<String>(
            direction:
                constraints.maxWidth < 560 ||
                    MediaQuery.textScalerOf(context).scale(1) > 1.3
                ? Axis.vertical
                : Axis.horizontal,
            segments: <ButtonSegment<String>>[
              ButtonSegment(
                value: 'automatic',
                label: Text(strings.get('automatic')),
              ),
              ButtonSegment(value: 'http3', label: Text(strings.get('http3'))),
              ButtonSegment(value: 'http2', label: Text(strings.get('http2'))),
              ButtonSegment(
                value: 'l4',
                label: Text(strings.get('l4_mode')),
                enabled: l4Available,
                tooltip: l4Available ? null : strings.get('l4_unsupported'),
              ),
            ],
            selected: <String>{
              _dataPlane == DataPlaneMode.l4Proxy ? 'l4' : _transport.name,
            },
            onSelectionChanged: _saving
                ? null
                : (selection) {
                    setState(() {
                      if (selection.first == 'l4') {
                        _dataPlane = DataPlaneMode.l4Proxy;
                      } else {
                        _dataPlane = DataPlaneMode.connectIp;
                        _transport = TransportPolicy.values.byName(
                          selection.first,
                        );
                      }
                      _saved = false;
                      _validationError = null;
                      _saveError = null;
                    });
                  },
            showSelectedIcon: false,
          ),
        ),
        if (_dataPlane == DataPlaneMode.l4Proxy) ...<Widget>[
          const SizedBox(height: 4),
          // The help button follows its sentence instead of the row's end.
          Row(
            children: <Widget>[
              Flexible(
                child: Semantics(
                  key: const ValueKey('l4-transport-hint'),
                  liveRegion: true,
                  child: HintText(strings.get('l4_transport_hint')),
                ),
              ),
              ContextHelpButton(
                title: strings.get('l4_mode'),
                message: strings.get('l4_explanation'),
                strings: strings,
              ),
            ],
          ),
          if (!l4Available) HintText(strings.get('l4_unsupported')),
        ],
        const SizedBox(height: 16),
        _transportTuning(),
      ],
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

  /// Congestion control and MTU share a row; the congestion-control status
  /// follows the live session, so this part rebuilds with the controller.
  Widget _transportTuning() => AnimatedBuilder(
    animation: widget.controller,
    builder: (context, _) {
      final controller = widget.controller;
      final strings = controller.strings;
      final algorithms =
          controller.engineCapabilities?.h3CongestionControlAlgorithms ??
          const <CongestionControlAlgorithm>[];
      final session = controller.snapshot.sessionCongestionControl;
      final pending =
          session != null &&
          session != controller.sharedNetwork.congestionControl;
      final h2 =
          (_dataPlane == DataPlaneMode.connectIp &&
              _transport == TransportPolicy.http2) ||
          const [
            'h2',
            'http2',
            'http/2',
          ].contains(controller.snapshot.transport?.toLowerCase());
      final hint = algorithms.isEmpty
          ? strings.get('cc_upgrade')
          : h2
          ? strings.get('cc_h2')
          : pending
          ? strings.get('cc_pending')
          : strings.get('cc_help');
      final ccPicker = DropdownButtonFormField<CongestionControlAlgorithm>(
        style: FieldDropdown.valueStyle(context),
        iconSize: FieldDropdown.iconSize,
        key: const ValueKey('congestion-control'),
        initialValue: _congestionControl,
        isExpanded: true,
        decoration: FieldDropdown.decoration(
          context,
          labelText: strings.get('cc_label'),
        ),
        items:
            const [
                  CongestionControlAlgorithm.cubic,
                  CongestionControlAlgorithm.bbr,
                  CongestionControlAlgorithm.bbr3,
                  CongestionControlAlgorithm.reno,
                ]
                .map(
                  (algorithm) => DropdownMenuItem(
                    value: algorithm,
                    enabled: algorithms.contains(algorithm),
                    child: Text(algorithm.label),
                  ),
                )
                .toList(),
        onChanged:
            _saving ||
                (_dataPlane == DataPlaneMode.connectIp &&
                    _transport == TransportPolicy.http2) ||
                algorithms.isEmpty
            ? null
            : (value) {
                if (value == null) return;
                setState(() => _congestionControl = value);
                _edited();
              },
      );
      final mtu = TextFormField(
        key: _fieldKeys[6],
        focusNode: _focus[6],
        enabled: !_saving,
        controller: _mtu,
        onChanged: (_) => _edited(),
        keyboardType: TextInputType.number,
        inputFormatters: <TextInputFormatter>[
          FilteringTextInputFormatter.digitsOnly,
        ],
        decoration: InputDecoration(labelText: strings.get('mtu')),
        validator: _validateMtu,
      );
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _ResponsiveFields(children: [ccPicker, mtu]),
          const SizedBox(height: 8),
          Semantics(
            liveRegion: true,
            child: HintText(
              hint,
              key: pending && !h2 && algorithms.isNotEmpty
                  ? const ValueKey('congestion-control-pending')
                  : null,
            ),
          ),
        ],
      );
    },
  );

  String? _validateSni(String? value) {
    final normalized = value?.trim() ?? '';
    final valid = RegExp(
      r'^(?=.{1,253}$)(?!-)(?:[a-zA-Z0-9-]{1,63}\.)+[a-zA-Z0-9-]{2,63}$',
    ).hasMatch(normalized);
    return valid ? null : widget.controller.strings.get('invalid_dns_name');
  }

  Future<void> _save() async {
    if (_saving) return;
    if (_zeroTrustEndpointIpsManaged &&
        _ztEndpointEditingSupported &&
        _ztRegisteredEndpointsAvailable &&
        !_ztEndpointsUnlocked &&
        (_values[0] != _baseline[0] || _values[1] != _baseline[1])) {
      final status = widget.controller.identityStatus(
        widget.controller.activeProfile.id,
      );
      final restoringRegistered =
          _endpointV4.text.trim() == status.registeredEndpointIpv4 &&
          _endpointV6.text.trim() == status.registeredEndpointIpv6;
      if (!restoringRegistered) {
        await _unlockZtEndpoints();
        if (!mounted || !_ztEndpointsUnlocked) return;
      }
    }
    if (_dataPlane == DataPlaneMode.l4Proxy &&
        !(widget.controller.engineCapabilities?.l4Available ?? false)) {
      setState(
        () =>
            _validationError = widget.controller.strings.get('l4_unsupported'),
      );
      return;
    }
    if (_warpDns.mode != WarpDnsMode.plain &&
        !(widget.controller.engineCapabilities?.encryptedWarpDns ?? false)) {
      setState(
        () => _validationError = widget.controller.strings.get(
          'warp_dns_unsupported',
        ),
      );
      return;
    }
    setState(() => _validationAttempted = true);
    if (!(_formKey.currentState?.validate() ?? false)) {
      setState(
        () => _validationError = widget.controller.strings.get('form_errors'),
      );
      bool focusField(int index) {
        if (!(_fieldKeys[index].currentState?.hasError ?? false)) return false;
        _focus[index].requestFocus();
        unawaited(Scrollable.ensureVisible(_fieldKeys[index].currentContext!));
        return true;
      }

      // Page order: WARP DNS, Direct DNS, endpoint, then MTU under Transport.
      if (focusField(4) || focusField(5)) return;
      if (_warpDnsKey.currentState?.focusFirstError() ?? false) return;
      if (_directDnsKey.currentState?.focusFirstError() ?? false) return;
      for (final index in const [0, 1, 2, 3, 6]) {
        if (focusField(index)) return;
      }
      return;
    }
    FocusScope.of(context).unfocus();
    setState(() {
      _saved = false;
      _validationError = null;
      _saving = true;
      _saveError = null;
    });
    final profile = widget.controller.activeProfile;
    final endpointIpsManaged = _zeroTrustEndpointIpsManaged;
    final preserveEndpointIps = endpointIpsManaged
        ? !_ztEndpointEditingSupported || !_ztRegisteredEndpointsAvailable
        : _endpointSelection == EndpointSelection.automatic;
    const paths = [
      'endpoint.ipv4',
      'endpoint.ipv6',
      'endpoint.port',
      'endpoint.sni',
      'dns_servers',
      'dns_servers',
      'mtu',
      'transport',
      'data_plane',
      'congestion_control',
      'ip_policy',
      'kill_switch',
      'allow_lan',
      'disable_quic',
      'direct_dns',
      'dns_mode',
      'proxy.socks5_listeners',
      'proxy.http_listeners',
      'proxy.dns_mode',
      'proxy.dns_servers',
      'proxy.dns_servers',
      'proxy.system_proxy',
      'endpoint.selection',
      'warp_dns',
    ];
    final changedFields = <String>{
      for (var i = 0; i < paths.length; i++)
        if (_values[i] != _baseline[i] &&
            !(i < 2 &&
                (endpointIpsManaged
                    ? !_ztEndpointEditingSupported ||
                          !_ztRegisteredEndpointsAvailable
                    : _endpointSelection == EndpointSelection.automatic)) &&
            !(endpointIpsManaged && paths[i] == 'endpoint.selection') &&
            !(_warpDns.mode != WarpDnsMode.plain && paths[i] == 'dns_servers'))
          paths[i],
    }.toList();
    final saved = await widget.controller.saveNetwork(
      profile.copyWith(
        id: _editingAccountId,
        transport: _transport,
        dataPlane: _dataPlane,
        congestionControl: _congestionControl,
        ipPolicy: _ipPolicy,
        endpointSelection: endpointIpsManaged
            ? EndpointSelection.custom
            : _endpointSelection,
        endpointIpv4: preserveEndpointIps
            ? profile.endpointIpv4
            : _endpointV4.text.trim(),
        endpointIpv6: preserveEndpointIps
            ? profile.endpointIpv6
            : _endpointV6.text.trim(),
        endpointPort: int.parse(_port.text),
        sni: _sni.text.trim(),
        mtu: int.parse(_mtu.text),
        dnsIpv4: _warpDns.mode == WarpDnsMode.plain
            ? _dnsV4.text.trim()
            : profile.dnsIpv4,
        dnsIpv6: _warpDns.mode == WarpDnsMode.plain
            ? _dnsV6.text.trim()
            : profile.dnsIpv6,
        killSwitch: _killSwitch,
        allowLan: _allowLan,
        disableQuic: _disableQuic,
        directDns: _directDns,
        warpDns: _warpDns,
        dnsMode: _dnsMode,
        proxy: _proxy.copyWith(authUsername: profile.proxy.authUsername),
      ),
      changedFields: changedFields,
    );
    if (!mounted) return;
    setState(() {
      _saving = false;
      _saveError = saved
          ? null
          : widget.controller.strings.get('changes_failed');
      _saved = saved;
      if (saved) {
        // A mode change can select a compatible DNS method in the same save.
        // Keep that confirmed dependency in later, unrelated form submissions.
        _proxy = _proxy.copyWith(
          dnsMode: widget.controller.activeProfile.proxy.dnsMode,
        );
        _baseline = _values;
      }
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
      final status = widget.controller.identityStatus(current.id);
      final restoreRegistered =
          _ztEndpointEditingSupported && _ztRegisteredEndpointsAvailable;
      reset = reset.copyWith(
        endpointSelection: EndpointSelection.custom,
        endpointIpv4: restoreRegistered
            ? status.registeredEndpointIpv4
            : current.endpointIpv4,
        endpointIpv6: restoreRegistered
            ? status.registeredEndpointIpv6
            : current.endpointIpv6,
      );
    }
    setState(() {
      _load(reset, baseline: false);
      _dnsResetRevision++;
      _saved = false;
      _validationError = null;
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
