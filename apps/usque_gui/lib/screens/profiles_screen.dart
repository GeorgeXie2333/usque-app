import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/profile_identity_dialog.dart';

class ProfilesScreen extends StatelessWidget {
  const ProfilesScreen({required this.controller, super.key});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    return PageFrame(
      title: strings.get('profiles'),
      actions: <Widget>[
        FilledButton.icon(
          onPressed: () => _createProfile(context),
          icon: const Icon(LucideIcons.plus),
          label: Text(strings.get('new_profile')),
        ),
      ],
      child: Column(
        children: controller.profiles
            .map(
              (profile) => Padding(
                padding: const EdgeInsets.only(bottom: 14),
                child: _ProfileCard(
                  profile: profile,
                  active: profile.id == controller.activeProfileId,
                  identityState: controller.identityState(profile.id),
                  identityStatus: controller.identityStatus(profile.id),
                  strings: strings,
                  onActivate: () => controller.setActiveProfile(profile.id),
                  onConfigureIdentity: () => showProfileIdentityDialog(
                    context,
                    controller: controller,
                    profile: profile,
                  ),
                  onEdit: () => _editProfile(context, profile, strings),
                  onManageIdentity: () => _showIdentityManagement(
                    context,
                    profile,
                    controller.identityStatus(profile.id),
                    strings,
                  ),
                  onDelete: () => _deleteProfile(context, profile, strings),
                ),
              ),
            )
            .toList(growable: false),
      ),
    );
  }

  Future<void> _createProfile(BuildContext context) async {
    await showProfileIdentityDialog(context, controller: controller);
  }

  Future<void> _editProfile(
    BuildContext context,
    UsqueProfile profile,
    AppStrings strings,
  ) async {
    final updated = await showDialog<UsqueProfile>(
      context: context,
      builder: (context) =>
          _ProfileEditDialog(strings: strings, profile: profile),
    );
    if (updated != null) {
      controller.updateProfile(updated);
    }
  }

  Future<void> _showIdentityManagement(
    BuildContext context,
    UsqueProfile profile,
    ProfileIdentityStatus status,
    AppStrings strings,
  ) async {
    await showDialog<void>(
      context: context,
      builder: (context) => _IdentityManagementDialog(
        controller: controller,
        profile: profile,
        status: status,
        strings: strings,
      ),
    );
  }

  Future<void> _deleteProfile(
    BuildContext context,
    UsqueProfile profile,
    AppStrings strings,
  ) async {
    if (controller.profiles.length == 1) {
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text(strings.get('profile_required'))));
      return;
    }
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        icon: const Icon(LucideIcons.trash2),
        title: Text(strings.get('delete_profile')),
        content: Text(strings.get('delete_profile_body')),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(strings.get('delete')),
          ),
        ],
      ),
    );
    if (confirmed ?? false) {
      controller.deleteProfile(profile.id);
    }
  }
}

class _ProfileEditDialog extends StatefulWidget {
  const _ProfileEditDialog({required this.strings, required this.profile});

  final AppStrings strings;
  final UsqueProfile profile;

  @override
  State<_ProfileEditDialog> createState() => _ProfileEditDialogState();
}

class _ProfileEditDialogState extends State<_ProfileEditDialog> {
  late final TextEditingController _controller;
  late final FocusNode _focusNode;
  late FrontendSettings _frontends;
  late bool _systemProxy;
  late bool _autoConnect;
  String? _errorText;
  bool _closing = false;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.profile.name);
    _focusNode = FocusNode();
    _frontends = widget.profile.frontends;
    _systemProxy = widget.profile.proxy.systemProxy;
    _autoConnect = widget.profile.autoConnect;
  }

  void _submit() {
    if (_closing) {
      return;
    }
    final value = _controller.text.trim();
    if (value.isEmpty) {
      setState(() => _errorText = widget.strings.get('required'));
      return;
    }
    _closing = true;
    Navigator.of(context).pop(
      widget.profile.copyWith(
        name: value,
        frontends: _frontends,
        autoConnect: _autoConnect,
        proxy: widget.profile.proxy.copyWith(
          systemProxy: _frontends.http && _systemProxy,
        ),
      ),
    );
  }

  @override
  void dispose() {
    _focusNode.dispose();
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(widget.strings.get('edit_profile')),
      content: SizedBox(
        width: 480,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: <Widget>[
              TextField(
                controller: _controller,
                focusNode: _focusNode,
                autofocus: true,
                maxLength: 64,
                textInputAction: TextInputAction.done,
                decoration: InputDecoration(
                  labelText: widget.strings.get('profile_name'),
                  errorText: _errorText,
                ),
                onChanged: (_) {
                  if (_errorText != null) setState(() => _errorText = null);
                },
                onSubmitted: (_) => _submit(),
              ),
              const Divider(),
              SwitchListTile(
                value: _frontends.tunnel,
                title: Text(widget.strings.get('tunnel_output')),
                onChanged: (value) => setState(
                  () => _frontends = _frontends.copyWith(tunnel: value),
                ),
              ),
              SwitchListTile(
                value: _frontends.socks5,
                title: const Text('SOCKS5'),
                onChanged: (value) => setState(
                  () => _frontends = _frontends.copyWith(socks5: value),
                ),
              ),
              SwitchListTile(
                value: _frontends.http,
                title: const Text('HTTP'),
                onChanged: (value) => setState(() {
                  _frontends = _frontends.copyWith(http: value);
                  if (!value) _systemProxy = false;
                }),
              ),
              if (Theme.of(context).platform == TargetPlatform.windows)
                SwitchListTile(
                  value: _systemProxy,
                  title: Text(widget.strings.get('system_proxy')),
                  onChanged: _frontends.http
                      ? (value) => setState(() => _systemProxy = value)
                      : null,
                ),
              SwitchListTile(
                value: _autoConnect,
                title: Text(widget.strings.get('auto_connect')),
                onChanged: (value) => setState(() => _autoConnect = value),
              ),
              if (!_frontends.any)
                ListTile(
                  leading: const Icon(LucideIcons.info),
                  title: Text(widget.strings.get('channel_only_warning')),
                ),
            ],
          ),
        ),
      ),
      actions: <Widget>[
        TextButton(
          onPressed: _closing ? null : () => Navigator.of(context).pop(),
          child: Text(widget.strings.get('cancel')),
        ),
        FilledButton(
          onPressed: _closing ? null : _submit,
          child: Text(widget.strings.get('save')),
        ),
      ],
    );
  }
}

class _IdentityManagementDialog extends StatefulWidget {
  const _IdentityManagementDialog({
    required this.controller,
    required this.profile,
    required this.status,
    required this.strings,
  });

  final AppController controller;
  final UsqueProfile profile;
  final ProfileIdentityStatus status;
  final AppStrings strings;

  @override
  State<_IdentityManagementDialog> createState() =>
      _IdentityManagementDialogState();
}

class _IdentityManagementDialogState extends State<_IdentityManagementDialog> {
  final TextEditingController _licenseController = TextEditingController();
  bool _showLicense = false;
  bool _busy = false;
  String? _error;

  @override
  void dispose() {
    _licenseController
      ..clear()
      ..dispose();
    super.dispose();
  }

  Future<void> _updateLicense() async {
    final value = _licenseController.text.trim();
    _licenseController.clear();
    if (value.isEmpty) {
      setState(() => _error = widget.strings.get('required'));
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    final success = await widget.controller.updateLicenseKey(
      widget.profile.id,
      value,
    );
    if (!mounted) return;
    if (success) {
      Navigator.of(context).pop();
    } else {
      setState(() {
        _busy = false;
        _error = widget.controller.lastError;
      });
    }
  }

  Future<void> _unbind() async {
    setState(() => _busy = true);
    final success = await widget.controller.unbindLicenseKey(widget.profile.id);
    if (!mounted) return;
    if (success) {
      Navigator.of(context).pop();
    } else {
      setState(() {
        _busy = false;
        _error = widget.controller.lastError;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final plus = widget.status.licenseState == LicenseState.warpPlus;
    return AlertDialog(
      icon: const Icon(LucideIcons.keyRound),
      title: Text(widget.strings.get('identity_and_license')),
      content: SizedBox(
        width: 480,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            ListTile(
              contentPadding: EdgeInsets.zero,
              leading: Icon(plus ? LucideIcons.badgeCheck : LucideIcons.user),
              title: Text(
                widget.status.accountType.isEmpty
                    ? (plus ? 'WARP+' : 'Free')
                    : widget.status.accountType,
              ),
              subtitle: widget.status.cleanupPending
                  ? Text(widget.strings.get('license_cleanup_pending'))
                  : null,
              trailing: plus
                  ? IconButton(
                      tooltip: widget.strings.get('copy_license'),
                      onPressed: _busy
                          ? null
                          : () => widget.controller.copyLicenseKey(
                              widget.profile.id,
                            ),
                      icon: const Icon(LucideIcons.copy),
                    )
                  : null,
            ),
            const SizedBox(height: 12),
            TextField(
              controller: _licenseController,
              obscureText: !_showLicense,
              enableSuggestions: false,
              autocorrect: false,
              decoration: InputDecoration(
                labelText: widget.strings.get('warp_license_key'),
                errorText: _error,
                suffixIcon: IconButton(
                  onPressed: () => setState(() => _showLicense = !_showLicense),
                  icon: Icon(
                    _showLicense ? LucideIcons.eyeOff : LucideIcons.eye,
                  ),
                ),
              ),
              onSubmitted: (_) => _updateLicense(),
            ),
            const SizedBox(height: 12),
            OutlinedButton.icon(
              onPressed: _busy ? null : _updateLicense,
              icon: const Icon(LucideIcons.refreshCw),
              label: Text(widget.strings.get('change_license')),
            ),
            if (plus)
              TextButton.icon(
                onPressed: _busy ? null : _unbind,
                icon: const Icon(LucideIcons.unlink),
                label: Text(widget.strings.get('unbind_license')),
              ),
            TextButton.icon(
              onPressed: _busy
                  ? null
                  : () => widget.controller.exportWarpSecret(widget.profile.id),
              icon: const Icon(LucideIcons.download),
              label: Text(widget.strings.get('export_warp_secret')),
            ),
          ],
        ),
      ),
      actions: <Widget>[
        TextButton(
          onPressed: _busy ? null : () => Navigator.of(context).pop(),
          child: Text(widget.strings.get('close')),
        ),
      ],
    );
  }
}

class _ProfileCard extends StatelessWidget {
  const _ProfileCard({
    required this.profile,
    required this.active,
    required this.identityState,
    required this.identityStatus,
    required this.strings,
    required this.onActivate,
    required this.onConfigureIdentity,
    required this.onEdit,
    required this.onManageIdentity,
    required this.onDelete,
  });

  final UsqueProfile profile;
  final bool active;
  final ProfileIdentityState identityState;
  final ProfileIdentityStatus identityStatus;
  final AppStrings strings;
  final VoidCallback onActivate;
  final VoidCallback onConfigureIdentity;
  final VoidCallback onEdit;
  final VoidCallback onManageIdentity;
  final VoidCallback onDelete;

  @override
  Widget build(BuildContext context) {
    return Panel(
      child: LayoutBuilder(
        builder: (context, constraints) {
          final compact = constraints.maxWidth < 620;
          final details = Wrap(
            spacing: 10,
            runSpacing: 8,
            children: <Widget>[
              if (profile.frontends.tunnel)
                _ProfileTag(
                  icon: LucideIcons.shield,
                  label: strings.get('tunnel_output'),
                ),
              if (profile.frontends.socks5)
                const _ProfileTag(icon: LucideIcons.network, label: 'SOCKS5'),
              if (profile.frontends.http)
                const _ProfileTag(icon: LucideIcons.globe2, label: 'HTTP'),
              if (!profile.frontends.any)
                _ProfileTag(
                  icon: LucideIcons.cable,
                  label: strings.get('channel_only'),
                ),
              _ProfileTag(
                icon: LucideIcons.cable,
                label: _transportLabel(strings, profile.transport),
              ),
              _ProfileTag(
                icon: LucideIcons.server,
                label: '${profile.endpointIpv4}:${profile.endpointPort}',
              ),
              _ProfileTag(
                icon: identityState == ProfileIdentityState.ready
                    ? LucideIcons.keyRound
                    : LucideIcons.triangleAlert,
                label: strings.get(switch (identityState) {
                  ProfileIdentityState.ready => 'identity_ready',
                  ProfileIdentityState.missing => 'identity_missing',
                  ProfileIdentityState.invalid => 'identity_invalid',
                }),
              ),
              if (profile.killSwitch)
                _ProfileTag(
                  icon: LucideIcons.shieldCheck,
                  label: strings.get('kill_switch'),
                ),
            ],
          );
          final actions = Wrap(
            spacing: 6,
            children: <Widget>[
              if (!active)
                TextButton.icon(
                  onPressed: onActivate,
                  icon: const Icon(LucideIcons.check, size: 18),
                  label: Text(strings.get('set_active')),
                ),
              if (identityState != ProfileIdentityState.ready)
                TextButton.icon(
                  onPressed: onConfigureIdentity,
                  icon: const Icon(LucideIcons.keyRound, size: 18),
                  label: Text(strings.get('configure_identity')),
                ),
              if (identityState == ProfileIdentityState.ready)
                IconButton(
                  tooltip: strings.get('identity_and_license'),
                  onPressed: onManageIdentity,
                  icon: Icon(
                    identityStatus.licenseState == LicenseState.warpPlus
                        ? LucideIcons.badgeCheck
                        : LucideIcons.keyRound,
                  ),
                ),
              IconButton(
                tooltip: strings.get('edit'),
                onPressed: onEdit,
                icon: const Icon(LucideIcons.pencil),
              ),
              IconButton(
                tooltip: strings.get('delete'),
                onPressed: onDelete,
                icon: const Icon(LucideIcons.trash2),
              ),
            ],
          );
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: <Widget>[
              Row(
                children: <Widget>[
                  DecoratedBox(
                    decoration: BoxDecoration(
                      color: Theme.of(context).colorScheme.secondaryContainer,
                      borderRadius: BorderRadius.circular(14),
                    ),
                    child: const Padding(
                      padding: EdgeInsets.all(12),
                      child: Icon(LucideIcons.layers3, size: 22),
                    ),
                  ),
                  const SizedBox(width: 14),
                  Expanded(
                    child: Text(
                      profile.name,
                      style: Theme.of(context).textTheme.titleLarge,
                    ),
                  ),
                  if (active)
                    StatusPill(
                      label: strings.get('active'),
                      tone: StatusTone.success,
                    ),
                  if (!compact) ...<Widget>[const SizedBox(width: 10), actions],
                ],
              ),
              const SizedBox(height: 18),
              details,
              if (compact) ...<Widget>[
                const Divider(height: 28),
                Align(alignment: Alignment.centerRight, child: actions),
              ],
            ],
          );
        },
      ),
    );
  }
}

class _ProfileTag extends StatelessWidget {
  const _ProfileTag({required this.icon, required this.label});

  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainerLow,
        borderRadius: BorderRadius.circular(999),
        border: Border.all(color: Theme.of(context).dividerColor),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 7),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            Icon(icon, size: 15),
            const SizedBox(width: 6),
            Text(label, style: Theme.of(context).textTheme.labelMedium),
          ],
        ),
      ),
    );
  }
}

String _transportLabel(AppStrings strings, TransportPolicy transport) {
  return strings.get(switch (transport) {
    TransportPolicy.automatic => 'automatic',
    TransportPolicy.http3 => 'http3',
    TransportPolicy.http2 => 'http2',
  });
}
