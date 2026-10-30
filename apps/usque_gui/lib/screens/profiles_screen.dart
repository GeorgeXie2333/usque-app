import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/chain_strings.dart';
import '../core/user_facing_errors.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/profile_identity_dialog.dart';
import '../widgets/usque_dialog.dart';

export 'profiles_hub_screen.dart' show ProfilesScreen;

class AccountsScreen extends StatelessWidget {
  const AccountsScreen({
    required this.controller,
    this.embedded = false,
    super.key,
  });

  final bool embedded;

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final content = controller.profileIdentitiesLoading
        ? const Center(child: CircularProgressIndicator())
        : ContentList(
            children: controller.profiles
                .map(
                  (profile) => _ProfileRow(
                    profile: profile,
                    active: profile.id == controller.activeProfileId,
                    identityState: controller.identityState(profile.id),
                    identityStatus: controller.identityStatus(profile.id),
                    strings: strings,
                    canDelete: controller.profiles.length > 1,
                    onActivate: () =>
                        _activateProfile(context, profile, strings),
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
                )
                .toList(growable: false),
          );
    // Profiles places the create action beside its tab selector.
    if (embedded) return content;
    return PageFrame(
      title: strings.get('scenario_accounts'),
      contentWidth: 880,
      actions: [AddAccountButton(controller: controller)],
      child: content,
    );
  }

  /// Switching the account of a live session reconnects it, so that case
  /// asks first. A disconnected or failed session switches directly.
  Future<void> _activateProfile(
    BuildContext context,
    UsqueProfile profile,
    AppStrings strings,
  ) async {
    final snapshot = controller.snapshot;
    if (snapshot.isConnected || snapshot.isTransitional) {
      final confirmed = await showDialog<bool>(
        context: context,
        builder: (context) => UsqueDialog(
          icon: LucideIcons.refreshCw,
          title: strings.get('set_active'),
          subtitle: profile.name,
          width: 420,
          content: Text(strings.get('switch_account_reconnect_body')),
          actions: <Widget>[
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: Text(strings.get('cancel')),
            ),
            FilledButton(
              key: const ValueKey('profile-switch-confirm'),
              onPressed: () => Navigator.pop(context, true),
              child: Text(strings.get('set_active')),
            ),
          ],
        ),
      );
      if (!(confirmed ?? false)) return;
    }
    controller.setActiveProfile(profile.id);
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
      controller.renameProfile(updated.id, updated.name);
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

/// Starts adding a WARP account; shared by Accounts and the Profiles tabs.
class AddAccountButton extends StatelessWidget {
  const AddAccountButton({required this.controller, super.key});

  final AppController controller;

  @override
  Widget build(BuildContext context) => FilledButton.icon(
    onPressed: controller.profileIdentitiesLoading
        ? null
        : () => showProfileIdentityDialog(context, controller: controller),
    icon: const Icon(LucideIcons.plus),
    label: Text(controller.strings.get('new_profile')),
  );
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
  String? _errorText;
  bool _closing = false;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.profile.name);
    _focusNode = FocusNode();
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
    Navigator.of(context).pop(widget.profile.copyWith(name: value));
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
    if (_busy) widget.controller.cancelIdentityFlow(widget.profile.id);
    _licenseController
      ..clear()
      ..dispose();
    super.dispose();
  }

  Future<void> _updateLicense() async {
    if (_busy) return;
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
    try {
      final success = await widget.controller.updateLicenseKey(
        widget.profile.id,
        value,
      );
      if (!mounted) return;
      if (success) {
        Navigator.of(context).pop();
      } else {
        setState(() => _error = widget.controller.lastError);
      }
    } on Object catch (error) {
      if (mounted) {
        setState(() => _error = userFacingError(widget.strings, error));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _unbind() async {
    if (_busy) return;
    setState(() => _busy = true);
    try {
      final success = await widget.controller.unbindLicenseKey(
        widget.profile.id,
      );
      if (!mounted) return;
      if (success) {
        Navigator.of(context).pop();
      } else {
        setState(() => _error = widget.controller.lastError);
      }
    } on Object catch (error) {
      if (mounted) {
        setState(() => _error = userFacingError(widget.strings, error));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
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
              title: Text(_accountIdentityLabel(widget.status, widget.strings)),
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
              enabled: !_busy,
              obscureText: !_showLicense,
              enableSuggestions: false,
              autocorrect: false,
              decoration: InputDecoration(
                labelText: widget.strings.get('warp_license_key'),
                errorText: _error,
                suffixIcon: IconButton(
                  onPressed: _busy
                      ? null
                      : () => setState(() => _showLicense = !_showLicense),
                  icon: Icon(
                    _showLicense ? LucideIcons.eyeOff : LucideIcons.eye,
                  ),
                ),
              ),
              onSubmitted: _busy ? null : (_) => _updateLicense(),
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

class _ProfileRow extends StatelessWidget {
  const _ProfileRow({
    required this.profile,
    required this.active,
    required this.identityState,
    required this.identityStatus,
    required this.strings,
    required this.canDelete,
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
  final bool canDelete;
  final VoidCallback onActivate;
  final VoidCallback onConfigureIdentity;
  final VoidCallback onEdit;
  final VoidCallback onManageIdentity;
  final VoidCallback onDelete;

  @override
  Widget build(BuildContext context) {
    final UsqueTokens tokens = UsqueTokens.of(context);
    final bool identityReady = identityState == ProfileIdentityState.ready;
    return Semantics(
      selected: active,
      container: true,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 24),
        decoration: BoxDecoration(
          color: active
              ? Theme.of(
                  context,
                ).colorScheme.primary.withValues(alpha: tokens.tint * 0.55)
              : null,
          border: BorderDirectional(
            start: BorderSide(
              width: 3,
              color: active
                  ? Theme.of(context).colorScheme.primary
                  : Colors.transparent,
            ),
          ),
        ),
        child: LayoutBuilder(
          builder: (context, constraints) {
            final compact =
                constraints.maxWidth < 620 ||
                MediaQuery.textScalerOf(context).scale(14) > 21;
            final identityTag = _ProfileTag(
              icon: identityStatus.provider == IdentityProvider.zeroTrust
                  ? LucideIcons.building2
                  : identityStatus.licenseState == LicenseState.warpPlus
                  ? LucideIcons.badgeCheck
                  : identityReady
                  ? LucideIcons.user
                  : LucideIcons.triangleAlert,
              tone: identityReady ? null : tokens.caution,
              label: _accountIdentityLabel(identityStatus, strings),
            );
            final activePill = active
                ? InlineStatus(
                    label: strings.get('active'),
                    tone: StatusTone.success,
                  )
                : null;
            // Wrapped runs keep the trailing edge the actions are aligned to.
            final actions = Wrap(
              spacing: 6,
              alignment: WrapAlignment.end,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: <Widget>[
                // A switch action, not a completed state: no check mark.
                if (!active)
                  TextButton.icon(
                    onPressed: onActivate,
                    icon: const Icon(LucideIcons.arrowLeftRight, size: 18),
                    label: Text(strings.get('set_active')),
                  ),
                if (identityState != ProfileIdentityState.ready)
                  TextButton.icon(
                    onPressed: onConfigureIdentity,
                    icon: const Icon(LucideIcons.keyRound, size: 18),
                    label: Text(strings.get('configure_identity')),
                  ),
                // Labelled, because touch screens never show a tooltip; the
                // tooltip keeps the full action name.
                if (identityState == ProfileIdentityState.ready)
                  Tooltip(
                    message: strings.get('identity_and_license'),
                    child: TextButton.icon(
                      onPressed: onManageIdentity,
                      icon: const Icon(LucideIcons.keyRound, size: 18),
                      label: Text(strings.get('license')),
                    ),
                  ),
                Tooltip(
                  message: strings.get('edit_profile'),
                  child: TextButton.icon(
                    onPressed: onEdit,
                    icon: const Icon(LucideIcons.pencil, size: 18),
                    label: Text(strings.chain('rename')),
                  ),
                ),
                IconButton(
                  key: ValueKey('profile-delete-${profile.id}'),
                  tooltip: strings.get(
                    canDelete ? 'delete' : 'profile_required',
                  ),
                  onPressed: canDelete ? onDelete : null,
                  icon: const Icon(LucideIcons.trash2),
                ),
              ],
            );
            final name = Text(
              profile.name,
              maxLines: compact ? 2 : 1,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(context).textTheme.titleLarge,
            );
            // Name and its tags form one block in every layout; the icon
            // follows the name's line. Wide rows centre the actions on the
            // block, narrow ones move them below a hairline.
            final identity = Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                // Layers marks scenes; an account keeps a person glyph.
                Icon(
                  active ? LucideIcons.circleCheck : LucideIcons.circleUser,
                  size: 24,
                  color: active
                      ? Theme.of(context).colorScheme.primary
                      : Theme.of(context).colorScheme.onSurfaceVariant,
                ),
                const SizedBox(width: 13),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: <Widget>[
                      name,
                      const SizedBox(height: 8),
                      Wrap(
                        spacing: 8,
                        runSpacing: 6,
                        children: <Widget>[identityTag, ?activePill],
                      ),
                    ],
                  ),
                ),
              ],
            );
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                if (compact)
                  identity
                else
                  Row(
                    children: <Widget>[
                      Expanded(child: identity),
                      const SizedBox(width: 16),
                      actions,
                    ],
                  ),
                if (compact) ...<Widget>[
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 14),
                    child: Divider(height: 1, color: tokens.hairline),
                  ),
                  Align(
                    alignment: AlignmentDirectional.centerEnd,
                    child: actions,
                  ),
                ],
              ],
            );
          },
        ),
      ),
    );
  }
}

String _accountIdentityLabel(ProfileIdentityStatus status, AppStrings strings) {
  if (status.provider == IdentityProvider.zeroTrust) {
    return 'Zero Trust';
  }
  if (status.licenseState == LicenseState.warpPlus) {
    return 'WARP+';
  }
  if (status.state != ProfileIdentityState.ready) {
    return strings.get(switch (status.state) {
      ProfileIdentityState.missing => 'identity_missing',
      ProfileIdentityState.invalid => 'identity_invalid',
      ProfileIdentityState.ready => 'warp_free',
    });
  }
  return strings.get('warp_free');
}

/// One fact about a profile. Outlined rather than filled, so a card full of
/// them still reads as a single object.
class _ProfileTag extends StatelessWidget {
  const _ProfileTag({required this.icon, required this.label, this.tone});

  final IconData icon;
  final String label;

  /// Overrides the neutral colour when the fact needs attention.
  final Color? tone;

  @override
  Widget build(BuildContext context) {
    return InlineStatus(
      label: label,
      tone: tone == null ? StatusTone.neutral : StatusTone.warning,
      icon: icon,
    );
  }
}
