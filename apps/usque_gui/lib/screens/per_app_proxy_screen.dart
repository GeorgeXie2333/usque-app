import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';

class PerAppProxyScreen extends StatefulWidget {
  const PerAppProxyScreen({required this.controller, super.key});

  final AppController controller;

  @override
  State<PerAppProxyScreen> createState() => _PerAppProxyScreenState();
}

class _PerAppProxyScreenState extends State<PerAppProxyScreen> {
  final TextEditingController _search = TextEditingController();
  final Set<String> _selected = <String>{};
  final Map<String, Uint8List?> _icons = <String, Uint8List?>{};
  List<InstalledAppInfo> _apps = const <InstalledAppInfo>[];
  bool _enabled = false;
  bool _showSystem = false;
  bool _loading = true;
  String? _loadError;

  @override
  void initState() {
    super.initState();
    final current = widget.controller.perAppProxy;
    _enabled = current.enabled;
    _selected.addAll(current.packageNames);
    _loadApps();
  }

  @override
  void dispose() {
    _search.dispose();
    super.dispose();
  }

  Future<void> _loadApps() async {
    try {
      final apps = await widget.controller.listInstalledApps();
      if (!mounted) return;
      setState(() {
        _apps = apps;
        _loading = false;
      });
      for (final app in apps) {
        unawaited(_loadIcon(app.packageName));
      }
    } on Object catch (error) {
      if (!mounted) return;
      setState(() {
        _loading = false;
        _loadError = error.toString();
      });
    }
  }

  Future<void> _loadIcon(String packageName) async {
    final bytes = await widget.controller.getAppIcon(packageName);
    if (!mounted || bytes == null) return;
    setState(() => _icons[packageName] = bytes);
  }

  List<InstalledAppInfo> get _visible {
    final query = _search.text.trim().toLowerCase();
    return _apps.where((app) {
      if (!app.hasInternet) return false;
      if (!_showSystem && app.isSystem) return false;
      if (query.isEmpty) return true;
      return app.label.toLowerCase().contains(query) ||
          app.packageName.toLowerCase().contains(query);
    }).toList(growable: false);
  }

  bool get _canSave {
    if (!_enabled) return true;
    return PerAppProxySettings.sanitizePackages(_selected).isNotEmpty;
  }

  Future<void> _save() async {
    final next = PerAppProxySettings(
      enabled: _enabled,
      packageNames: PerAppProxySettings.sanitizePackages(_selected),
    );
    if (next.validationError() != null) {
      return;
    }
    await widget.controller.setPerAppProxy(next);
    if (!mounted) return;
    if (widget.controller.lastError == null && Navigator.of(context).canPop()) {
      Navigator.of(context).pop();
    }
  }

  @override
  Widget build(BuildContext context) {
    final strings = widget.controller.strings;
    final visible = _visible;
    return Scaffold(
      appBar: AppBar(
        title: Text(strings.get('per_app_proxy')),
        actions: <Widget>[
          TextButton(
            onPressed: _canSave ? _save : null,
            child: Text(strings.get('save')),
          ),
          const SizedBox(width: 8),
        ],
      ),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(24, 18, 24, 40),
        children: <Widget>[
          Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 880),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: <Widget>[
                  if (!widget.controller.activeProfile.frontends.tunnel) ...<
                    Widget
                  >[
                    WarningBanner(
                      title: strings.get('per_app_proxy'),
                      message: strings.get('per_app_proxy_tunnel_hint'),
                    ),
                    const SizedBox(height: 16),
                  ],
                  if (_enabled) ...<Widget>[
                    WarningBanner(
                      title: strings.get('lockdown'),
                      message: strings.get('per_app_proxy_lockdown_help'),
                    ),
                    const SizedBox(height: 16),
                  ],
                  Panel(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: <Widget>[
                        SwitchListTile(
                          contentPadding: EdgeInsets.zero,
                          secondary: const Icon(LucideIcons.layers3),
                          title: Text(strings.get('per_app_proxy_enable')),
                          subtitle: Text(strings.get('per_app_proxy_help')),
                          value: _enabled,
                          onChanged: (value) =>
                              setState(() => _enabled = value),
                        ),
                        const Divider(height: 28),
                        TextField(
                          controller: _search,
                          decoration: InputDecoration(
                            labelText: strings.get('per_app_search'),
                            prefixIcon: const Icon(LucideIcons.search),
                          ),
                          onChanged: (_) => setState(() {}),
                        ),
                        SwitchListTile(
                          contentPadding: EdgeInsets.zero,
                          title: Text(strings.get('per_app_show_system')),
                          value: _showSystem,
                          onChanged: (value) =>
                              setState(() => _showSystem = value),
                        ),
                        Wrap(
                          spacing: 8,
                          runSpacing: 8,
                          children: <Widget>[
                            OutlinedButton(
                              onPressed: visible.isEmpty
                                  ? null
                                  : () => setState(() {
                                      _selected.addAll(
                                        visible.map((app) => app.packageName),
                                      );
                                    }),
                              child: Text(strings.get('per_app_select_visible')),
                            ),
                            OutlinedButton(
                              onPressed: visible.isEmpty
                                  ? null
                                  : () => setState(() {
                                      _selected.removeAll(
                                        visible.map((app) => app.packageName),
                                      );
                                    }),
                              child: Text(strings.get('per_app_clear_visible')),
                            ),
                          ],
                        ),
                        const SizedBox(height: 12),
                        Text(
                          strings
                              .get('per_app_selected_count')
                              .replaceAll('{count}', '${_selected.length}'),
                          style: Theme.of(context).textTheme.bodyMedium,
                        ),
                        if (_enabled && !_canSave) ...<Widget>[
                          const SizedBox(height: 8),
                          Text(
                            strings.get('per_app_need_one'),
                            style: Theme.of(context).textTheme.bodyMedium
                                ?.copyWith(
                                  color: Theme.of(context).colorScheme.error,
                                ),
                          ),
                        ],
                      ],
                    ),
                  ),
                  const SizedBox(height: 16),
                  Panel(
                    padding: EdgeInsets.zero,
                    child: _buildAppList(context, visible),
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildAppList(BuildContext context, List<InstalledAppInfo> visible) {
    final strings = widget.controller.strings;
    if (_loading) {
      return Padding(
        padding: const EdgeInsets.all(24),
        child: Row(
          children: <Widget>[
            const SizedBox(
              width: 18,
              height: 18,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
            const SizedBox(width: 12),
            Text(strings.get('per_app_loading')),
          ],
        ),
      );
    }
    if (_loadError != null) {
      return Padding(
        padding: const EdgeInsets.all(24),
        child: Text(_loadError!),
      );
    }
    if (visible.isEmpty) {
      return Padding(
        padding: const EdgeInsets.all(24),
        child: Text(strings.get('per_app_empty')),
      );
    }
    return ListView.separated(
      shrinkWrap: true,
      physics: const NeverScrollableScrollPhysics(),
      itemCount: visible.length,
      separatorBuilder: (_, _) => const Divider(height: 1),
      itemBuilder: (context, index) {
        final app = visible[index];
        final icon = _icons[app.packageName];
        return CheckboxListTile(
          value: _selected.contains(app.packageName),
          onChanged: (checked) {
            setState(() {
              if (checked ?? false) {
                _selected.add(app.packageName);
              } else {
                _selected.remove(app.packageName);
              }
            });
          },
          secondary: icon == null
              ? const Icon(LucideIcons.layers3)
              : Image.memory(icon, width: 36, height: 36, gaplessPlayback: true),
          title: Text(app.label),
          subtitle: Text(app.packageName),
        );
      },
    );
  }
}
