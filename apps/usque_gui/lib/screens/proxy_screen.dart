import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

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
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          if (currentProxy.exposesLan) ...<Widget>[
            WarningBanner(
              title: strings.get('lan_warning'),
              message: strings.get('lan_warning_body'),
            ),
            const SizedBox(height: 16),
          ],
          if (currentProxy.dnsMode != ProxyDnsMode.remote) ...<Widget>[
            WarningBanner(
              title: strings.get('dns_leak_warning'),
              message: strings.get('dns_leak_warning_body'),
            ),
            const SizedBox(height: 16),
          ],
          _listenerPanel(context, profile, socks5: true),
          const SizedBox(height: 16),
          _listenerPanel(context, profile, socks5: false),
          const SizedBox(height: 16),
          Panel(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                DropdownButtonFormField<ProxyDnsMode>(
                  key: const ValueKey<String>('proxy-dns-mode'),
                  initialValue: currentProxy.dnsMode,
                  decoration: InputDecoration(
                    prefixIcon: const Icon(LucideIcons.server),
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
                if (currentProxy.dnsMode ==
                    ProxyDnsMode.localConfigured) ...<Widget>[
                  const SizedBox(height: 14),
                  _dnsFields(profile),
                ],
              ],
            ),
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
    return Panel(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          SectionTitle(
            icon: socks5 ? LucideIcons.route : LucideIcons.globe2,
            title: strings.get(socks5 ? 'socks_listener' : 'http_listener'),
            subtitle: strings.get(
              enabled
                  ? (socks5 ? 'socks_capabilities' : 'http_capabilities')
                  : 'output_disabled_in_profile',
            ),
          ),
          const SizedBox(height: 22),
          LayoutBuilder(
            builder: (context, constraints) {
              final narrow = constraints.maxWidth < 640;
              final fields = <Widget>[
                Expanded(
                  child: _AddressField(
                    label: strings.get('listen_ipv4'),
                    controller: v4Controller,
                    onChanged: (_) => _saveListeners(profile),
                  ),
                ),
                Expanded(
                  child: _AddressField(
                    label: strings.get('listen_ipv6'),
                    controller: v6Controller,
                    onChanged: (_) => _saveListeners(profile),
                  ),
                ),
                SizedBox(
                  width: narrow ? double.infinity : 150,
                  child: TextField(
                    controller: portController,
                    keyboardType: TextInputType.number,
                    inputFormatters: <TextInputFormatter>[
                      FilteringTextInputFormatter.digitsOnly,
                    ],
                    decoration: InputDecoration(labelText: strings.get('port')),
                    onChanged: (_) => _saveListeners(profile),
                  ),
                ),
              ];
              if (narrow) {
                return Column(
                  children: <Widget>[
                    for (var index = 0; index < fields.length; index++) ...[
                      if (fields[index] is Expanded)
                        (fields[index] as Expanded).child
                      else
                        fields[index],
                      if (index != fields.length - 1)
                        const SizedBox(height: 12),
                    ],
                  ],
                );
              }
              return Row(
                children: <Widget>[
                  fields[0],
                  const SizedBox(width: 12),
                  fields[1],
                  const SizedBox(width: 12),
                  fields[2],
                ],
              );
            },
          ),
        ],
      ),
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
