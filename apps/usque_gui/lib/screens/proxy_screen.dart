import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/frontend_presentation.dart';
import '../core/usque_motion.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/save_changes_bar.dart';

class ProxyScreen extends StatefulWidget {
  const ProxyScreen({required this.controller, super.key});
  final AppController controller;
  @override
  State<ProxyScreen> createState() => _ProxyScreenState();
}

class _ProxyScreenState extends State<ProxyScreen> {
  final _formKey = GlobalKey<FormState>();
  final _fields = List.generate(8, (_) => TextEditingController());
  final _focus = List.generate(8, (_) => FocusNode());
  late ProxyDnsMode _dnsMode;
  late List<Object> _baseline;
  bool _saving = false;
  bool _saved = false;
  bool _loading = false;
  bool _validationAttempted = false;
  String? _saveError;

  List<Object> get _values => [
    for (final field in _fields) field.text.trim(),
    _dnsMode,
  ];
  bool get _dirty => !listEquals(_values, _baseline);

  @override
  void initState() {
    super.initState();
    _load(widget.controller.activeProfile.proxy);
  }

  @override
  void didUpdateWidget(covariant ProxyScreen oldWidget) {
    super.didUpdateWidget(oldWidget);
    // Account switches and external changes may refresh clean fields. Never
    // overwrite a shared-network draft or an in-flight apply with a snapshot.
    if (!_dirty && !_saving) {
      final proxy = widget.controller.activeProfile.proxy;
      if (!listEquals(_proxyValues(proxy), _baseline)) _load(proxy);
    }
  }

  List<Object> _proxyValues(ProxySettings proxy) => [
    proxy.socksIpv4,
    proxy.socksIpv6,
    '${proxy.socksPort}',
    proxy.httpIpv4,
    proxy.httpIpv6,
    '${proxy.httpPort}',
    proxy.dnsIpv4,
    proxy.dnsIpv6,
    proxy.dnsMode,
  ];

  void _load(ProxySettings proxy) {
    _loading = true;
    final values = _proxyValues(proxy);
    for (var i = 0; i < _fields.length; i++) {
      if (_fields[i].text != values[i]) _fields[i].text = values[i] as String;
    }
    _dnsMode = proxy.dnsMode;
    _baseline = _values;
    _loading = false;
  }

  @override
  void dispose() {
    for (final field in _fields) {
      field.dispose();
    }
    for (final focus in _focus) {
      focus.dispose();
    }
    super.dispose();
  }

  void _edited() {
    if (_loading || _saving) return;
    setState(() {
      _saved = false;
      _saveError = null;
    });
  }

  String? _validate(int index, String? value) {
    final strings = widget.controller.strings;
    final text = value?.trim() ?? '';
    if (index == 2 || index == 5) {
      final port = int.tryParse(text);
      return port == null || port < 1 || port > 65535
          ? strings.get('invalid_port')
          : null;
    }
    final ipv4 = index == 0 || index == 3 || index == 6;
    final expected = ipv4 ? InternetAddressType.IPv4 : InternetAddressType.IPv6;
    return InternetAddress.tryParse(text)?.type != expected
        ? strings.get(ipv4 ? 'invalid_ipv4' : 'invalid_ipv6')
        : null;
  }

  Future<void> _save() async {
    if (_saving || widget.controller.busy) return;
    setState(() => _validationAttempted = true);
    if (!(_formKey.currentState?.validate() ?? false)) {
      setState(() => _saveError = widget.controller.strings.get('form_errors'));
      for (
        var i = 0;
        i < (_dnsMode == ProxyDnsMode.localConfigured ? 8 : 6);
        i++
      ) {
        if (_validate(i, _fields[i].text) != null) {
          _focus[i].requestFocus();
          final fieldContext = _focus[i].context;
          if (fieldContext != null) {
            unawaited(Scrollable.ensureVisible(fieldContext));
          }
          break;
        }
      }
      return;
    }
    FocusScope.of(context).unfocus();
    setState(() {
      _saving = true;
      _saveError = null;
      _saved = false;
    });
    // Merge only this form's fields into the latest shared settings so a
    // separate credential update cannot be overwritten by an older draft.
    final profile = widget.controller.activeProfile;
    final applied = await widget.controller.saveNetwork(
      profile.copyWith(
        proxy: profile.proxy.copyWith(
          socksIpv4: _fields[0].text.trim(),
          socksIpv6: _fields[1].text.trim(),
          socksPort: int.parse(_fields[2].text.trim()),
          httpIpv4: _fields[3].text.trim(),
          httpIpv6: _fields[4].text.trim(),
          httpPort: int.parse(_fields[5].text.trim()),
          dnsMode: _dnsMode,
          dnsIpv4: _dnsMode == ProxyDnsMode.localConfigured
              ? _fields[6].text.trim()
              : profile.proxy.dnsIpv4,
          dnsIpv6: _dnsMode == ProxyDnsMode.localConfigured
              ? _fields[7].text.trim()
              : profile.proxy.dnsIpv6,
        ),
      ),
    );
    if (!mounted) return;
    setState(() {
      _saving = false;
      _saved = applied;
      // saveNetwork reloads the authoritative catalog on failure. Reflect
      // that result rather than leaving an apparently applied text value.
      _load(widget.controller.activeProfile.proxy);
      _saveError = applied
          ? null
          : widget.controller.strings.get('changes_failed');
    });
  }

  @override
  Widget build(BuildContext context) {
    final strings = widget.controller.strings;
    final profile = widget.controller.activeProfile;
    // Preview risk warnings for the draft as well as the active configuration.
    final draft = profile.proxy.copyWith(
      socksIpv4: _fields[0].text.trim(),
      socksIpv6: _fields[1].text.trim(),
      httpIpv4: _fields[3].text.trim(),
      httpIpv6: _fields[4].text.trim(),
      dnsMode: _dnsMode,
    );
    return Column(
      children: [
        Expanded(
          child: PageFrame(
            title: strings.get('proxy'),
            subtitle: strings.get('proxy_subtitle'),
            child: Form(
              key: _formKey,
              autovalidateMode: _validationAttempted
                  ? AutovalidateMode.onUserInteraction
                  : AutovalidateMode.disabled,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  BannerSlot(
                    child: draft.exposesLan || profile.proxy.exposesLan
                        ? WarningBanner(
                            title: strings.get('listener_exposure'),
                            message: strings.get(
                              profile.proxy.hasAuth
                                  ? 'lan_warning_body_authenticated'
                                  : 'lan_warning_body',
                            ),
                          )
                        : null,
                  ),
                  BannerSlot(
                    child:
                        _dnsMode != ProxyDnsMode.remote ||
                            profile.proxy.dnsMode != ProxyDnsMode.remote
                        ? WarningBanner(
                            title: strings.get('dns_leak_warning'),
                            message: strings.get('dns_leak_warning_body'),
                          )
                        : null,
                  ),
                  PanelStack(
                    children: [
                      _listenerPanel(profile, socks5: true),
                      _listenerPanel(profile, socks5: false),
                      SectionPanel(
                        icon: LucideIcons.server,
                        title: strings.get('proxy_dns_mode'),
                        subtitle: strings.get('proxy_dns_subtitle'),
                        children: [
                          DropdownButtonFormField<ProxyDnsMode>(
                            key: const ValueKey<String>('proxy-dns-mode'),
                            initialValue: _dnsMode,
                            isExpanded: true,
                            decoration: InputDecoration(
                              labelText: strings.get('proxy_dns_mode'),
                            ),
                            items: ProxyDnsMode.values
                                .map(
                                  (mode) => DropdownMenuItem(
                                    value: mode,
                                    child: Text(
                                      strings.get(switch (mode) {
                                        ProxyDnsMode.remote =>
                                          'proxy_dns_remote',
                                        ProxyDnsMode.localConfigured =>
                                          'proxy_dns_configured',
                                        ProxyDnsMode.system =>
                                          'proxy_dns_system',
                                      }),
                                      overflow: TextOverflow.ellipsis,
                                    ),
                                  ),
                                )
                                .toList(),
                            onChanged: _saving
                                ? null
                                : (mode) {
                                    if (mode != null) {
                                      setState(() {
                                        _dnsMode = mode;
                                        _saved = false;
                                        _saveError = null;
                                      });
                                    }
                                  },
                          ),
                          Builder(
                            builder: (context) {
                              final fields =
                                  _dnsMode == ProxyDnsMode.localConfigured
                                  ? Padding(
                                      padding: const EdgeInsets.only(top: 16),
                                      child: _responsiveFields([
                                        _field(
                                          6,
                                          'dns_ipv4',
                                          key: const ValueKey('proxy-dns-ipv4'),
                                        ),
                                        _field(
                                          7,
                                          'dns_ipv6',
                                          key: const ValueKey('proxy-dns-ipv6'),
                                        ),
                                      ]),
                                    )
                                  : const SizedBox(width: double.infinity);
                              // Skip the size animation entirely under reduced
                              // motion, including offstage viewport changes.
                              if (UsqueMotion.reduced(context)) return fields;
                              return AnimatedSize(
                                duration: UsqueMotion.gentle,
                                alignment: Alignment.topCenter,
                                curve: UsqueMotion.emphasized,
                                child: fields,
                              );
                            },
                          ),
                        ],
                      ),
                      _AuthPanel(
                        controller: widget.controller,
                        enabled: !_saving,
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
        ),
        SaveChangesBar(
          key: const ValueKey('proxy-save-bar'),
          strings: strings,
          dirty: _dirty,
          saving: _saving,
          saved: _saved,
          error: _saveError,
          onSave: widget.controller.busy ? null : _save,
        ),
      ],
    );
  }

  Widget _field(int index, String label, {Key? key}) {
    final port = index == 2 || index == 5;
    return TextFormField(
      key: key,
      controller: _fields[index],
      focusNode: _focus[index],
      onChanged: (_) => _edited(),
      enabled: !_saving,
      keyboardType: port ? TextInputType.number : TextInputType.url,
      autocorrect: false,
      enableSuggestions: false,
      inputFormatters: port ? [FilteringTextInputFormatter.digitsOnly] : null,
      textInputAction: TextInputAction.next,
      decoration: InputDecoration(
        labelText: widget.controller.strings.get(label),
        errorMaxLines: 3,
      ),
      validator: (value) => _validate(index, value),
    );
  }

  Widget _responsiveFields(List<Widget> fields) => LayoutBuilder(
    builder: (context, constraints) {
      if (constraints.maxWidth < 640 ||
          MediaQuery.textScalerOf(context).scale(14) > 21) {
        return Column(
          children: [
            for (var i = 0; i < fields.length; i++) ...[
              if (i > 0) const SizedBox(height: 12),
              fields[i],
            ],
          ],
        );
      }
      return Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (var i = 0; i < fields.length; i++) ...[
            if (i > 0) const SizedBox(width: 12),
            Expanded(child: fields[i]),
          ],
        ],
      );
    },
  );

  Widget _listenerPanel(UsqueProfile profile, {required bool socks5}) {
    final strings = widget.controller.strings;
    final enabled = socks5 ? profile.frontends.socks5 : profile.frontends.http;
    final start = socks5 ? 0 : 3;
    final snapshot = widget.controller.snapshot;
    final kind = socks5 ? FrontendKind.socks5 : FrontendKind.http;
    final runtime = snapshot.frontends
        .where((item) => item.kind == kind)
        .firstOrNull
        ?.phase;
    final state = FrontendPresentation.of(
      configured: enabled,
      connection: snapshot.phase,
      runtime: runtime,
    );
    final pill = StatusPill(
      label: strings.get(state.labelKey),
      tone: state.tone,
      icon: state.icon,
    );
    final compact =
        MediaQuery.sizeOf(context).width < 760 ||
        MediaQuery.textScalerOf(context).scale(14) > 21;
    return SectionPanel(
      icon: socks5 ? LucideIcons.route : LucideIcons.globe2,
      title: strings.get(socks5 ? 'socks_listener' : 'http_listener'),
      subtitle: strings.get(
        enabled
            ? (socks5 ? 'socks_capabilities' : 'http_capabilities')
            : 'output_disabled_in_profile',
      ),
      trailing: compact ? null : pill,
      gap: 22,
      children: [
        if (compact) ...[
          Align(alignment: AlignmentDirectional.centerStart, child: pill),
          const SizedBox(height: 16),
        ],
        _responsiveFields([
          _field(start, 'listen_ipv4'),
          _field(start + 1, 'listen_ipv6'),
          _field(start + 2, 'port'),
        ]),
      ],
    );
  }
}

class _AuthPanel extends StatefulWidget {
  const _AuthPanel({required this.controller, required this.enabled});

  final bool enabled;

  final AppController controller;

  @override
  State<_AuthPanel> createState() => _AuthPanelState();
}

class _AuthPanelState extends State<_AuthPanel> {
  final _usernameFocus = FocusNode();
  late final TextEditingController _username;
  late final TextEditingController _password;
  String? _authError;
  String? _resultMessage;
  bool _resultFailed = false;
  bool _saving = false;
  String? _loadedProfileId;

  @override
  void initState() {
    super.initState();
    _username = TextEditingController();
    _password = TextEditingController();
    _load(widget.controller.activeProfile);
  }

  @override
  void didUpdateWidget(covariant _AuthPanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_loadedProfileId != widget.controller.activeProfile.id) {
      _load(widget.controller.activeProfile);
    }
  }

  void _load(UsqueProfile profile) {
    _loadedProfileId = profile.id;
    _username.text = profile.proxy.authUsername;
    _password.clear();
    _authError = null;
    _resultMessage = null;
  }

  @override
  void dispose() {
    _username.dispose();
    _password
      ..clear()
      ..dispose();
    _usernameFocus.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final strings = widget.controller.strings;
    return SectionPanel(
      icon: LucideIcons.keyRound,
      title: strings.get('proxy_auth'),
      subtitle: strings.get('proxy_auth_help'),
      gap: 20,
      children: <Widget>[
        Text(strings.get('proxy_auth_separate')),
        const SizedBox(height: 12),
        LayoutBuilder(
          builder: (context, constraints) {
            final username = TextField(
              key: const ValueKey<String>('proxy-auth-username'),
              controller: _username,
              focusNode: _usernameFocus,
              onChanged: (_) => _edited(),
              enabled: widget.enabled && !_saving,
              autocorrect: false,
              enableSuggestions: false,
              decoration: InputDecoration(
                labelText: strings.get('proxy_username'),
                errorText: _authError,
              ),
            );
            final password = TextField(
              key: const ValueKey<String>('proxy-auth-password'),
              controller: _password,
              onChanged: (_) => _edited(),
              enabled: widget.enabled && !_saving,
              obscureText: true,
              autocorrect: false,
              enableSuggestions: false,
              decoration: InputDecoration(
                labelText: strings.get('proxy_password'),
                helperText: strings.get('proxy_password_hint'),
              ),
            );
            if (constraints.maxWidth < 640) {
              return Column(
                children: <Widget>[
                  username,
                  const SizedBox(height: 12),
                  password,
                ],
              );
            }
            return Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Expanded(child: username),
                const SizedBox(width: 12),
                Expanded(child: password),
              ],
            );
          },
        ),
        const SizedBox(height: 14),
        if (_resultMessage != null) ...[
          Semantics(
            liveRegion: true,
            child: Text(
              _resultMessage!,
              style: TextStyle(
                color: _resultFailed
                    ? Theme.of(context).colorScheme.error
                    : null,
              ),
            ),
          ),
          const SizedBox(height: 12),
        ],
        Align(
          alignment: AlignmentDirectional.centerEnd,
          child: FilledButton(
            key: const ValueKey<String>('proxy-auth-apply'),
            onPressed: !widget.enabled || _saving || widget.controller.busy
                ? null
                : _commit,
            child: Text(
              strings.get(_saving ? 'saving_changes' : 'proxy_auth_apply'),
            ),
          ),
        ),
      ],
    );
  }

  Future<void> _commit() async {
    if (_saving || !widget.enabled || widget.controller.busy) return;
    final profileId = widget.controller.activeProfileId;
    final username = _username.text;
    final password = _password.text;
    if (!_validAuth(username, password)) {
      setState(
        () => _authError = widget.controller.strings.get('proxy_auth_invalid'),
      );
      _usernameFocus.requestFocus();
      return;
    }
    if (_authError != null) {
      setState(() => _authError = null);
    }
    setState(() {
      _saving = true;
      _resultMessage = null;
    });
    final success = await widget.controller.updateProxyAuth(
      username: username,
      password: password,
    );
    if (!mounted) {
      return;
    }
    setState(() {
      _saving = false;
      // Do not show an earlier account's result in a newly selected account.
      if (widget.controller.activeProfileId != profileId) return;
      _password.clear();
      _resultFailed = !success;
      _resultMessage = widget.controller.strings.get(
        success
            ? (username.isEmpty ? 'proxy_auth_cleared' : 'proxy_auth_saved')
            : 'changes_failed',
      );
    });
  }

  void _edited() => setState(() {
    _authError = null;
    _resultMessage = null;
  });

  bool _validAuth(String username, String password) {
    if (username.isEmpty && password.isEmpty) {
      return true;
    }
    final usernameBytes = utf8.encode(username);
    final passwordBytes = utf8.encode(password);
    if (username.isEmpty ||
        usernameBytes.length > 255 ||
        username.contains(':') ||
        username.contains('\u0000')) {
      return false;
    }
    if (password.isEmpty || passwordBytes.length > 255) {
      return false;
    }
    return true;
  }
}
