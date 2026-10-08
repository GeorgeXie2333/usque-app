import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import '../models/encrypted_dns_endpoint.dart';
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
  final _url = TextEditingController();
  final _port = TextEditingController();
  final _bootstrap = TextEditingController();
  final _bootstraps = <WarpDnsMode, String>{};
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
    _bootstraps.clear();
    _mode = value.mode;
    final tls = _mode == WarpDnsMode.dot || _mode == WarpDnsMode.unknown;
    _server.text = tls ? value.serverName : cloudflareDotServer;
    _url.text = _mode == WarpDnsMode.doh
        ? DohEndpoint(value.serverName, value.port, value.dohPath).url
        : cloudflareDohUrl;
    _port.text = tls && value.port != 0 ? '${value.port}' : '853';
    _bootstrap.text = value.bootstrapIps.join('\n');
    _bootstraps[_mode] = _bootstrap.text;
  }

  WarpDnsSettings _value() {
    if (_mode == WarpDnsMode.plain) return const WarpDnsSettings();
    final endpoint = _mode == WarpDnsMode.doh
        ? DohEndpoint.tryParse(_url.text)
        : null;
    return WarpDnsSettings(
      mode: _mode,
      serverName: _mode == WarpDnsMode.doh
          ? endpoint?.serverName ?? _url.text
          : _server.text,
      // Invalid URL drafts must fail core validation too, never become defaults.
      dohPath: _mode == WarpDnsMode.doh ? endpoint?.path ?? 'invalid-url' : '',
      port: _mode == WarpDnsMode.doh
          ? endpoint?.port ?? 0
          : int.tryParse(_port.text) ?? 0,
      bootstrapIps: directDnsBootstrapValues(_bootstrap.text),
    );
  }

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
      _url,
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
                    _bootstraps[_mode] = _bootstrap.text;
                    _mode = value;
                    _bootstrap.text =
                        _bootstraps[_mode] ??
                        cloudflareDnsBootstrapIps.join('\n');
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
          if (_mode != WarpDnsMode.doh)
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
                counterText: '',
                errorMaxLines: 3,
              ),
              onChanged: _emit,
              validator: (value) => !editable || validDirectDnsName(value ?? '')
                  ? null
                  : s.get('warp_dns_invalid_name'),
            ),
          if (_mode != WarpDnsMode.doh) const SizedBox(height: 12),
          if (_mode == WarpDnsMode.doh) ...<Widget>[
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
                errorMaxLines: 3,
              ),
              onChanged: _emit,
              validator: (value) =>
                  !editable || DohEndpoint.tryParse(value ?? '') != null
                  ? null
                  : s.get('dns_invalid_doh_url'),
            ),
            const SizedBox(height: 12),
          ],
          if (_mode != WarpDnsMode.doh)
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
          if (_mode != WarpDnsMode.doh) const SizedBox(height: 12),
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
              hintText: s.get('warp_dns_bootstrap_optional'),
              counterText: '',
              errorMaxLines: 3,
            ),
            onChanged: _emit,
            validator: (value) {
              if (!editable || (value ?? '').trim().isEmpty) return null;
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
