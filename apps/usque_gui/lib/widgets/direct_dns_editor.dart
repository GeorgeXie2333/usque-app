import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import 'common.dart';

bool validDirectDnsName(String value) {
  if (value.isEmpty ||
      value.runes.length > 253 ||
      value.trim() != value ||
      InternetAddress.tryParse(value) != null) {
    return false;
  }
  if (value.runes.any((rune) => rune <= 32 || rune == 127) ||
      RegExp(r'[:/\\?#@%*\[\]\s]').hasMatch(value)) {
    return false;
  }
  return value
      .split('.')
      .every(
        (label) =>
            label.isNotEmpty &&
            label.runes.length <= 63 &&
            !label.startsWith('-') &&
            !label.endsWith('-') &&
            label.runes.every(
              (rune) =>
                  rune > 127 ||
                  RegExp(r'[a-zA-Z0-9-]').hasMatch(String.fromCharCode(rune)),
            ),
      );
}

bool validDirectDnsPath(String value) =>
    value.startsWith('/') &&
    !value.startsWith('//') &&
    value.length <= 256 &&
    !value.contains('://') &&
    !RegExp(r'[?#\\\s]').hasMatch(value) &&
    value.codeUnits.every((unit) => unit >= 33 && unit <= 126);

List<String> directDnsBootstrapValues(String value) => value
    .split(RegExp(r'[,\s]+'))
    .where((value) => value.isNotEmpty)
    .toList(growable: false);

bool validDirectDnsBootstrap(String value) {
  final values = directDnsBootstrapValues(value);
  if (values.isEmpty || values.length > 8) return false;
  final unique = <String>{};
  for (final value in values) {
    final address = InternetAddress.tryParse(value);
    if (address == null || value.contains('%')) return false;
    final raw = address.rawAddress;
    if (raw.every((byte) => byte == 0) || !unique.add(raw.join('.'))) {
      return false;
    }
    if (address.type == InternetAddressType.IPv4 &&
        ((raw[0] >= 224 && raw[0] <= 239) ||
            raw.every((byte) => byte == 255))) {
      return false;
    }
    if (address.type == InternetAddressType.IPv6 &&
        (raw[0] == 255 || raw[0] == 254 && raw[1] & 192 == 128)) {
      return false;
    }
  }
  return true;
}

class DirectDnsEditor extends StatefulWidget {
  const DirectDnsEditor({
    required this.value,
    required this.enabled,
    this.encryptedAvailable = true,
    required this.strings,
    required this.onChanged,
    super.key,
  });
  final DirectDnsSettings value;
  final bool enabled;
  final bool encryptedAvailable;
  final AppStrings strings;
  final ValueChanged<DirectDnsSettings> onChanged;
  @override
  State<DirectDnsEditor> createState() => DirectDnsEditorState();
}

class DirectDnsEditorState extends State<DirectDnsEditor> {
  late DirectDnsMode _mode;
  final _server = TextEditingController();
  final _path = TextEditingController();
  final _port = TextEditingController();
  final _bootstrap = TextEditingController();
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
    if (widget.value != oldWidget.value && widget.value != _value()) {
      _load(widget.value);
    }
  }

  void _load(DirectDnsSettings value) {
    _mode = value.mode;
    _server.text = value.serverName;
    _path.text = value.dohPath;
    _port.text = '${value.port}';
    _bootstrap.text = value.bootstrapIps.join('\n');
  }

  DirectDnsSettings _value() => _mode == DirectDnsMode.physicalSystem
      ? const DirectDnsSettings()
      : DirectDnsSettings(
          mode: _mode,
          serverName: _server.text,
          dohPath: _mode == DirectDnsMode.doh ? _path.text : '',
          port: int.tryParse(_port.text) ?? 0,
          bootstrapIps: directDnsBootstrapValues(_bootstrap.text),
        );

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
      _path,
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
    return SectionPanel(
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
                    _mode = value;
                    if (_mode == DirectDnsMode.doh && _path.text.isEmpty) {
                      _path.text = '/dns-query';
                    }
                    _port.text = _mode == DirectDnsMode.doh
                        ? '443'
                        : _mode == DirectDnsMode.dot
                        ? '853'
                        : '0';
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
          TextFormField(
            key: _keys[0],
            focusNode: _focus[0],
            controller: _server,
            readOnly: !editable,
            autocorrect: false,
            enableSuggestions: false,
            maxLength: 253,
            decoration: InputDecoration(labelText: s.get('nq_dns_server')),
            onChanged: _emit,
            validator: (value) => !editable || validDirectDnsName(value ?? '')
                ? null
                : s.get('nq_dns_invalid_name'),
          ),
          const SizedBox(height: 12),
          if (_mode == DirectDnsMode.doh) ...<Widget>[
            TextFormField(
              key: _keys[1],
              focusNode: _focus[1],
              controller: _path,
              readOnly: !editable,
              autocorrect: false,
              enableSuggestions: false,
              maxLength: 256,
              decoration: InputDecoration(labelText: s.get('nq_dns_path')),
              onChanged: _emit,
              validator: (value) => !editable || validDirectDnsPath(value ?? '')
                  ? null
                  : s.get('nq_dns_invalid_path'),
            ),
            const SizedBox(height: 12),
          ],
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
          const SizedBox(height: 12),
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
            ),
            onChanged: _emit,
            validator: (value) =>
                !editable || validDirectDnsBootstrap(value ?? '')
                ? null
                : s.get('nq_dns_invalid_bootstrap'),
          ),
        ],
      ],
    );
  }
}
