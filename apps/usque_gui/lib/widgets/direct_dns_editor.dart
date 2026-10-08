import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import '../models/encrypted_dns_endpoint.dart';
import 'common.dart';

export '../models/encrypted_dns_endpoint.dart'
    show validDirectDnsName, validDirectDnsPath;

List<String> directDnsBootstrapValues(String value) => value
    .split(RegExp(r'[,\s]+'))
    .where((value) => value.isNotEmpty)
    .toList(growable: false);

bool validDirectDnsBootstrap(String value) =>
    directDnsBootstrapError(value) == null;

String? directDnsBootstrapError(String value) {
  final values = directDnsBootstrapValues(value);
  if (values.isEmpty) return 'required';
  if (values.length > 8) return 'nq_dns_invalid_bootstrap';
  final unique = <String>{};
  for (final value in values) {
    final address = InternetAddress.tryParse(value);
    if (address == null || value.contains('%')) return 'invalid_address';
    final raw = address.rawAddress;
    if (!unique.add(raw.join('.'))) return 'dns_duplicate_address';
    if (raw.every((byte) => byte == 0)) {
      return 'dns_address_not_allowed';
    }
    if (address.type == InternetAddressType.IPv4 &&
        ((raw[0] >= 224 && raw[0] <= 239) ||
            raw.every((byte) => byte == 255))) {
      return 'dns_address_not_allowed';
    }
    if (address.type == InternetAddressType.IPv6 &&
        (raw[0] == 255 || raw[0] == 254 && raw[1] & 192 == 128)) {
      return 'dns_address_not_allowed';
    }
  }
  return null;
}

class DirectDnsEditor extends StatefulWidget {
  const DirectDnsEditor({
    required this.value,
    required this.enabled,
    this.encryptedAvailable = true,
    this.resetRevision = 0,
    required this.strings,
    required this.onChanged,
    super.key,
  });
  final DirectDnsSettings value;
  final bool enabled;
  final bool encryptedAvailable;
  final int resetRevision;
  final AppStrings strings;
  final ValueChanged<DirectDnsSettings> onChanged;
  @override
  State<DirectDnsEditor> createState() => DirectDnsEditorState();
}

class DirectDnsEditorState extends State<DirectDnsEditor> {
  late DirectDnsMode _mode;
  final _server = TextEditingController();
  final _url = TextEditingController();
  final _port = TextEditingController();
  final _bootstrap = TextEditingController();
  final _bootstraps = <DirectDnsMode, String>{};
  final _keys = List<GlobalKey<FormFieldState<String>>>.generate(
    4,
    (_) => GlobalKey<FormFieldState<String>>(),
  );
  final _focus = List<FocusNode>.generate(4, (_) => FocusNode());

  @override
  void initState() {
    super.initState();
    _load(widget.value);
  }

  @override
  void didUpdateWidget(covariant DirectDnsEditor oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.resetRevision != oldWidget.resetRevision ||
        widget.value != oldWidget.value && widget.value != _value()) {
      _load(widget.value);
    }
  }

  void _load(DirectDnsSettings value) {
    _bootstraps.clear();
    _mode = value.mode;
    final tls = _mode == DirectDnsMode.dot || _mode == DirectDnsMode.unknown;
    _server.text = tls ? value.serverName : cloudflareDotServer;
    _url.text = _mode == DirectDnsMode.doh
        ? DohEndpoint(value.serverName, value.port, value.dohPath).url
        : cloudflareDohUrl;
    _port.text = tls && value.port != 0 ? '${value.port}' : '853';
    _bootstrap.text = value.bootstrapIps.join('\n');
    _bootstraps[_mode] = _bootstrap.text;
  }

  DirectDnsSettings _value() {
    if (_mode == DirectDnsMode.physicalSystem) return const DirectDnsSettings();
    final endpoint = _mode == DirectDnsMode.doh
        ? DohEndpoint.tryParse(_url.text)
        : null;
    return DirectDnsSettings(
      mode: _mode,
      serverName: _mode == DirectDnsMode.doh
          ? endpoint?.serverName ?? _url.text
          : _server.text,
      // Invalid URL drafts must fail core validation too, never become defaults.
      dohPath: _mode == DirectDnsMode.doh
          ? endpoint?.path ?? 'invalid-url'
          : '',
      port: _mode == DirectDnsMode.doh
          ? endpoint?.port ?? 0
          : int.tryParse(_port.text) ?? 0,
      bootstrapIps: directDnsBootstrapValues(_bootstrap.text),
    );
  }

  void _emit(String _) {
    widget.onChanged(_value());
  }

  void focusFirstError() {
    for (var index = 0; index < _keys.length; index++) {
      if (_keys[index].currentState?.hasError ?? false) {
        _focus[index].requestFocus();
        return;
      }
    }
  }

  @override
  void dispose() {
    for (final controller in <TextEditingController>[
      _server,
      _url,
      _port,
      _bootstrap,
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
    final s = widget.strings;
    final custom = _mode != DirectDnsMode.physicalSystem;
    final editable = widget.enabled && widget.encryptedAvailable;
    return ContentSection(
      icon: LucideIcons.shieldCheck,
      title: s.get('nq_direct_dns'),
      children: <Widget>[
        Text(s.get('nq_dns_scope')),
        const SizedBox(height: 12),
        DropdownButtonFormField<DirectDnsMode>(
          key: ValueKey<DirectDnsMode>(_mode),
          initialValue: _mode,
          isExpanded: true,
          decoration: InputDecoration(labelText: s.get('nq_direct_dns')),
          items:
              <DirectDnsMode>[
                    DirectDnsMode.physicalSystem,
                    DirectDnsMode.doh,
                    DirectDnsMode.dot,
                    if (_mode == DirectDnsMode.unknown) DirectDnsMode.unknown,
                  ]
                  .map(
                    (mode) => DropdownMenuItem<DirectDnsMode>(
                      value: mode,
                      enabled:
                          widget.encryptedAvailable ||
                          mode == DirectDnsMode.physicalSystem,
                      child: Text(
                        s.get(switch (mode) {
                          DirectDnsMode.physicalSystem => 'nq_system_dns',
                          DirectDnsMode.doh => 'nq_doh',
                          DirectDnsMode.dot => 'nq_dot',
                          _ => 'nq_unsupported',
                        }),
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                  )
                  .toList(growable: false),
          onChanged: !widget.enabled
              ? null
              : (value) {
                  if (value == null ||
                      !widget.encryptedAvailable &&
                          value != DirectDnsMode.physicalSystem) {
                    return;
                  }
                  setState(() {
                    _bootstraps[_mode] = _bootstrap.text;
                    _mode = value;
                    _bootstrap.text =
                        _bootstraps[_mode] ??
                        cloudflareDnsBootstrapIps.join('\n');
                  });
                  _emit('');
                },
          validator: (value) => editable && value == DirectDnsMode.unknown
              ? s.get('nq_dns_invalid_mode')
              : null,
        ),
        const SizedBox(height: 12),
        if (!widget.encryptedAvailable) Text(s.get('nq_dns_no_capability')),
        Text(s.get(custom ? 'nq_dns_no_fallback' : 'nq_dns_system_privacy')),
        if (custom) ...<Widget>[
          const SizedBox(height: 20),
          if (_mode != DirectDnsMode.doh)
            TextFormField(
              key: _keys[0],
              focusNode: _focus[0],
              controller: _server,
              readOnly: !editable,
              autocorrect: false,
              enableSuggestions: false,
              maxLength: 253,
              decoration: InputDecoration(
                labelText: s.get('nq_dns_server'),
                hintText: cloudflareDotServer,
                errorMaxLines: 6,
              ),
              onChanged: _emit,
              validator: (value) => !editable || validDirectDnsName(value ?? '')
                  ? null
                  : s.get('nq_dns_invalid_name'),
            ),
          if (_mode != DirectDnsMode.doh) const SizedBox(height: 12),
          if (_mode == DirectDnsMode.doh) ...<Widget>[
            TextFormField(
              key: _keys[1],
              focusNode: _focus[1],
              controller: _url,
              readOnly: !editable,
              autocorrect: false,
              enableSuggestions: false,
              keyboardType: TextInputType.url,
              textDirection: TextDirection.ltr,
              maxLength: 522,
              decoration: InputDecoration(
                labelText: s.get('dns_doh_url'),
                hintText: cloudflareDohUrl,
                counterText: '',
                errorMaxLines: 6,
              ),
              onChanged: _emit,
              validator: (value) =>
                  !editable || DohEndpoint.tryParse(value ?? '') != null
                  ? null
                  : s.get('dns_invalid_doh_url'),
            ),
            const SizedBox(height: 12),
          ],
          if (_mode != DirectDnsMode.doh)
            TextFormField(
              key: _keys[2],
              focusNode: _focus[2],
              controller: _port,
              readOnly: !editable,
              keyboardType: TextInputType.number,
              inputFormatters: <TextInputFormatter>[
                FilteringTextInputFormatter.digitsOnly,
              ],
              decoration: InputDecoration(labelText: s.get('nq_dns_port')),
              onChanged: _emit,
              validator: (value) {
                final port = int.tryParse(value ?? '');
                return !editable || port != null && port >= 0 && port <= 65535
                    ? null
                    : s.get('nq_dns_invalid_port');
              },
            ),
          if (_mode != DirectDnsMode.doh) const SizedBox(height: 12),
          TextFormField(
            key: _keys[3],
            focusNode: _focus[3],
            controller: _bootstrap,
            readOnly: !editable,
            autocorrect: false,
            enableSuggestions: false,
            minLines: 2,
            maxLines: 8,
            maxLength: 512,
            decoration: InputDecoration(
              labelText: s.get('nq_dns_bootstrap'),
              helperText: s.get('nq_dns_bootstrap_help'),
              helperMaxLines: 6,
              errorMaxLines: 6,
            ),
            onChanged: _emit,
            validator: (value) {
              if (!editable) return null;
              final issue = directDnsBootstrapError(value ?? '');
              return issue == null ? null : s.get(issue);
            },
          ),
        ],
      ],
    );
  }
}
