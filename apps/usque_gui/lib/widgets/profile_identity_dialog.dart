import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:url_launcher/url_launcher.dart';

import '../core/app_strings.dart';
import '../core/usque_motion.dart';
import '../models/app_models.dart';
import '../services/engine_client.dart';
import '../services/zero_trust_callback.dart';
import '../state/app_controller.dart';
import 'common.dart';
import 'usque_dialog.dart';

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

class _ProfileIdentityDialogState extends State<_ProfileIdentityDialog>
    with WidgetsBindingObserver {
  late final TextEditingController _nameController;
  late final TextEditingController _licenseController;
  late final TextEditingController _teamController;
  late final TextEditingController _callbackController;
  late final FocusNode _nameFocusNode;
  late final FocusNode _licenseFocusNode;
  late final FocusNode _teamFocusNode;
  late final FocusNode _callbackFocusNode;
  late int _step;
  IdentityProvisioningMethod _method = IdentityProvisioningMethod.register;
  String? _nameError;
  String? _licenseError;
  String? _teamError;
  String? _callbackError;
  String? _operationError;
  bool _showLicense = false;
  bool _submitting = false;
  bool _startingLogin = false;
  bool _callbackReceived = false;
  int _seenZeroTrustTicket = 0;

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
    WidgetsBinding.instance.addObserver(this);
    _step = _isRepair ? 1 : 0;
    if (_isZeroTrustRepair) {
      _method = IdentityProvisioningMethod.zeroTrust;
    }
    _nameController = TextEditingController(text: widget.profile?.name ?? '');
    _licenseController = TextEditingController();
    _teamController = TextEditingController(
      text: _isZeroTrustRepair ? _existingStatus?.organization ?? '' : '',
    );
    _callbackController = TextEditingController();
    _nameFocusNode = FocusNode();
    _licenseFocusNode = FocusNode();
    _teamFocusNode = FocusNode();
    _callbackFocusNode = FocusNode();
    _seenZeroTrustTicket = widget.controller.zeroTrustCallbackTicket;
    widget.controller.addListener(_onControllerChanged);
    if (_isZeroTrustRepair) {
      unawaited(_consumeAutomaticCallback());
    }
  }

  @override
  void dispose() {
    widget.controller.removeListener(_onControllerChanged);
    WidgetsBinding.instance.removeObserver(this);
    unawaited(widget.controller.cancelZeroTrustLogin());
    _licenseController.clear();
    _callbackController.clear();
    _licenseFocusNode.dispose();
    _teamFocusNode.dispose();
    _callbackFocusNode.dispose();
    _nameFocusNode.dispose();
    _licenseController.dispose();
    _teamController.dispose();
    _callbackController.dispose();
    _nameController.dispose();
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed &&
        _method == IdentityProvisioningMethod.zeroTrust) {
      unawaited(_consumeAutomaticCallback());
    }
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

  String? _normalizedTeam() =>
      ZeroTrustCallbackSession.tryNormalizeTeam(_teamController.text);

  void _onControllerChanged() {
    if (_method != IdentityProvisioningMethod.zeroTrust) return;
    final ticket = widget.controller.zeroTrustCallbackTicket;
    if (ticket == _seenZeroTrustTicket) return;
    _seenZeroTrustTicket = ticket;
    unawaited(_consumeAutomaticCallback());
  }

  String? _callbackValidationError({required bool submitting}) {
    final raw = _callbackController.text;
    if (raw.length > ZeroTrustCallbackSession.maxCallbackChars) {
      return _strings.get('zero_trust_callback_invalid');
    }
    final callback = raw.trim();
    if (callback.isEmpty) {
      return submitting ? _strings.get('zero_trust_callback_required') : null;
    }
    final team = _normalizedTeam();
    if (team == null ||
        !ZeroTrustCallbackSession.isValidCallback(team, callback)) {
      return _strings.get('zero_trust_callback_invalid');
    }
    return null;
  }

  Future<void> _fillCallbackFromClipboard() async {
    if (_startingLogin || _submitting) return;
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    var text = data?.text?.trim() ?? '';
    if (text.length >= 2 && text.startsWith('"') && text.endsWith('"')) {
      text = text.substring(1, text.length - 1).trim();
    }
    if (text.isEmpty) {
      setState(() {
        _callbackReceived = false;
        _callbackError = _strings.get('zero_trust_clipboard_empty');
      });
      return;
    }
    _callbackController.text = text;
    setState(() {
      _callbackReceived = false;
      _callbackError = _callbackValidationError(submitting: false);
    });
  }

  Future<void> _beginZeroTrustLogin() async {
    if (_startingLogin || _submitting) return;
    final team = _normalizedTeam();
    if (team == null) {
      setState(() => _teamError = _strings.get('zero_trust_team_invalid'));
      _teamFocusNode.requestFocus();
      return;
    }
    _teamController.text = team;
    _callbackController.clear();
    setState(() {
      _startingLogin = true;
      _teamError = null;
      _callbackError = null;
      _callbackReceived = false;
      _operationError = null;
    });
    try {
      final loginUrl = await widget.controller.beginZeroTrustLogin(team);
      final opened = await launchUrl(
        Uri.parse(loginUrl),
        mode: LaunchMode.externalApplication,
      );
      if (!opened) {
        throw StateError('The system browser could not be opened.');
      }
    } on Object catch (error) {
      await widget.controller.cancelZeroTrustLogin();
      if (!mounted) return;
      setState(() {
        _operationError = error is EngineException
            ? error.message
            : _strings.get('zero_trust_browser_failed');
      });
    } finally {
      if (mounted) setState(() => _startingLogin = false);
    }
  }

  Future<void> _consumeAutomaticCallback() async {
    final team = _normalizedTeam();
    if (team == null) return;
    final callback = await widget.controller.consumeZeroTrustCallback();
    if (!mounted || callback == null || callback.isEmpty) return;
    if (!ZeroTrustCallbackSession.isValidCallback(team, callback)) {
      return;
    }
    _callbackController.text = callback;
    setState(() {
      _callbackReceived = true;
      _callbackError = null;
      _operationError = null;
    });
  }

  Future<void> _cancelDialog() async {
    _licenseController.clear();
    _callbackController.clear();
    await widget.controller.cancelZeroTrustLogin();
    if (mounted) Navigator.of(context).pop(false);
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
      teamName = _normalizedTeam();
      final rawCallback = _callbackController.text;
      callbackUri = rawCallback.trim();
      if (teamName == null) {
        setState(() => _teamError = _strings.get('zero_trust_team_invalid'));
        _teamFocusNode.requestFocus();
        return;
      }
      final callbackError = _callbackValidationError(submitting: true);
      if (callbackError != null) {
        callbackUri = null;
        setState(() => _callbackError = callbackError);
        _callbackFocusNode.requestFocus();
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
    _callbackController.clear();
    _callbackReceived = false;
    await widget.controller.cancelZeroTrustLogin();
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
                  _ExperimentalBadge(strings: _strings),
                ],
              ),
            ),
          )
        else
          RadioGroup<IdentityProvisioningMethod>(
            groupValue: _method,
            onChanged: (value) {
              if (_submitting || value == null) return;
              _licenseController.clear();
              _callbackController.clear();
              unawaited(widget.controller.cancelZeroTrustLogin());
              setState(() {
                _method = value;
                _licenseError = null;
                _teamError = null;
                _callbackError = null;
                _callbackReceived = false;
                _operationError = null;
              });
              if (value == IdentityProvisioningMethod.zeroTrust) {
                unawaited(_consumeAutomaticCallback());
              }
            },
            child: Column(
              children: <Widget>[
                RadioListTile<IdentityProvisioningMethod>(
                  value: IdentityProvisioningMethod.register,
                  contentPadding: MediaQuery.sizeOf(context).width < 600
                      ? EdgeInsets.zero
                      : null,
                  title: Text(_strings.get('register_new')),
                  secondary: MediaQuery.sizeOf(context).width >= 600
                      ? const Icon(LucideIcons.userPlus)
                      : null,
                ),
                RadioListTile<IdentityProvisioningMethod>(
                  value: IdentityProvisioningMethod.registerWithLicense,
                  contentPadding: MediaQuery.sizeOf(context).width < 600
                      ? EdgeInsets.zero
                      : null,
                  title: Text(_strings.get('use_license_key')),
                  secondary: MediaQuery.sizeOf(context).width >= 600
                      ? const Icon(LucideIcons.badgePlus)
                      : null,
                ),
                if (!_isRepair)
                  RadioListTile<IdentityProvisioningMethod>(
                    value: IdentityProvisioningMethod.zeroTrust,
                    contentPadding: MediaQuery.sizeOf(context).width < 600
                        ? EdgeInsets.zero
                        : null,
                    title: Text(_strings.get('zero_trust_title')),
                    subtitle: Padding(
                      padding: const EdgeInsets.only(top: 4),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: <Widget>[
                          Text(_strings.get('zero_trust_subtitle')),
                          const SizedBox(height: 6),
                          _ExperimentalBadge(strings: _strings),
                        ],
                      ),
                    ),
                    secondary: MediaQuery.sizeOf(context).width >= 600
                        ? const Icon(LucideIcons.building2)
                        : null,
                  ),
              ],
            ),
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
          DialogGroup(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                TextField(
                  controller: _teamController,
                  focusNode: _teamFocusNode,
                  readOnly: _isZeroTrustRepair,
                  autocorrect: false,
                  enableSuggestions: false,
                  decoration: InputDecoration(
                    labelText: _strings.get('zero_trust_team'),
                    hintText: 'example-team',
                    errorText: _teamError,
                    prefixIcon: const Icon(LucideIcons.building2),
                  ),
                  onChanged: (_) {
                    setState(() {
                      _teamError = null;
                      _callbackError = _callbackValidationError(
                        submitting: false,
                      );
                    });
                    if (_callbackController.text.isEmpty) {
                      unawaited(_consumeAutomaticCallback());
                    }
                  },
                ),
                const SizedBox(height: 10),
                FilledButton.tonalIcon(
                  onPressed: _startingLogin || _submitting
                      ? null
                      : _beginZeroTrustLogin,
                  icon: _startingLogin
                      ? const SizedBox.square(
                          dimension: 16,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(LucideIcons.externalLink),
                  label: Text(_strings.get('zero_trust_open_login')),
                ),
                const SizedBox(height: 12),
                Text(
                  _callbackReceived
                      ? _strings.get('zero_trust_callback_received')
                      : _strings.get('zero_trust_manual_callback'),
                  style: Theme.of(context).textTheme.bodySmall,
                ),
                const SizedBox(height: 6),
                TextField(
                  controller: _callbackController,
                  focusNode: _callbackFocusNode,
                  obscureText: true,
                  enableSuggestions: false,
                  autocorrect: false,
                  decoration: InputDecoration(
                    labelText: _strings.get('zero_trust_callback'),
                    errorText: _callbackError,
                    prefixIcon: const Icon(LucideIcons.link),
                  ),
                  onChanged: (_) {
                    setState(() {
                      _callbackReceived = false;
                      _callbackError = _callbackValidationError(
                        submitting: false,
                      );
                    });
                  },
                  onSubmitted: (_) => _submit(),
                ),
                const SizedBox(height: 8),
                Align(
                  alignment: Alignment.centerRight,
                  child: TextButton.icon(
                    onPressed: _startingLogin || _submitting
                        ? null
                        : _fillCallbackFromClipboard,
                    icon: const Icon(LucideIcons.clipboardPaste),
                    label: Text(_strings.get('zero_trust_paste_clipboard')),
                  ),
                ),
                const SizedBox(height: 10),
                Text(
                  _strings.get('zero_trust_scope_note'),
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: Theme.of(context).colorScheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
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

class _ExperimentalBadge extends StatelessWidget {
  const _ExperimentalBadge({required this.strings});

  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    return StatusPill(
      label: strings.get('experimental'),
      tone: StatusTone.warning,
      dim: true,
      showIndicator: false,
    );
  }
}
