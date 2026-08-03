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
                  strings: strings,
                  onActivate: () => controller.setActiveProfile(profile.id),
                  onConfigureIdentity: () => showProfileIdentityDialog(
                    context,
                    controller: controller,
                    profile: profile,
                  ),
                  onEdit: () => _renameProfile(context, profile, strings),
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

  Future<void> _renameProfile(
    BuildContext context,
    UsqueProfile profile,
    AppStrings strings,
  ) async {
    final name = await _profileNameDialog(
      context,
      strings: strings,
      title: strings.get('edit'),
      initialValue: profile.name,
    );
    if (name != null && name.trim().isNotEmpty) {
      controller.updateProfile(profile.copyWith(name: name.trim()));
    }
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

  Future<String?> _profileNameDialog(
    BuildContext context, {
    required AppStrings strings,
    required String title,
    String initialValue = '',
  }) {
    return showDialog<String>(
      context: context,
      builder: (context) => _ProfileNameDialog(
        strings: strings,
        title: title,
        initialValue: initialValue,
      ),
    );
  }
}

class _ProfileNameDialog extends StatefulWidget {
  const _ProfileNameDialog({
    required this.strings,
    required this.title,
    required this.initialValue,
  });

  final AppStrings strings;
  final String title;
  final String initialValue;

  @override
  State<_ProfileNameDialog> createState() => _ProfileNameDialogState();
}

class _ProfileNameDialogState extends State<_ProfileNameDialog> {
  late final TextEditingController _controller;
  late final FocusNode _focusNode;
  String? _errorText;
  bool _closing = false;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.initialValue);
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
    Navigator.of(context).pop(value);
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
      title: Text(widget.title),
      content: TextField(
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
          if (_errorText != null) {
            setState(() => _errorText = null);
          }
        },
        onSubmitted: (_) => _submit(),
      ),
      actions: <Widget>[
        TextButton(
          onPressed: _closing ? null : () => Navigator.of(context).pop(),
          child: Text(widget.strings.get('cancel')),
        ),
        FilledButton(
          onPressed: _closing ? null : _submit,
          child: Text(
            widget.strings.get(widget.initialValue.isEmpty ? 'create' : 'save'),
          ),
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
    required this.strings,
    required this.onActivate,
    required this.onConfigureIdentity,
    required this.onEdit,
    required this.onDelete,
  });

  final UsqueProfile profile;
  final bool active;
  final ProfileIdentityState identityState;
  final AppStrings strings;
  final VoidCallback onActivate;
  final VoidCallback onConfigureIdentity;
  final VoidCallback onEdit;
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
              _ProfileTag(
                icon: LucideIcons.shield,
                label: _modeLabel(strings, profile.mode),
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

String _modeLabel(AppStrings strings, OperatingMode mode) {
  return strings.get(switch (mode) {
    OperatingMode.vpn => 'vpn_mode',
    OperatingMode.socks5 => 'socks_mode',
    OperatingMode.httpProxy => 'http_mode',
  });
}

String _transportLabel(AppStrings strings, TransportPolicy transport) {
  return strings.get(switch (transport) {
    TransportPolicy.automatic => 'automatic',
    TransportPolicy.http3 => 'http3',
    TransportPolicy.http2 => 'http2',
  });
}
