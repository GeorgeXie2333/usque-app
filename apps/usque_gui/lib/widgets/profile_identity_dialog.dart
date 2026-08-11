import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';

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
  late int _step;
  IdentityProvisioningMethod _method = IdentityProvisioningMethod.register;
  String? _nameError;
  String? _licenseError;
  String? _operationError;
  bool _showLicense = false;
  bool _submitting = false;

  AppStrings get _strings => widget.controller.strings;
  bool get _isRepair => widget.profile != null;

  @override
  void initState() {
    super.initState();
    _step = _isRepair ? 1 : 0;
    _nameController = TextEditingController(text: widget.profile?.name ?? '');
    _licenseController = TextEditingController();
    _nameFocusNode = FocusNode();
    _licenseFocusNode = FocusNode();
  }

  @override
  void dispose() {
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

  Future<void> _submit() async {
    if (_submitting) return;
    final name = _nameController.text.trim();
    if (!_isRepair && name.isEmpty) {
      setState(() {
        _step = 0;
        _nameError = _strings.get('required');
      });
      return;
    }

    String? licenseKey;
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
          )
        : await widget.controller.createProfileWithIdentity(
            name,
            method: _method,
            licenseKey: licenseKey,
          );
    licenseKey = null;
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
    return AlertDialog(
      icon: Icon(_step == 0 ? LucideIcons.layers3 : LucideIcons.keyRound),
      title: Text(title),
      content: SizedBox(
        width: 480,
        child: AnimatedSwitcher(
          duration: const Duration(milliseconds: 180),
          child: _step == 0
              ? _buildNameStep(key: const ValueKey<String>('name'))
              : _buildIdentityStep(key: const ValueKey<String>('identity')),
        ),
      ),
      actions: <Widget>[
        TextButton(
          onPressed: _submitting
              ? null
              : () {
                  _licenseController.clear();
                  Navigator.of(context).pop(false);
                },
          child: Text(_strings.get('cancel')),
        ),
        if (_step == 1 && !_isRepair)
          TextButton(
            onPressed: _submitting ? null : () => setState(() => _step = 0),
            child: Text(_strings.get('back')),
          ),
        FilledButton(
          onPressed: _submitting
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
        RadioGroup<IdentityProvisioningMethod>(
          groupValue: _method,
          onChanged: (value) {
            if (_submitting || value == null) return;
            _licenseController.clear();
            setState(() {
              _method = value;
              _licenseError = null;
              _operationError = null;
            });
          },
          child: Column(
            children: <Widget>[
              RadioListTile<IdentityProvisioningMethod>(
                value: IdentityProvisioningMethod.register,
                title: Text(_strings.get('register_new')),
                secondary: const Icon(LucideIcons.userPlus),
              ),
              RadioListTile<IdentityProvisioningMethod>(
                value: IdentityProvisioningMethod.registerWithLicense,
                title: Text(_strings.get('use_license_key')),
                secondary: const Icon(LucideIcons.badgePlus),
              ),
            ],
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
        if (_operationError != null) ...<Widget>[
          const SizedBox(height: 12),
          Text(
            _operationError!,
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        ],
      ],
    );
  }
}
