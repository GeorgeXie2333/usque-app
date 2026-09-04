import 'dart:async';

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_motion.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import 'common.dart';
import 'usque_dialog.dart';
import 'zero_trust_enrollment_editor.dart';

Future<bool> showProfileIdentityDialog(
  BuildContext context, {
  required AppController controller,
  UsqueProfile? profile,
}) async {
  return await showDialog<bool>(
        context: context,
        barrierDismissible: false,
        builder: (context) =>
            _ProfileIdentityDialog(controller: controller, profile: profile),
      ) ??
      false;
}

class _ProfileIdentityDialog extends StatefulWidget {
  const _ProfileIdentityDialog({required this.controller, this.profile});

  final AppController controller;
  final UsqueProfile? profile;

  @override
  State<_ProfileIdentityDialog> createState() => _ProfileIdentityDialogState();
}

class _ProfileIdentityDialogState extends State<_ProfileIdentityDialog> {
  late final TextEditingController _nameController;
  late final TextEditingController _licenseController;
  late final FocusNode _nameFocusNode;
  late final FocusNode _licenseFocusNode;
  final _zeroTrustKey = GlobalKey<ZeroTrustEnrollmentEditorState>();
  late int _step;
  IdentityProvisioningMethod _method = IdentityProvisioningMethod.register;
  String? _nameError;
  String? _licenseError;
  String? _operationError;
  bool _showLicense = false;
  bool _submitting = false;

  AppStrings get _strings => widget.controller.strings;
  bool get _isRepair => widget.profile != null;
  ProfileIdentityStatus? get _existingStatus => widget.profile == null
      ? null
      : widget.controller.identityStatus(widget.profile!.id);
  bool get _isZeroTrustRepair =>
      _existingStatus?.provider == IdentityProvider.zeroTrust;
  bool get _zeroTrustRepairUnavailable =>
      _isZeroTrustRepair &&
      (_existingStatus?.organization.trim().isEmpty ?? true);

  @override
  void initState() {
    super.initState();
    _step = _isRepair ? 1 : 0;
    if (_isZeroTrustRepair) {
      _method = IdentityProvisioningMethod.zeroTrust;
    }
    _nameController = TextEditingController(text: widget.profile?.name ?? '');
    _licenseController = TextEditingController();
    _nameFocusNode = FocusNode();
    _licenseFocusNode = FocusNode();
  }

  @override
  void dispose() {
    unawaited(widget.controller.cancelZeroTrustLogin());
    _licenseController.clear();
    _licenseFocusNode.dispose();
    _nameFocusNode.dispose();
    _licenseController.dispose();
    _nameController.dispose();
    super.dispose();
  }

  void _continueFromName() {
    final name = _nameController.text.trim();
    if (name.isEmpty) {
      setState(() => _nameError = _strings.get('required'));
      return;
    }
    if (name.runes.length > 64) {
      setState(() => _nameError = _strings.get('profile_name_too_long'));
      return;
    }
    setState(() {
      _nameError = null;
      _step = 1;
    });
  }

  Future<void> _cancelDialog() async {
    _licenseController.clear();
    final editor = _zeroTrustKey.currentState;
    if (editor != null) {
      await editor.clearSensitive();
    } else {
      await widget.controller.cancelZeroTrustLogin();
    }
    if (mounted) Navigator.of(context).pop(false);
  }

  void _changeMethod(IdentityProvisioningMethod value) {
    if (_submitting || value == _method) return;
    _licenseController.clear();
    if (_method == IdentityProvisioningMethod.zeroTrust) {
      final editor = _zeroTrustKey.currentState;
      if (editor != null) {
        unawaited(editor.clearSensitive());
      } else {
        unawaited(widget.controller.cancelZeroTrustLogin());
      }
    }
    setState(() {
      _method = value;
      _licenseError = null;
      _operationError = null;
    });
  }

  Future<void> _submit() async {
    if (_submitting || _zeroTrustRepairUnavailable) return;
    final name = _nameController.text.trim();
    if (!_isRepair && name.isEmpty) {
      setState(() {
        _step = 0;
        _nameError = _strings.get('required');
      });
      return;
    }

    String? licenseKey;
    String? teamName;
    String? callbackUri;
    if (_method == IdentityProvisioningMethod.registerWithLicense) {
      licenseKey = _licenseController.text.trim();
      _licenseController.clear();
      if (licenseKey.isEmpty) {
        licenseKey = null;
        setState(() => _licenseError = _strings.get('required'));
        _licenseFocusNode.requestFocus();
        return;
      }
    }
    if (_method == IdentityProvisioningMethod.zeroTrust) {
      final enrollment = _zeroTrustKey.currentState?.validateAndRead();
      if (enrollment == null) return;
      teamName = enrollment.teamName;
      callbackUri = enrollment.callbackUri;
    }

    setState(() {
      _submitting = true;
      _licenseError = null;
      _operationError = null;
    });
    final success = _isRepair
        ? await widget.controller.provisionProfileIdentity(
            widget.profile!,
            method: _method,
            licenseKey: licenseKey,
            teamName: teamName,
            callbackUri: callbackUri,
          )
        : await widget.controller.createProfileWithIdentity(
            name,
            method: _method,
            licenseKey: licenseKey,
            teamName: teamName,
            callbackUri: callbackUri,
          );
    licenseKey = null;
    callbackUri = null;
    final editor = _zeroTrustKey.currentState;
    if (editor != null) {
      await editor.clearSensitive();
    } else {
      await widget.controller.cancelZeroTrustLogin();
    }
    if (!mounted) return;
    if (success) {
      Navigator.of(context).pop(true);
      return;
    }
    setState(() {
      _submitting = false;
      _operationError =
          widget.controller.lastError ?? _strings.get('identity_setup_failed');
    });
  }

  @override
  Widget build(BuildContext context) {
    final title = _isRepair
        ? _strings.get('configure_identity')
        : _strings.get('new_profile');
    return UsqueDialog(
      icon: _step == 0
          ? LucideIcons.layers3
          : _method == IdentityProvisioningMethod.zeroTrust
          ? LucideIcons.building2
          : LucideIcons.keyRound,
      title: title,
      subtitle: _strings.get(
        _step == 0 ? 'profile_name' : 'identity_and_license',
      ),
      content: AnimatedSize(
        duration: UsqueMotion.of(context, UsqueMotion.base),
        curve: UsqueMotion.emphasized,
        alignment: Alignment.topCenter,
        child: FadeThroughSwitcher(
          duration: UsqueMotion.base,
          child: _step == 0
              ? _buildNameStep(key: const ValueKey<String>('name'))
              : _buildIdentityStep(key: const ValueKey<String>('identity')),
        ),
      ),
      actions: <Widget>[
        TextButton(
          onPressed: _submitting ? null : _cancelDialog,
          child: Text(_strings.get('cancel')),
        ),
        if (_step == 1 && !_isRepair)
          TextButton(
            onPressed: _submitting ? null : () => setState(() => _step = 0),
            child: Text(_strings.get('back')),
          ),
        FilledButton(
          onPressed: _submitting || (_step == 1 && _zeroTrustRepairUnavailable)
              ? null
              : _step == 0
              ? _continueFromName
              : _submit,
          child: _submitting
              ? const SizedBox.square(
                  dimension: 18,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              : Text(
                  _strings.get(
                    _step == 0
                        ? 'continue'
                        : _isRepair
                        ? 'save'
                        : 'create',
                  ),
                ),
        ),
      ],
    );
  }

  Widget _buildNameStep({required Key key}) {
    return Column(
      key: key,
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        TextField(
          controller: _nameController,
          focusNode: _nameFocusNode,
          autofocus: true,
          maxLength: 64,
          textInputAction: TextInputAction.next,
          decoration: InputDecoration(
            labelText: _strings.get('profile_name'),
            errorText: _nameError,
          ),
          onChanged: (_) {
            if (_nameError != null) setState(() => _nameError = null);
          },
          onSubmitted: (_) => _continueFromName(),
        ),
      ],
    );
  }

  Widget _buildIdentityStep({required Key key}) {
    return Column(
      key: key,
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        if (_isZeroTrustRepair)
          ListTile(
            contentPadding: const EdgeInsets.symmetric(horizontal: 12),
            leading: const Icon(LucideIcons.building2),
            title: Text(_strings.get('zero_trust_title')),
            subtitle: Padding(
              padding: const EdgeInsets.only(top: 4),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  Text(_strings.get('zero_trust_repair_same_team')),
                  const SizedBox(height: 6),
                  ZeroTrustExperimentalBadge(strings: _strings),
                ],
              ),
            ),
          )
        else
          IdentityProvisioningMethodSelector(
            strings: _strings,
            value: _method,
            enabled: !_submitting,
            showZeroTrust: !_isRepair,
            onChanged: _changeMethod,
          ),
        if (_zeroTrustRepairUnavailable)
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 4, 12, 8),
            child: Text(
              _strings.get('zero_trust_metadata_missing'),
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                color: Theme.of(context).colorScheme.error,
              ),
            ),
          ),
        if (_method ==
            IdentityProvisioningMethod.registerWithLicense) ...<Widget>[
          const SizedBox(height: 10),
          TextField(
            controller: _licenseController,
            focusNode: _licenseFocusNode,
            autofocus: true,
            obscureText: !_showLicense,
            enableSuggestions: false,
            autocorrect: false,
            textInputAction: TextInputAction.done,
            decoration: InputDecoration(
              labelText: _strings.get('warp_license_key'),
              errorText: _licenseError,
              suffixIcon: IconButton(
                tooltip: _strings.get(
                  _showLicense ? 'hide_license' : 'show_license',
                ),
                onPressed: () => setState(() => _showLicense = !_showLicense),
                icon: Icon(_showLicense ? LucideIcons.eyeOff : LucideIcons.eye),
              ),
            ),
            onChanged: (_) {
              if (_licenseError != null) setState(() => _licenseError = null);
            },
            onSubmitted: (_) => _submit(),
          ),
        ],
        if (_method == IdentityProvisioningMethod.zeroTrust &&
            !_zeroTrustRepairUnavailable) ...<Widget>[
          const SizedBox(height: 12),
          ZeroTrustEnrollmentEditor(
            key: _zeroTrustKey,
            controller: widget.controller,
            enabled: !_submitting,
            initialTeam: _isZeroTrustRepair
                ? _existingStatus?.organization ?? ''
                : '',
            teamReadOnly: _isZeroTrustRepair,
            onSubmitted: _submit,
          ),
        ],
        BannerSlot(
          spacing: 0,
          child: _operationError == null
              ? null
              : Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: WarningBanner(
                    title: _strings.get('identity_setup_failed'),
                    message: _operationError!,
                    danger: true,
                  ),
                ),
        ),
      ],
    );
  }
}
