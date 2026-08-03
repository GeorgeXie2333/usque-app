import 'package:flutter/foundation.dart';
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
      subtitle: strings.get('mode_help'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          Panel(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                SectionTitle(
                  icon: LucideIcons.waypoints,
                  title: strings.get('mode'),
                  subtitle: strings.get('mode_help'),
                ),
                const SizedBox(height: 22),
                SegmentedButton<OperatingMode>(
                  segments: <ButtonSegment<OperatingMode>>[
                    ButtonSegment<OperatingMode>(
                      value: OperatingMode.vpn,
                      icon: const Icon(LucideIcons.shield),
                      label: Text(strings.get('vpn_mode')),
                    ),
                    ButtonSegment<OperatingMode>(
                      value: OperatingMode.socks5,
                      icon: const Icon(LucideIcons.route),
                      label: Text(strings.get('socks_mode')),
                    ),
                    ButtonSegment<OperatingMode>(
                      value: OperatingMode.httpProxy,
                      icon: const Icon(LucideIcons.globe2),
                      label: Text(strings.get('http_mode')),
                    ),
                  ],
                  selected: <OperatingMode>{profile.mode},
                  onSelectionChanged: (selection) => widget.controller
                      .updateProfile(profile.copyWith(mode: selection.first)),
                  showSelectedIcon: false,
                ),
              ],
            ),
          ),
          if (profile.mode != OperatingMode.vpn) ...<Widget>[
            const SizedBox(height: 16),
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
            Panel(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: <Widget>[
                  SectionTitle(
                    icon: profile.mode == OperatingMode.socks5
                        ? LucideIcons.route
                        : LucideIcons.globe2,
                    title: strings.get(
                      profile.mode == OperatingMode.socks5
                          ? 'socks_listener'
                          : 'http_listener',
                    ),
                    subtitle: profile.mode == OperatingMode.socks5
                        ? strings.get('socks_capabilities')
                        : strings.get('http_capabilities'),
                  ),
                  const SizedBox(height: 22),
                  LayoutBuilder(
                    builder: (context, constraints) {
                      final narrow = constraints.maxWidth < 640;
                      final v4 = _AddressField(
                        label: strings.get('listen_ipv4'),
                        controller: profile.mode == OperatingMode.socks5
                            ? _socksV4
                            : _httpV4,
                        onChanged: (_) => _saveListeners(profile),
                      );
                      final v6 = _AddressField(
                        label: strings.get('listen_ipv6'),
                        controller: profile.mode == OperatingMode.socks5
                            ? _socksV6
                            : _httpV6,
                        onChanged: (_) => _saveListeners(profile),
                      );
                      final port = SizedBox(
                        width: narrow ? double.infinity : 150,
                        child: TextField(
                          controller: profile.mode == OperatingMode.socks5
                              ? _socksPort
                              : _httpPort,
                          keyboardType: TextInputType.number,
                          inputFormatters: <TextInputFormatter>[
                            FilteringTextInputFormatter.digitsOnly,
                          ],
                          decoration: InputDecoration(
                            labelText: strings.get('port'),
                          ),
                          onChanged: (_) => _saveListeners(profile),
                        ),
                      );
                      if (narrow) {
                        return Column(
                          children: <Widget>[
                            v4,
                            const SizedBox(height: 12),
                            v6,
                            const SizedBox(height: 12),
                            port,
                          ],
                        );
                      }
                      return Row(
                        children: <Widget>[
                          Expanded(child: v4),
                          const SizedBox(width: 12),
                          Expanded(child: v6),
                          const SizedBox(width: 12),
                          port,
                        ],
                      );
                    },
                  ),
                  const SizedBox(height: 14),
                  DropdownButtonFormField<ProxyDnsMode>(
                    initialValue: currentProxy.dnsMode,
                    decoration: InputDecoration(
                      prefixIcon: const Icon(LucideIcons.server),
                      labelText: strings.get('proxy_dns_mode'),
                      helperText: strings.get('remote_dns_help'),
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
                  if (profile.mode == OperatingMode.httpProxy &&
                      defaultTargetPlatform == TargetPlatform.windows)
                    SwitchListTile(
                      contentPadding: EdgeInsets.zero,
                      secondary: const Icon(LucideIcons.monitorCog),
                      title: Text(strings.get('system_proxy')),
                      value: currentProxy.systemProxy,
                      onChanged: (value) => widget.controller.updateProfile(
                        profile.copyWith(
                          proxy: currentProxy.copyWith(systemProxy: value),
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ],
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
