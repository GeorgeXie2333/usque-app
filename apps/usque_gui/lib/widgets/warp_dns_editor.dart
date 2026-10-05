import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import 'direct_dns_editor.dart';

class WarpDnsEditor extends StatefulWidget {
  const WarpDnsEditor({
    required this.value,
    required this.enabled,
    this.encryptedAvailable = true,
    this.resetRevision = 0,
    required this.strings,
    required this.onChanged,
    super.key,
  });
  final WarpDnsSettings value;
  final bool enabled;
  final bool encryptedAvailable;

  /// An explicit reset clears inactive drafts even when the selected value
  /// already equals the defaults.
  final int resetRevision;
  final AppStrings strings;
  final ValueChanged<WarpDnsSettings> onChanged;
  @override
  State<WarpDnsEditor> createState() => WarpDnsEditorState();
}

class WarpDnsEditorState extends State<WarpDnsEditor> {
  late WarpDnsMode _mode;
  final _server = TextEditingController();
  final _path = TextEditingController();
  final _port = TextEditingController();
  final _bootstrap = TextEditingController();
  final _ports = <WarpDnsMode, String>{};
  final _modeFocus = FocusNode();
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
  void didUpdateWidget(covariant WarpDnsEditor oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.resetRevision != oldWidget.resetRevision ||
        widget.value != oldWidget.value && widget.value != _value()) {
      _load(widget.value);
    }
  }

  void _load(WarpDnsSettings value) {
    _ports.clear();
    _mode = value.mode;
    _server.text = value.serverName;
    _path.text = value.dohPath;
    _port.text = '${value.port}';
    _ports[value.mode] = _port.text;
    _bootstrap.text = value.bootstrapIps.join('\n');
  }

  WarpDnsSettings _value() => _mode == WarpDnsMode.plain
      ? const WarpDnsSettings()
      : WarpDnsSettings(
          mode: _mode,
          serverName: _server.text,
          dohPath: _mode == WarpDnsMode.doh ? _path.text : '',
          port: int.tryParse(_port.text) ?? 0,
          bootstrapIps: directDnsBootstrapValues(_bootstrap.text),
        );

  void _emit(String _) {
    widget.onChanged(_value());
  }

  bool focusFirstError() {
    if (_mode == WarpDnsMode.unknown) {
      _modeFocus.requestFocus();
      return true;
    }
    for (var index = 0; index < _keys.length; index++) {
      if (_keys[index].currentState?.hasError ?? false) {
        _focus[index].requestFocus();
        return true;
      }
    }
    return false;
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
    _modeFocus.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final s = widget.strings;
    final custom = _mode != WarpDnsMode.plain;
    final editable = widget.enabled && widget.encryptedAvailable;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        DropdownButtonFormField<WarpDnsMode>(
          key: ValueKey<WarpDnsMode>(_mode),
          initialValue: _mode,
          focusNode: _modeFocus,
          isExpanded: true,
          decoration: InputDecoration(labelText: s.get('warp_dns_type')),
          items:
              <WarpDnsMode>[
                    WarpDnsMode.plain,
                    WarpDnsMode.doh,
                    WarpDnsMode.dot,
                    if (_mode == WarpDnsMode.unknown) WarpDnsMode.unknown,
                  ]
                  .map(
                    (mode) => DropdownMenuItem<WarpDnsMode>(
                      value: mode,
                      enabled:
                          widget.encryptedAvailable ||
                          mode == WarpDnsMode.plain,
                      child: Text(
                        s.get(switch (mode) {
                          WarpDnsMode.plain => 'warp_dns_plain',
                          WarpDnsMode.doh => 'nq_doh',
                          WarpDnsMode.dot => 'nq_dot',
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
                          value != WarpDnsMode.plain) {
                    return;
                  }
                  setState(() {
                    _ports[_mode] = _port.text;
                    _mode = value;
                    if (_mode == WarpDnsMode.doh && _path.text.isEmpty) {
                      _path.text = '/dns-query';
                    }
                    _port.text =
                        _ports[_mode] ??
                        (_mode == WarpDnsMode.doh
                            ? '443'
                            : _mode == WarpDnsMode.dot
                            ? '853'
                            : '0');
                  });
                  _emit('');
                },
          validator: (value) => value == WarpDnsMode.unknown
              ? s.get('warp_dns_invalid_mode')
              : null,
        ),
        if (custom && !widget.encryptedAvailable) ...<Widget>[
          const SizedBox(height: 8),
          Text(s.get('warp_dns_unsupported')),
        ],
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
            decoration: InputDecoration(
              labelText: s.get('nq_dns_server'),
              hintText: 'dns.example.com',
              counterText: '',
              errorMaxLines: 3,
            ),
            onChanged: _emit,
            validator: (value) => !editable || validDirectDnsName(value ?? '')
                ? null
                : s.get('warp_dns_invalid_name'),
          ),
          const SizedBox(height: 12),
          if (_mode == WarpDnsMode.doh) ...<Widget>[
            TextFormField(
              key: _keys[1],
              focusNode: _focus[1],
              controller: _path,
              readOnly: !editable,
              autocorrect: false,
              enableSuggestions: false,
              maxLength: 256,
              decoration: InputDecoration(
                labelText: s.get('nq_dns_path'),
                hintText: '/dns-query',
                counterText: '',
                errorMaxLines: 3,
              ),
              onChanged: _emit,
              validator: (value) => !editable || validDirectDnsPath(value ?? '')
                  ? null
                  : s.get('warp_dns_invalid_path'),
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
            decoration: InputDecoration(labelText: s.get('port')),
            onChanged: _emit,
            validator: (value) {
              final port = int.tryParse(value ?? '');
              return !editable || port != null && port >= 1 && port <= 65535
                  ? null
                  : '1–65535';
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
              counterText: '',
              errorMaxLines: 3,
            ),
            onChanged: _emit,
            validator: (value) {
              if (!editable) return null;
              final issue = directDnsBootstrapError(value ?? '');
              return issue == null
                  ? null
                  : s.get(
                      issue == 'nq_dns_invalid_bootstrap'
                          ? 'warp_dns_invalid_bootstrap'
                          : issue,
                    );
            },
          ),
        ],
      ],
    );
  }
}
