import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/profile_identity_dialog.dart';
import '../widgets/usque_dialog.dart';

class ProfilesScreen extends StatelessWidget {
  const ProfilesScreen({required this.controller, super.key});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    return PageFrame(
      title: strings.get('profiles'),
      subtitle: strings.get('profiles_subtitle'),
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
    final zeroTrust =
        controller.identityStatus(profile.id).provider ==
        IdentityProvider.zeroTrust;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: LucideIcons.trash2,
        title: strings.get('delete_profile'),
        subtitle: profile.name,
        danger: true,
        width: 420,
        content: Text(
          strings.get(
            zeroTrust
                ? 'delete_zero_trust_profile_body'
                : 'delete_profile_body',
          ),
        ),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton(
            style: FilledButton.styleFrom(
              backgroundColor: UsqueTokens.of(context).danger,
              foregroundColor: Theme.of(context).colorScheme.onError,
            ),
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
    return UsqueDialog(
      icon: LucideIcons.pencil,
      title: widget.strings.get('edit_profile'),
      subtitle: widget.profile.name,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
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
          const SizedBox(height: 6),
          DialogGroup(
            padding: const EdgeInsets.symmetric(vertical: 4),
            child: Column(
              children: <Widget>[
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
              ],
            ),
          ),
          if (!_frontends.any) ...<Widget>[
            const SizedBox(height: 12),
            WarningBanner(
              title: widget.strings.get('channel_only'),
              message: widget.strings.get('channel_only_warning'),
            ),
          ],
        ],
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

  Future<void> _reauthenticateZeroTrust() async {
    final success = await showProfileIdentityDialog(
      context,
      controller: widget.controller,
      profile: widget.profile,
    );
    if (success && mounted) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final plus = widget.status.licenseState == LicenseState.warpPlus;
    final zeroTrust = widget.status.provider == IdentityProvider.zeroTrust;
    return UsqueDialog(
      icon: zeroTrust ? LucideIcons.building2 : LucideIcons.keyRound,
      title: widget.strings.get('identity_and_license'),
      subtitle: widget.profile.name,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          DialogGroup(
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 4),
            child: ListTile(
              contentPadding: EdgeInsets.zero,
              leading: Icon(
                zeroTrust
                    ? LucideIcons.building2
                    : plus
                    ? LucideIcons.badgeCheck
                    : LucideIcons.user,
              ),
              title: Text(
                widget.status.accountType.isEmpty
                    ? (zeroTrust
                          ? 'Zero Trust'
                          : plus
                          ? 'WARP+'
                          : 'Free')
                    : widget.status.accountType,
              ),
              subtitle: zeroTrust
                  ? Text(
                      '${widget.status.organization}\n${widget.strings.get('license_not_applicable')}',
                    )
                  : widget.status.cleanupPending
                  ? Text(widget.strings.get('license_cleanup_pending'))
                  : null,
              isThreeLine: zeroTrust,
              trailing: plus && !zeroTrust
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
          ),
          if (zeroTrust) ...<Widget>[
            const SizedBox(height: 12),
            FilledButton.tonalIcon(
              onPressed: _busy ? null : _reauthenticateZeroTrust,
              icon: const Icon(LucideIcons.logIn),
              label: Text(widget.strings.get('zero_trust_reauthenticate')),
            ),
            const SizedBox(height: 8),
            Text(
              widget.strings.get('zero_trust_admin_cleanup_note'),
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ] else ...<Widget>[
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
        ],
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
    final UsqueTokens tokens = UsqueTokens.of(context);
    final bool identityReady = identityState == ProfileIdentityState.ready;
    return Panel(
      accent: active ? tokens.brand : null,
      child: LayoutBuilder(
        builder: (context, constraints) {
          final compact = constraints.maxWidth < 620;
          final details = Wrap(
            spacing: 8,
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
                mono: true,
              ),
              _ProfileTag(
                icon: identityReady
                    ? LucideIcons.keyRound
                    : LucideIcons.triangleAlert,
                tone: identityReady ? null : tokens.caution,
                label: strings.get(switch (identityState) {
                  ProfileIdentityState.ready => 'identity_ready',
                  ProfileIdentityState.missing => 'identity_missing',
                  ProfileIdentityState.invalid => 'identity_invalid',
                }),
              ),
              if (identityStatus.provider == IdentityProvider.zeroTrust)
                _ProfileTag(
                  icon: LucideIcons.building2,
                  label: identityStatus.organization.isEmpty
                      ? 'Zero Trust · ${strings.get('experimental')}'
                      : '${identityStatus.organization} · ${strings.get('experimental')}',
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
                  Container(
                    width: 40,
                    height: 40,
                    alignment: Alignment.center,
                    decoration: BoxDecoration(
                      color:
                          (active
                                  ? tokens.brand
                                  : Theme.of(
                                      context,
                                    ).colorScheme.onSurfaceVariant)
                              .withValues(alpha: tokens.tint),
                      borderRadius: BorderRadius.circular(UsqueRadii.control),
                    ),
                    child: Icon(
                      LucideIcons.layers3,
                      size: 19,
                      color: active
                          ? tokens.brand
                          : Theme.of(context).colorScheme.onSurfaceVariant,
                    ),
                  ),
                  const SizedBox(width: 13),
                  Expanded(
                    child: Text(
                      profile.name,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
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
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 14),
                  child: Divider(height: 1, color: tokens.hairline),
                ),
                Align(alignment: Alignment.centerRight, child: actions),
              ],
            ],
          );
        },
      ),
    );
  }
}

/// One fact about a profile. Outlined rather than filled, so a card full of
/// them still reads as a single object.
class _ProfileTag extends StatelessWidget {
  const _ProfileTag({
    required this.icon,
    required this.label,
    this.tone,
    this.mono = false,
  });

  final IconData icon;
  final String label;

  /// Overrides the neutral colour when the fact needs attention.
  final Color? tone;
  final bool mono;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final Color foreground = tone ?? theme.colorScheme.onSurfaceVariant;
    return DecoratedBox(
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(UsqueRadii.pill),
        border: Border.all(
          color: tone == null
              ? UsqueTokens.of(context).hairline
              : tone!.withValues(alpha: 0.4),
        ),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            Icon(icon, size: 14, color: foreground),
            const SizedBox(width: 7),
            Text(
              label,
              style: mono
                  ? UsqueTheme.mono(context, size: 12, color: foreground)
                  : theme.textTheme.labelMedium?.copyWith(color: foreground),
            ),
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
