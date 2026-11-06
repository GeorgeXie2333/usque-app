import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/user_facing_errors.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/profile_identity_dialog.dart';
import '../widgets/profiles_status.dart';
import '../widgets/tv_text_input.dart';
import '../widgets/usque_dialog.dart';

export 'profiles_hub_screen.dart' show ProfilesScreen;

/// Account management lives inside the Profiles section's navigation stack.
class AccountsPage extends StatefulWidget {
  const AccountsPage({required this.controller, super.key});

  final AppController controller;

  @override
  State<AccountsPage> createState() => _AccountsPageState();
}

class _AccountsPageState extends State<AccountsPage> {
  AppController get controller => widget.controller;

  @override
  void initState() {
    super.initState();
    controller.addListener(_changed);
  }

  @override
  void didUpdateWidget(covariant AccountsPage oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != controller) {
      oldWidget.controller.removeListener(_changed);
      controller.addListener(_changed);
    }
  }

  void _changed() {
    if (!mounted ||
        !controller.uiVisible ||
        controller.section != AppSection.profiles) {
      return;
    }
    setState(() {});
  }

  @override
  void dispose() {
    controller.removeListener(_changed);
    super.dispose();
  }

  // The rows carry the selection and the live account, so the page needs no
  // summary; an uncertain result is refreshed from its banner.
  @override
  Widget build(BuildContext context) => SubPage(
    title: controller.strings.get('scenario_accounts'),
    backLabel: controller.strings.get('back'),
    contentWidth: 880,
    actions: [AddAccountButton(controller: controller)],
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        BannerSlot(
          child: profilesOperationBanner(controller, refreshable: true),
        ),
        AccountsScreen(controller: controller),
      ],
    ),
  );
}

/// The account list with its management actions. Choosing an account for a
/// scene uses [ScenarioAccountPicker] instead, which never manages accounts.
class AccountsScreen extends StatelessWidget {
  const AccountsScreen({required this.controller, super.key});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    if (controller.profileIdentitiesLoading) {
      return const Center(child: CircularProgressIndicator());
    }
    final connected = profilesConnectedAccountId(controller);
    // While an unconfirmed read may have replaced the saved choice, no row
    // claims it; the page banner reports the uncertainty.
    final selected = profilesSelectionUncertain(controller)
        ? null
        : controller.activeProfileId;
    // The owning page frames this list and places its create action.
    return ContentList(
      children: [
        for (final profile in controller.profiles)
          _ManagedAccountRow(
            profile: profile,
            selected: profile.id == selected,
            connected: profile.id == connected,
            identityState: controller.identityState(profile.id),
            identityStatus: controller.identityStatus(profile.id),
            strings: strings,
            canDelete: controller.profiles.length > 1,
            onActivate: () => _activateProfile(context, profile, strings),
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
      ],
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
          subtitle: strings.accountName(profile),
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
        subtitle: strings.accountName(profile),
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
            style: dangerButtonStyle(context),
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

/// Starts adding a WARP account; shared by management and scene binding pages.
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

/// Chooses the account a scene draft binds. A pick returns at once and only
/// changes the caller's draft; it never switches, edits or deletes accounts.
/// Adding an account stays available, since the wanted one may not exist yet.
class ScenarioAccountPicker extends StatelessWidget {
  const ScenarioAccountPicker({
    required this.controller,
    required this.boundAccountId,
    required this.onSelected,
    super.key,
  });

  final AppController controller;

  /// The draft's binding; null follows the selected account.
  final String? boundAccountId;
  final ValueChanged<String?> onSelected;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: controller,
    builder: (context, _) {
      final strings = controller.strings;
      return SubPage(
        title: strings.get('scenario_binding'),
        subtitle: strings.get('scenario_account_hint'),
        backLabel: strings.get('back'),
        contentWidth: 880,
        actions: [AddAccountButton(controller: controller)],
        child: controller.profileIdentitiesLoading
            ? const Center(child: CircularProgressIndicator())
            : ContentList(
                children: [
                  _PickerAccountRow(
                    key: const ValueKey('scenario-follow-account'),
                    checked: boundAccountId == null,
                    title: strings.get('scenario_follow_account'),
                    detail: Text(
                      strings
                          .get('scenario_follow_account_hint')
                          .replaceAll(
                            '{account}',
                            profilesSelectedAccountLabel(controller),
                          ),
                    ),
                    onPick: () => onSelected(null),
                  ),
                  for (final profile in controller.profiles)
                    _PickerAccountRow(
                      key: ValueKey('scene-select-account-${profile.id}'),
                      checked: profile.id == boundAccountId,
                      title: strings.accountName(profile),
                      detail: _AccountIdentityLine(
                        state: controller.identityState(profile.id),
                        status: controller.identityStatus(profile.id),
                        strings: strings,
                      ),
                      onPick: () => onSelected(profile.id),
                    ),
                ],
              ),
      );
    },
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
    _controller = TextEditingController(
      text: widget.strings.accountName(widget.profile),
    );
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
      subtitle: widget.strings.accountName(widget.profile),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          tvTextField(
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
      subtitle: widget.strings.accountName(widget.profile),
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
            tvTextField(
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

/// The account's kind. Selection has its own marks and never changes it.
IconData _accountGlyph(ProfileIdentityStatus status) =>
    status.provider == IdentityProvider.zeroTrust
    ? LucideIcons.building2
    : status.licenseState == LicenseState.warpPlus
    ? LucideIcons.badgeCheck
    : LucideIcons.user;

/// One managed account: its kind, name and state, at most one inline action
/// and a labelled menu for the rest, in one block at every width.
class _ManagedAccountRow extends StatelessWidget {
  const _ManagedAccountRow({
    required this.profile,
    required this.selected,
    required this.connected,
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
  final bool selected, connected, canDelete;
  final ProfileIdentityState identityState;
  final ProfileIdentityStatus identityStatus;
  final AppStrings strings;
  final VoidCallback onActivate;
  final VoidCallback onConfigureIdentity;
  final VoidCallback onEdit;
  final VoidCallback onManageIdentity;
  final VoidCallback onDelete;

  Widget _menuItem(
    BuildContext context,
    IconData icon,
    String label, {
    bool danger = false,
  }) {
    final color = danger ? statusToneColor(context, StatusTone.danger) : null;
    return Row(
      children: <Widget>[
        Icon(icon, size: 18, color: color),
        const SizedBox(width: 12),
        Flexible(
          child: Text(
            label,
            style: color == null ? null : TextStyle(color: color),
          ),
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final tokens = UsqueTokens.of(context);
    final ready = identityState == ProfileIdentityState.ready;
    final tags = <Widget>[
      // A saved choice, not a confirmed connection: brand, no check.
      if (selected)
        InlineStatus(
          label: strings.get('account_selected'),
          tone: StatusTone.brand,
          icon: LucideIcons.circleDot,
        ),
      // Only a session the Engine confirms; an uncertain one is reported by
      // the page banner instead.
      if (connected)
        InlineStatus(
          key: ValueKey('profile-connected-${profile.id}'),
          label: strings.get('account_connected'),
          tone: StatusTone.success,
        ),
    ];
    // Identity repair, license, rename and delete share one labelled menu,
    // so touch screens need no tooltips and delete comes last, after a rule.
    final menu = PopupMenuButton<String>(
      key: ValueKey('profile-menu-${profile.id}'),
      onSelected: (value) {
        switch (value) {
          case 'license':
            onManageIdentity();
          case 'setup':
            onConfigureIdentity();
          case 'rename':
            onEdit();
          case 'delete':
            onDelete();
        }
      },
      itemBuilder: (context) => <PopupMenuEntry<String>>[
        PopupMenuItem(
          key: ValueKey('profile-menu-identity-${profile.id}'),
          value: ready ? 'license' : 'setup',
          child: _menuItem(
            context,
            LucideIcons.keyRound,
            strings.get(ready ? 'identity_and_license' : 'configure_identity'),
          ),
        ),
        PopupMenuItem(
          value: 'rename',
          child: _menuItem(
            context,
            LucideIcons.pencil,
            strings.get('edit_profile'),
          ),
        ),
        const PopupMenuDivider(),
        // The disabled entry names why, since it cannot be chosen.
        PopupMenuItem(
          key: ValueKey('profile-menu-delete-${profile.id}'),
          value: 'delete',
          enabled: canDelete,
          child: _menuItem(
            context,
            LucideIcons.trash2,
            strings.get(canDelete ? 'delete' : 'profile_required'),
            danger: canDelete,
          ),
        ),
      ],
    );
    return Semantics(
      selected: selected,
      container: true,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 18),
        decoration: BoxDecoration(
          color: selected
              ? theme.colorScheme.primary.withValues(alpha: tokens.tint * 0.55)
              : null,
          border: BorderDirectional(
            start: BorderSide(
              width: 3,
              color: selected ? theme.colorScheme.primary : Colors.transparent,
            ),
          ),
        ),
        child: LayoutBuilder(
          builder: (context, constraints) {
            final compact =
                constraints.maxWidth < 620 ||
                MediaQuery.textScalerOf(context).scale(14) > 21;
            // Switching is the only inline action, and a switch, not a
            // completed state: no check mark. Under the text it starts on
            // the text's edge.
            final activate = selected
                ? null
                : TextButton.icon(
                    key: ValueKey('profile-activate-${profile.id}'),
                    onPressed: onActivate,
                    style: compact
                        ? TextButton.styleFrom(
                            alignment: AlignmentDirectional.centerStart,
                            padding: const EdgeInsetsDirectional.fromSTEB(
                              0,
                              8,
                              12,
                              8,
                            ),
                          )
                        : null,
                    icon: const Icon(LucideIcons.arrowLeftRight, size: 18),
                    label: Text(strings.get('set_active')),
                  );
            final details = Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Text(
                  strings.accountName(profile),
                  maxLines: compact ? 2 : 1,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.titleLarge,
                ),
                const SizedBox(height: 6),
                Wrap(
                  spacing: 12,
                  runSpacing: 6,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: <Widget>[
                    _AccountIdentityLine(
                      state: identityState,
                      status: identityStatus,
                      strings: strings,
                    ),
                    ...tags,
                  ],
                ),
                if (compact && activate != null) ...<Widget>[
                  const SizedBox(height: 6),
                  activate,
                ],
              ],
            );
            return Row(
              crossAxisAlignment: compact
                  ? CrossAxisAlignment.start
                  : CrossAxisAlignment.center,
              children: <Widget>[
                _AccountGlyphTile(
                  icon: _accountGlyph(identityStatus),
                  lit: selected,
                ),
                const SizedBox(width: 14),
                Expanded(child: details),
                const SizedBox(width: 12),
                if (!compact && activate != null) ...<Widget>[
                  activate,
                  const SizedBox(width: 4),
                ],
                menu,
              ],
            );
          },
        ),
      ),
    );
  }
}

/// One option of [ScenarioAccountPicker]. Activating it picks the option
/// and returns; arrow keys only move focus, so browsing never picks.
class _PickerAccountRow extends StatelessWidget {
  const _PickerAccountRow({
    required this.checked,
    required this.title,
    required this.detail,
    required this.onPick,
    super.key,
  });

  final bool checked;
  final String title;
  final Widget detail;
  final VoidCallback onPick;

  // One node holds the label, the checked state and the activation, as a
  // radio list tile's would.
  @override
  Widget build(BuildContext context) => MergeSemantics(
    child: Semantics(
      checked: checked,
      inMutuallyExclusiveGroup: true,
      child: ListTile(
        contentPadding: const EdgeInsetsDirectional.only(start: 16, end: 16),
        minLeadingWidth: 24,
        horizontalTitleGap: 16,
        minTileHeight: 72,
        minVerticalPadding: 12,
        selected: checked,
        selectedTileColor: Theme.of(
          context,
        ).colorScheme.primary.withValues(alpha: .06),
        // A pick, not a confirmed connection: a dot, no check.
        leading: Icon(
          checked ? LucideIcons.circleDot : LucideIcons.circle,
          size: 24,
        ),
        title: Text(title, maxLines: 2, overflow: TextOverflow.ellipsis),
        subtitle: Padding(
          padding: const EdgeInsets.only(top: 2),
          child: detail,
        ),
        onTap: onPick,
      ),
    ),
  );
}

/// The account's kind beside its name, tinted while it is the selection.
class _AccountGlyphTile extends StatelessWidget {
  const _AccountGlyphTile({required this.icon, required this.lit});

  final IconData icon;
  final bool lit;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return ExcludeSemantics(
      child: Container(
        width: 36,
        height: 36,
        decoration: BoxDecoration(
          color: lit
              ? theme.colorScheme.primary.withValues(
                  alpha: UsqueTokens.of(context).tint,
                )
              : theme.colorScheme.surfaceContainerHigh,
          borderRadius: BorderRadius.circular(UsqueRadii.chip),
        ),
        child: Icon(
          icon,
          size: 18,
          color: lit
              ? theme.colorScheme.primary
              : theme.colorScheme.onSurfaceVariant,
        ),
      ),
    );
  }
}

/// The account's kind in words (with a Zero Trust organization), or why its
/// identity cannot be used. Normal flows always provision an identity, so the
/// warning appears only after a credential is lost or damaged.
class _AccountIdentityLine extends StatelessWidget {
  const _AccountIdentityLine({
    required this.state,
    required this.status,
    required this.strings,
  });

  final ProfileIdentityState state;
  final ProfileIdentityStatus status;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    if (state != ProfileIdentityState.ready) {
      return InlineStatus(
        label: strings.get(
          state == ProfileIdentityState.missing
              ? 'identity_missing'
              : 'identity_invalid',
        ),
        tone: StatusTone.warning,
      );
    }
    final theme = Theme.of(context);
    final zeroTrust = status.provider == IdentityProvider.zeroTrust;
    final organization = zeroTrust && status.organization.isNotEmpty
        ? ' · ${status.organization}'
        : '';
    // Product names read left to right, so right-to-left text keeps the plus
    // of WARP+ after the name; the line still starts on the reading side.
    final latin = zeroTrust || status.licenseState == LicenseState.warpPlus;
    final rtl = Directionality.of(context) == TextDirection.rtl;
    return Text(
      '${_accountIdentityLabel(status, strings)}$organization',
      textDirection: latin ? TextDirection.ltr : null,
      textAlign: latin && rtl ? TextAlign.right : null,
      style: theme.textTheme.bodyMedium?.copyWith(
        color: theme.colorScheme.onSurfaceVariant,
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
  return strings.get('warp_free');
}
