import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/usque_theme.dart';
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
    return _apps
        .where((app) {
          if (!app.hasInternet) return false;
          if (!_showSystem && app.isSystem) return false;
          if (query.isEmpty) return true;
          return app.label.toLowerCase().contains(query) ||
              app.packageName.toLowerCase().contains(query);
        })
        .toList(growable: false);
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
    return SubPage(
      title: strings.get('per_app_proxy'),
      subtitle: strings.get('per_app_proxy_help'),
      backLabel: strings.get('back'),
      actions: <Widget>[
        FilledButton.icon(
          onPressed: _canSave ? _save : null,
          icon: const Icon(LucideIcons.save),
          label: Text(strings.get('save')),
        ),
      ],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          BannerSlot(
            child: widget.controller.activeProfile.frontends.tunnel
                ? null
                : WarningBanner(
                    title: strings.get('per_app_proxy'),
                    message: strings.get('per_app_proxy_tunnel_hint'),
                  ),
          ),
          BannerSlot(
            child: _enabled
                ? WarningBanner(
                    title: strings.get('lockdown'),
                    message: strings.get('per_app_proxy_lockdown_help'),
                  )
                : null,
          ),
          PanelStack(
            children: <Widget>[
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
                      onChanged: (value) => setState(() => _enabled = value),
                    ),
                    const SizedBox(height: 16),
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
                      onChanged: (value) => setState(() => _showSystem = value),
                    ),
                    const SizedBox(height: 4),
                    Row(
                      children: <Widget>[
                        Expanded(
                          child: Text(
                            strings
                                .get('per_app_selected_count')
                                .replaceAll('{count}', '${_selected.length}'),
                            style: Theme.of(context).textTheme.bodyMedium
                                ?.copyWith(
                                  color: Theme.of(
                                    context,
                                  ).colorScheme.onSurfaceVariant,
                                ),
                          ),
                        ),
                        const SizedBox(width: 12),
                        Wrap(
                          spacing: 8,
                          children: <Widget>[
                            TextButton(
                              onPressed: visible.isEmpty
                                  ? null
                                  : () => setState(() {
                                      _selected.addAll(
                                        visible.map((app) => app.packageName),
                                      );
                                    }),
                              child: Text(
                                strings.get('per_app_select_visible'),
                              ),
                            ),
                            TextButton(
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
                      ],
                    ),
                    BannerSlot(
                      spacing: 0,
                      child: _enabled && !_canSave
                          ? Padding(
                              padding: const EdgeInsets.only(top: 12),
                              child: WarningBanner(
                                title: strings.get('per_app_proxy'),
                                message: strings.get('per_app_need_one'),
                                danger: true,
                              ),
                            )
                          : null,
                    ),
                  ],
                ),
              ),
              Panel(
                padding: EdgeInsets.zero,
                child: _buildAppList(context, visible),
              ),
            ],
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
      separatorBuilder: (_, _) =>
          Divider(height: 1, color: UsqueTokens.of(context).hairline),
      itemBuilder: (context, index) {
        final app = visible[index];
        final icon = _icons[app.packageName];
        return CheckboxListTile(
          contentPadding: const EdgeInsets.symmetric(horizontal: 16),
          shape: const RoundedRectangleBorder(),
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
          secondary: SizedBox(
            width: 36,
            height: 36,
            child: icon == null
                ? DecoratedBox(
                    decoration: BoxDecoration(
                      color: Theme.of(context).colorScheme.surfaceContainerHigh,
                      borderRadius: BorderRadius.circular(UsqueRadii.chip),
                    ),
                    child: const Icon(LucideIcons.layers3, size: 18),
                  )
                : ClipRRect(
                    borderRadius: BorderRadius.circular(UsqueRadii.chip),
                    child: Image.memory(icon, gaplessPlayback: true),
                  ),
          ),
          title: Text(app.label),
          subtitle: Text(
            app.packageName,
            style: UsqueTheme.mono(
              context,
              size: 11.5,
              color: Theme.of(context).colorScheme.onSurfaceVariant,
            ),
          ),
        );
      },
    );
  }
}
