import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/usque_motion.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';

class ProxyScreen extends StatefulWidget {
  const ProxyScreen({required this.controller, super.key});

  final AppController controller;

  @override
  State<ProxyScreen> createState() => _ProxyScreenState();
}

class _ProxyScreenState extends State<ProxyScreen> {
  late final TextEditingController _socksV4;
  late final TextEditingController _socksV6;
  late final TextEditingController _socksPort;
  late final TextEditingController _httpV4;
  late final TextEditingController _httpV6;
  late final TextEditingController _httpPort;
  late final TextEditingController _dnsV4;
  late final TextEditingController _dnsV6;
  String? _dnsV4Error;
  String? _dnsV6Error;
  String? _loadedProfileId;

  @override
  void initState() {
    super.initState();
    _socksV4 = TextEditingController();
    _socksV6 = TextEditingController();
    _socksPort = TextEditingController();
    _httpV4 = TextEditingController();
    _httpV6 = TextEditingController();
    _httpPort = TextEditingController();
    _dnsV4 = TextEditingController();
    _dnsV6 = TextEditingController();
    _load(widget.controller.activeProfile);
  }

  @override
  void didUpdateWidget(covariant ProxyScreen oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_loadedProfileId != widget.controller.activeProfile.id) {
      _load(widget.controller.activeProfile);
    }
  }

  void _load(UsqueProfile profile) {
    _loadedProfileId = profile.id;
    _socksV4.text = profile.proxy.socksIpv4;
    _socksV6.text = profile.proxy.socksIpv6;
    _socksPort.text = profile.proxy.socksPort.toString();
    _httpV4.text = profile.proxy.httpIpv4;
    _httpV6.text = profile.proxy.httpIpv6;
    _httpPort.text = profile.proxy.httpPort.toString();
    _dnsV4.text = profile.proxy.dnsIpv4;
    _dnsV6.text = profile.proxy.dnsIpv6;
    _dnsV4Error = null;
    _dnsV6Error = null;
  }

  @override
  void dispose() {
    for (final controller in <TextEditingController>[
      _socksV4,
      _socksV6,
      _socksPort,
      _httpV4,
      _httpV6,
      _httpPort,
      _dnsV4,
      _dnsV6,
    ]) {
      controller.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final strings = widget.controller.strings;
    final profile = widget.controller.activeProfile;
    final currentProxy = profile.proxy;
    return PageFrame(
      title: strings.get('proxy'),
      subtitle: strings.get('proxy_subtitle'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          BannerSlot(
            child: currentProxy.exposesLan
                ? WarningBanner(
                    title: strings.get('lan_warning'),
                    message: strings.get(
                      currentProxy.hasAuth
                          ? 'lan_warning_body_authenticated'
                          : 'lan_warning_body',
                    ),
                  )
                : null,
          ),
          BannerSlot(
            child: currentProxy.dnsMode != ProxyDnsMode.remote
                ? WarningBanner(
                    title: strings.get('dns_leak_warning'),
                    message: strings.get('dns_leak_warning_body'),
                  )
                : null,
          ),
          PanelStack(
            children: <Widget>[
              _listenerPanel(context, profile, socks5: true),
              _listenerPanel(context, profile, socks5: false),
              _AuthPanel(controller: widget.controller),
              SectionPanel(
                icon: LucideIcons.server,
                title: strings.get('proxy_dns_mode'),
                subtitle: strings.get('proxy_dns_subtitle'),
                gap: 20,
                children: <Widget>[
                  DropdownButtonFormField<ProxyDnsMode>(
                    key: const ValueKey<String>('proxy-dns-mode'),
                    initialValue: currentProxy.dnsMode,
                    decoration: InputDecoration(
                      labelText: strings.get('proxy_dns_mode'),
                    ),
                    items: ProxyDnsMode.values
                        .map(
                          (mode) => DropdownMenuItem<ProxyDnsMode>(
                            value: mode,
                            child: Text(
                              strings.get(switch (mode) {
                                ProxyDnsMode.remote => 'proxy_dns_remote',
                                ProxyDnsMode.localConfigured =>
                                  'proxy_dns_configured',
                                ProxyDnsMode.system => 'proxy_dns_system',
                              }),
                            ),
                          ),
                        )
                        .toList(growable: false),
                    onChanged: (value) {
                      if (value != null) {
                        widget.controller.updateProfile(
                          profile.copyWith(
                            proxy: currentProxy.copyWith(dnsMode: value),
                          ),
                        );
                      }
                    },
                  ),
                  AnimatedSize(
                    duration: UsqueMotion.of(context, UsqueMotion.gentle),
                    curve: UsqueMotion.emphasized,
                    alignment: Alignment.topCenter,
                    child: currentProxy.dnsMode == ProxyDnsMode.localConfigured
                        ? Padding(
                            padding: const EdgeInsets.only(top: 14),
                            child: _dnsFields(profile),
                          )
                        : const SizedBox(width: double.infinity),
                  ),
                ],
              ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _dnsFields(UsqueProfile profile) {
    final strings = widget.controller.strings;
    return LayoutBuilder(
      builder: (context, constraints) {
        final fields = <Widget>[
          TextField(
            key: const ValueKey<String>('proxy-dns-ipv4'),
            controller: _dnsV4,
            keyboardType: TextInputType.url,
            autocorrect: false,
            enableSuggestions: false,
            textInputAction: TextInputAction.next,
            decoration: InputDecoration(
              labelText: strings.get('dns_ipv4'),
              errorText: _dnsV4Error,
            ),
            onChanged: (_) => _saveDns(profile),
          ),
          TextField(
            key: const ValueKey<String>('proxy-dns-ipv6'),
            controller: _dnsV6,
            keyboardType: TextInputType.url,
            autocorrect: false,
            enableSuggestions: false,
            textInputAction: TextInputAction.done,
            decoration: InputDecoration(
              labelText: strings.get('dns_ipv6'),
              errorText: _dnsV6Error,
            ),
            onChanged: (_) => _saveDns(profile),
          ),
        ];
        if (constraints.maxWidth < 640) {
          return Column(
            children: <Widget>[
              fields[0],
              const SizedBox(height: 12),
              fields[1],
            ],
          );
        }
        return Row(
          children: <Widget>[
            Expanded(child: fields[0]),
            const SizedBox(width: 12),
            Expanded(child: fields[1]),
          ],
        );
      },
    );
  }

  void _saveDns(UsqueProfile profile) {
    final ipv4 = _dnsV4.text.trim();
    final ipv6 = _dnsV6.text.trim();
    final ipv4Valid =
        InternetAddress.tryParse(ipv4)?.type == InternetAddressType.IPv4;
    final ipv6Valid =
        InternetAddress.tryParse(ipv6)?.type == InternetAddressType.IPv6;
    final invalidAddress = widget.controller.strings.get('invalid_address');
    final ipv4Error = ipv4Valid ? null : invalidAddress;
    final ipv6Error = ipv6Valid ? null : invalidAddress;
    if (ipv4Error != _dnsV4Error || ipv6Error != _dnsV6Error) {
      setState(() {
        _dnsV4Error = ipv4Error;
        _dnsV6Error = ipv6Error;
      });
    }
    if (!ipv4Valid || !ipv6Valid) {
      return;
    }
    if (profile.proxy.dnsIpv4 == ipv4 && profile.proxy.dnsIpv6 == ipv6) {
      return;
    }
    widget.controller.updateProfile(
      profile.copyWith(
        proxy: profile.proxy.copyWith(dnsIpv4: ipv4, dnsIpv6: ipv6),
      ),
    );
  }

  Widget _listenerPanel(
    BuildContext context,
    UsqueProfile profile, {
    required bool socks5,
  }) {
    final strings = widget.controller.strings;
    final enabled = socks5 ? profile.frontends.socks5 : profile.frontends.http;
    final v4Controller = socks5 ? _socksV4 : _httpV4;
    final v6Controller = socks5 ? _socksV6 : _httpV6;
    final portController = socks5 ? _socksPort : _httpPort;
    return SectionPanel(
      icon: socks5 ? LucideIcons.route : LucideIcons.globe2,
      title: strings.get(socks5 ? 'socks_listener' : 'http_listener'),
      subtitle: strings.get(
        enabled
            ? (socks5 ? 'socks_capabilities' : 'http_capabilities')
            : 'output_disabled_in_profile',
      ),
      trailing: StatusPill(
        label: strings.get(enabled ? 'on' : 'off'),
        tone: enabled ? StatusTone.success : StatusTone.neutral,
      ),
      gap: 22,
      children: <Widget>[
        LayoutBuilder(
          builder: (context, constraints) {
            final narrow = constraints.maxWidth < 640;
            final address = <Widget>[
              _AddressField(
                label: strings.get('listen_ipv4'),
                controller: v4Controller,
                onChanged: (_) => _saveListeners(profile),
              ),
              _AddressField(
                label: strings.get('listen_ipv6'),
                controller: v6Controller,
                onChanged: (_) => _saveListeners(profile),
              ),
            ];
            final port = TextField(
              controller: portController,
              keyboardType: TextInputType.number,
              inputFormatters: <TextInputFormatter>[
                FilteringTextInputFormatter.digitsOnly,
              ],
              decoration: InputDecoration(labelText: strings.get('port')),
              onChanged: (_) => _saveListeners(profile),
            );
            if (narrow) {
              return Column(
                children: <Widget>[
                  address[0],
                  const SizedBox(height: 12),
                  address[1],
                  const SizedBox(height: 12),
                  port,
                ],
              );
            }
            return Row(
              children: <Widget>[
                Expanded(child: address[0]),
                const SizedBox(width: 12),
                Expanded(child: address[1]),
                const SizedBox(width: 12),
                SizedBox(width: 150, child: port),
              ],
            );
          },
        ),
      ],
    );
  }

  void _saveListeners(UsqueProfile profile) {
    final previous = profile.proxy;
    final socksPort = int.tryParse(_socksPort.text);
    final httpPort = int.tryParse(_httpPort.text);
    final next = previous.copyWith(
      socksIpv4: _socksV4.text.trim(),
      socksIpv6: _socksV6.text.trim(),
      socksPort: socksPort != null && socksPort > 0 && socksPort <= 65535
          ? socksPort
          : previous.socksPort,
      httpIpv4: _httpV4.text.trim(),
      httpIpv6: _httpV6.text.trim(),
      httpPort: httpPort != null && httpPort > 0 && httpPort <= 65535
          ? httpPort
          : previous.httpPort,
    );
    widget.controller.updateProfile(profile.copyWith(proxy: next));
  }
}

class _AddressField extends StatelessWidget {
  const _AddressField({
    required this.label,
    required this.controller,
    required this.onChanged,
  });

  final String label;
  final TextEditingController controller;
  final ValueChanged<String> onChanged;

  @override
  Widget build(BuildContext context) {
    return TextField(
      controller: controller,
      keyboardType: TextInputType.url,
      decoration: InputDecoration(labelText: label),
      onChanged: onChanged,
    );
  }
}

class _AuthPanel extends StatefulWidget {
  const _AuthPanel({required this.controller});

  final AppController controller;

  @override
  State<_AuthPanel> createState() => _AuthPanelState();
}

class _AuthPanelState extends State<_AuthPanel> {
  late final TextEditingController _username;
  late final TextEditingController _password;
  String? _authError;
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
  }

  @override
  void dispose() {
    _username.dispose();
    _password.dispose();
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
        LayoutBuilder(
          builder: (context, constraints) {
            final username = TextField(
              key: const ValueKey<String>('proxy-auth-username'),
              controller: _username,
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
        Align(
          alignment: AlignmentDirectional.centerEnd,
          child: FilledButton(
            key: const ValueKey<String>('proxy-auth-apply'),
            onPressed: widget.controller.busy ? null : _commit,
            child: Text(strings.get('proxy_auth_apply')),
          ),
        ),
      ],
    );
  }

  Future<void> _commit() async {
    final username = _username.text;
    final password = _password.text;
    if (!_validAuth(username, password)) {
      setState(
        () => _authError = widget.controller.strings.get('proxy_auth_invalid'),
      );
      return;
    }
    if (_authError != null) {
      setState(() => _authError = null);
    }
    final success = await widget.controller.updateProxyAuth(
      username: username,
      password: password,
    );
    if (!mounted) {
      return;
    }
    if (success) {
      _password.clear();
    }
  }

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
