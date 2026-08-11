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
            child: DropdownButtonFormField<ProxyDnsMode>(
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
          ),
        ],
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
