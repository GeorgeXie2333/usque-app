import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import '../models/endpoint_subscriptions.dart';
import 'common.dart';
import 'endpoint_fields.dart';
import 'tv_text_input.dart';
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
    this.subscriptions = const <EndpointSubscription>[],
    this.accountTier,
    this.inheritsSubscriptionConfirmation = true,
    super.key,
  });
  final UsqueProfile value, account;
  final ProfileIdentityStatus identity;
  final EngineCapabilities? capabilities;
  final AppStrings strings;
  final ValueChanged<UsqueProfile> onChanged;
  final bool enabled;
  final List<EndpointSubscription> subscriptions;
  final EndpointSubscriptionTier? accountTier;

  /// False after the scene was re-bound to another account: the draft's
  /// subscription was never confirmed for this account, so only the
  /// account's own current binding needs no Zero Trust warning.
  final bool inheritsSubscriptionConfirmation;

  @override
  State<ScenarioEndpointEditor> createState() => ScenarioEndpointEditorState();
}

class ScenarioEndpointEditorState extends State<ScenarioEndpointEditor> {
  late final TextEditingController _v4, _v6, _port, _sni;
  late final String _savedV4, _savedV6;
  late EndpointSelection _selection;
  late EndpointPresetRegion _region;
  late bool _preset;
  late bool _subscriptionMode;
  String? _subscriptionId;
  String? _initialSubscriptionId;
  bool _ztSubscriptionConfirmed = false;
  bool _unlocked = false, _dialogOpen = false;
  final _v4Focus = FocusNode();
  bool get _managed => widget.identity.provider == IdentityProvider.zeroTrust;
  bool get _subscriptionSupported =>
      widget.capabilities?.endpointSubscriptions == true;
  bool get _usingSubscription =>
      _subscriptionSupported &&
      _subscriptionMode &&
      (_managed || _selection == EndpointSelection.custom);
  bool get _usingPreset =>
      !_usingSubscription &&
      _preset &&
      (_managed || _selection == EndpointSelection.custom);
  List<EndpointSubscription> get _compatibleSubscriptions {
    final tier = widget.accountTier;
    if (tier == null) return const <EndpointSubscription>[];
    return widget.subscriptions
        .where((s) => s.accountTier == tier)
        .toList(growable: false);
  }

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
            value.endpointPresetRegion != null ||
            value.endpointSubscriptionId != null);
    _savedV4 = inherited ? widget.account.endpointIpv4 : value.endpointIpv4;
    _savedV6 = inherited ? widget.account.endpointIpv6 : value.endpointIpv6;
    _v4 = TextEditingController(text: _savedV4);
    _v6 = TextEditingController(text: _savedV6);
    _port = TextEditingController(text: '${value.endpointPort}');
    _sni = TextEditingController(text: value.sni);
    _selection = _managed ? EndpointSelection.custom : value.endpointSelection;
    _initialSubscriptionId = widget.inheritsSubscriptionConfirmation
        ? value.endpointSubscriptionId
        : widget.account.endpointSubscriptionId;
    _subscriptionMode =
        _subscriptionSupported && value.endpointSubscriptionId != null;
    _subscriptionId =
        value.endpointSubscriptionId ??
        _compatibleSubscriptions.firstOrNull?.id;
    _preset = !_subscriptionMode && value.endpointPresetRegion != null;
    _region = value.endpointPresetRegion ?? EndpointPresetRegion.us;
  }

  @override
  void didUpdateWidget(ScenarioEndpointEditor oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.identity != widget.identity ||
        oldWidget.account.id != widget.account.id) {
      _unlocked = false;
      _ztSubscriptionConfirmed = false;
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
        _usingSubscription ||
        _selection == EndpointSelection.automatic ||
        (_managed && (!_ztSupported || !_registered));
    widget.onChanged(
      widget.value.copyWith(
        endpointSelection: _selection,
        endpointPresetRegion: _usingPreset ? _region : null,
        clearEndpointPresetRegion: !_usingPreset,
        endpointSubscriptionId: _usingSubscription ? _subscriptionId : null,
        clearEndpointSubscriptionId:
            !_usingSubscription || _subscriptionId == null,
        endpointIpv4: preserve ? _savedV4 : _v4.text.trim(),
        endpointIpv6: preserve ? _savedV6 : _v6.text.trim(),
        endpointPort: int.tryParse(_port.text) ?? 0,
        sni: _sni.text.trim(),
        ipPolicy: policy,
      ),
    );
  }

  Future<void> _customSourceChanged(EndpointCustomSource source) async {
    if (_dialogOpen || !widget.enabled) return;
    if (source == EndpointCustomSource.preset) {
      _dialogOpen = true;
      final identity = widget.identity;
      final policy = !_ipv6Policy
          ? await chooseEndpointPresetPolicy(context, widget.strings)
          : widget.value.ipPolicy;
      _dialogOpen = false;
      if (!mounted ||
          !widget.enabled ||
          policy == null ||
          identity != widget.identity) {
        return;
      }
      setState(() {
        _subscriptionMode = false;
        _preset = true;
      });
      _emit(policy: policy);
      return;
    }
    if (source == EndpointCustomSource.subscription) {
      setState(() {
        _preset = false;
        _subscriptionMode = true;
        if (!_compatibleSubscriptions.any((s) => s.id == _subscriptionId)) {
          _subscriptionId = _compatibleSubscriptions.firstOrNull?.id;
        }
      });
      _emit();
      return;
    }
    setState(() {
      _preset = false;
      _subscriptionMode = false;
    });
    _emit();
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

  /// Why the last [prepareSave] refused, when the user must act on it. A
  /// declined confirmation dialog needs no message and leaves this null.
  String? prepareError;

  /// Presets require an IPv6 policy, including after a later policy edit.
  Future<bool> prepareSave() async {
    prepareError = null;
    if (_dialogOpen) return false;
    if (_usingSubscription) {
      if (!_subscriptionSupported) {
        prepareError = widget.strings.get('sub_err_unavailable');
        return false;
      }
      if (widget.accountTier == null) {
        prepareError = widget.strings.get('sub_verify_account');
        return false;
      }
      if (_subscriptionId == null ||
          !_compatibleSubscriptions.any((s) => s.id == _subscriptionId)) {
        prepareError = widget.strings.get(
          _compatibleSubscriptions.isEmpty
              ? 'sub_no_matching_tier'
              : 'sub_choose',
        );
        return false;
      }
      if (_managed &&
          _subscriptionId != _initialSubscriptionId &&
          !_ztSubscriptionConfirmed) {
        _dialogOpen = true;
        final identity = widget.identity;
        final accepted = await showDialog<bool>(
          context: context,
          barrierDismissible: false,
          builder: (_) => ZeroTrustEndpointWarning(strings: widget.strings),
        );
        _dialogOpen = false;
        if (!mounted || accepted != true || identity != widget.identity) {
          return false;
        }
        _ztSubscriptionConfirmed = true;
      }
      return true;
    }
    if (_usingPreset) {
      if (widget.capabilities?.endpointPresets != true || !_verified) {
        prepareError = widget.strings.get(
          widget.capabilities?.endpointPresets != true
              ? 'endpoint_preset_unsupported'
              : 'endpoint_preset_verify',
        );
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
        (_managed || _selection == EndpointSelection.custom) &&
        !_usingPreset &&
        !_usingSubscription;
    Widget address(
      TextEditingController controller,
      InternetAddressType type,
      String key,
    ) => tvTextFormField(
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
      subscriptionSupported: _subscriptionSupported,
      subscriptionMode: _subscriptionMode,
      subscriptionId: _subscriptionId,
      availableSubscriptions: _compatibleSubscriptions,
      accountTierVerified: widget.accountTier != null,
      onSelection: (value) {
        setState(() => _selection = value);
        _emit();
      },
      onPresetMode: _presetMode,
      onCustomSourceChanged: _customSourceChanged,
      onSubscriptionIdChanged: (value) {
        setState(() {
          _subscriptionId = value;
          _ztSubscriptionConfirmed = false;
        });
        _emit();
      },
      subscriptionMissingMessage:
          _usingSubscription &&
              _subscriptionId != null &&
              !_compatibleSubscriptions.any((s) => s.id == _subscriptionId)
          ? s.get('scenario_reference_missing')
          : null,
      onRegion: (value) {
        setState(() => _region = value);
        _emit();
      },
      onUnlock: _unlock,
      fields: ResponsiveEndpointFields(
        children: [
          if (_usingPreset)
            tvTextFormField(
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
          tvTextFormField(
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
            tvTextFormField(
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
            tvTextFormField(
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
