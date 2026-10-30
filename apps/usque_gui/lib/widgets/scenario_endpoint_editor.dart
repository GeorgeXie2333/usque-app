import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import 'common.dart';
import 'endpoint_fields.dart';
import 'zero_trust_endpoint_warning.dart';

/// A pure scene draft. It has no controller, engine or global save operation.
class ScenarioEndpointEditor extends StatefulWidget {
  const ScenarioEndpointEditor({
    required this.value,
    required this.account,
    required this.identity,
    required this.capabilities,
    required this.strings,
    required this.onChanged,
    required this.enabled,
    super.key,
  });
  final UsqueProfile value, account;
  final ProfileIdentityStatus identity;
  final EngineCapabilities? capabilities;
  final AppStrings strings;
  final ValueChanged<UsqueProfile> onChanged;
  final bool enabled;

  @override
  State<ScenarioEndpointEditor> createState() => ScenarioEndpointEditorState();
}

class ScenarioEndpointEditorState extends State<ScenarioEndpointEditor> {
  late final TextEditingController _v4, _v6, _port, _sni;
  late final String _savedV4, _savedV6;
  late EndpointSelection _selection;
  late EndpointPresetRegion _region;
  late bool _preset;
  bool _unlocked = false, _dialogOpen = false;
  final _v4Focus = FocusNode();
  bool get _managed => widget.identity.provider == IdentityProvider.zeroTrust;
  bool get _usingPreset =>
      _preset && (_managed || _selection == EndpointSelection.custom);
  bool get _registered =>
      InternetAddress.tryParse(widget.identity.registeredEndpointIpv4)?.type ==
          InternetAddressType.IPv4 &&
      InternetAddress.tryParse(widget.identity.registeredEndpointIpv6)?.type ==
          InternetAddressType.IPv6;
  bool get _ztSupported =>
      widget.capabilities?.zeroTrustEndpointEditing == true;
  bool get _verified => _region.hostname(widget.identity) != null;
  bool get _ipv6Policy =>
      widget.value.ipPolicy == IpPolicy.ipv6Only ||
      widget.value.ipPolicy == IpPolicy.preferIpv6;

  @override
  void initState() {
    super.initState();
    final value = widget.value;
    final inherited =
        _managed &&
        (value.endpointSelection == EndpointSelection.automatic ||
            value.endpointPresetRegion != null);
    _savedV4 = inherited ? widget.account.endpointIpv4 : value.endpointIpv4;
    _savedV6 = inherited ? widget.account.endpointIpv6 : value.endpointIpv6;
    _v4 = TextEditingController(text: _savedV4);
    _v6 = TextEditingController(text: _savedV6);
    _port = TextEditingController(text: '${value.endpointPort}');
    _sni = TextEditingController(text: value.sni);
    _selection = _managed ? EndpointSelection.custom : value.endpointSelection;
    _preset = value.endpointPresetRegion != null;
    _region = value.endpointPresetRegion ?? EndpointPresetRegion.us;
  }

  @override
  void didUpdateWidget(ScenarioEndpointEditor oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.identity != widget.identity ||
        oldWidget.account.id != widget.account.id) {
      _unlocked = false;
    }
  }

  @override
  void dispose() {
    for (final controller in [_v4, _v6, _port, _sni]) {
      controller.dispose();
    }
    _v4Focus.dispose();
    super.dispose();
  }

  void _emit({IpPolicy? policy}) {
    final preserve =
        _usingPreset ||
        _selection == EndpointSelection.automatic ||
        (_managed && (!_ztSupported || !_registered));
    widget.onChanged(
      widget.value.copyWith(
        endpointSelection: _selection,
        endpointPresetRegion: _usingPreset ? _region : null,
        clearEndpointPresetRegion: !_usingPreset,
        endpointIpv4: preserve ? _savedV4 : _v4.text.trim(),
        endpointIpv6: preserve ? _savedV6 : _v6.text.trim(),
        endpointPort: int.tryParse(_port.text) ?? 0,
        sni: _sni.text.trim(),
        ipPolicy: policy,
      ),
    );
  }

  Future<void> _presetMode(bool value) async {
    if (_dialogOpen || !widget.enabled || value == _preset) return;
    _dialogOpen = true;
    final identity = widget.identity;
    final policy = value && !_ipv6Policy
        ? await chooseEndpointPresetPolicy(context, widget.strings)
        : widget.value.ipPolicy;
    _dialogOpen = false;
    if (!mounted ||
        !widget.enabled ||
        policy == null ||
        identity != widget.identity) {
      return;
    }
    setState(() => _preset = value);
    _emit(policy: policy);
  }

  Future<void> _unlock() async {
    if (_dialogOpen || !widget.enabled || !_registered || !_ztSupported) return;
    _dialogOpen = true;
    final identity = widget.identity;
    final accepted = await showDialog<bool>(
      context: context,
      barrierDismissible: false,
      builder: (_) => ZeroTrustEndpointWarning(strings: widget.strings),
    );
    _dialogOpen = false;
    if (!mounted ||
        !widget.enabled ||
        accepted != true ||
        identity != widget.identity) {
      return;
    }
    setState(() => _unlocked = true);
    _v4Focus.requestFocus();
  }

  /// Presets require an IPv6 policy, including after a later policy edit.
  Future<bool> prepareSave() async {
    if (_dialogOpen) return false;
    if (_usingPreset) {
      if (widget.capabilities?.endpointPresets != true || !_verified) {
        return false;
      }
      if (!_ipv6Policy) {
        final identity = widget.identity;
        final policy = await chooseEndpointPresetPolicy(
          context,
          widget.strings,
        );
        if (!mounted || policy == null || identity != widget.identity) {
          return false;
        }
        _emit(policy: policy);
      }
    }
    return true;
  }

  @override
  Widget build(BuildContext context) {
    final s = widget.strings;
    final addresses =
        (_managed || _selection == EndpointSelection.custom) && !_usingPreset;
    TextFormField address(
      TextEditingController controller,
      InternetAddressType type,
      String key,
    ) => TextFormField(
      key: ValueKey('scenario-$key'),
      controller: controller,
      focusNode: type == InternetAddressType.IPv4 ? _v4Focus : null,
      enabled: widget.enabled,
      readOnly: _managed && !_unlocked,
      onChanged: (_) => _emit(),
      decoration: InputDecoration(labelText: s.get(key)),
      validator: (value) =>
          InternetAddress.tryParse(value?.trim() ?? '')?.type == type
          ? null
          : s.get('invalid_address'),
    );
    return EndpointFields(
      strings: s,
      selection: _selection,
      presetMode: _preset,
      presetRegion: _region,
      saving: !widget.enabled,
      automaticAvailable: widget.capabilities?.automaticEndpoints == true,
      managed: _managed,
      presetSupported: widget.capabilities?.endpointPresets == true,
      presetVerified: _verified,
      ztEditingSupported: _ztSupported,
      ztRegistered: _registered,
      ztUnlocked: _unlocked,
      onSelection: (value) {
        setState(() => _selection = value);
        _emit();
      },
      onPresetMode: _presetMode,
      onRegion: (value) {
        setState(() => _region = value);
        _emit();
      },
      onUnlock: _unlock,
      fields: ResponsiveEndpointFields(
        children: [
          if (_usingPreset)
            TextFormField(
              initialValue: _savedV4,
              readOnly: true,
              decoration: InputDecoration(
                labelText: s.get('endpoint_preset_fallback'),
              ),
            ),
          if (addresses)
            address(_v4, InternetAddressType.IPv4, 'endpoint_ipv4'),
          if (addresses)
            address(_v6, InternetAddressType.IPv6, 'endpoint_ipv6'),
          TextFormField(
            key: const ValueKey('scenario-endpoint-port'),
            controller: _port,
            enabled: widget.enabled,
            onChanged: (_) => _emit(),
            keyboardType: TextInputType.number,
            inputFormatters: [FilteringTextInputFormatter.digitsOnly],
            decoration: InputDecoration(labelText: s.get('port')),
            validator: (value) {
              final port = int.tryParse(value ?? '');
              return port != null && port > 0 && port <= 65535
                  ? null
                  : '1–65535';
            },
          ),
          if (widget.value.dataPlane == DataPlaneMode.l4Proxy)
            TextFormField(
              key: ValueKey(_managed),
              readOnly: true,
              initialValue: _managed
                  ? 'zt-masque-proxy.cloudflareclient.com'
                  : 'consumer-masque-proxy.cloudflareclient.com',
              decoration: InputDecoration(
                labelText: s.get('sni'),
                helperText: s.get('l4_sni_identity'),
                helperMaxLines: 6,
              ),
            )
          else
            TextFormField(
              key: const ValueKey('scenario-endpoint-sni'),
              controller: _sni,
              enabled: widget.enabled,
              onChanged: (_) => _emit(),
              keyboardType: TextInputType.url,
              decoration: InputDecoration(labelText: s.get('sni')),
              validator: (value) => validateEndpointSni(s, value),
            ),
        ],
      ),
      ipPolicyField: DropdownButtonFormField<IpPolicy>(
        key: ValueKey(widget.value.ipPolicy),
        initialValue: widget.value.ipPolicy,
        style: FieldDropdown.valueStyle(context),
        iconSize: FieldDropdown.iconSize,
        isExpanded: true,
        decoration: FieldDropdown.decoration(
          context,
          labelText: s.get('ip_policy'),
        ),
        items: [
          for (final policy in IpPolicy.values)
            DropdownMenuItem(
              value: policy,
              child: Text(
                s.get(switch (policy) {
                  IpPolicy.automatic => 'automatic',
                  IpPolicy.preferIpv4 => 'prefer_ipv4',
                  IpPolicy.preferIpv6 => 'prefer_ipv6',
                  IpPolicy.ipv4Only => 'ipv4_only',
                  IpPolicy.ipv6Only => 'ipv6_only',
                }),
              ),
            ),
        ],
        onChanged: widget.enabled
            ? (value) {
                if (value != null) _emit(policy: value);
              }
            : null,
      ),
    );
  }
}
