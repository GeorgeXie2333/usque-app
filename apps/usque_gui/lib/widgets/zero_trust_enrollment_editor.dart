import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:url_launcher/url_launcher.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import '../services/engine_client.dart';
import '../services/zero_trust_callback.dart';
import '../state/app_controller.dart';
import 'common.dart';
import 'usque_dialog.dart';

class ZeroTrustEnrollmentDraft {
  const ZeroTrustEnrollmentDraft({
    required this.teamName,
    required this.callbackUri,
  });

  final String teamName;
  final String callbackUri;
}

class IdentityProvisioningMethodSelector extends StatelessWidget {
  const IdentityProvisioningMethodSelector({
    required this.strings,
    required this.value,
    required this.enabled,
    required this.onChanged,
    this.showZeroTrust = true,
    super.key,
  });

  final AppStrings strings;
  final IdentityProvisioningMethod value;
  final bool enabled;
  final ValueChanged<IdentityProvisioningMethod> onChanged;
  final bool showZeroTrust;

  @override
  Widget build(BuildContext context) {
    final wide = MediaQuery.sizeOf(context).width >= 600;
    return RadioGroup<IdentityProvisioningMethod>(
      groupValue: value,
      onChanged: (value) {
        if (enabled && value != null) onChanged(value);
      },
      child: Column(
        children: <Widget>[
          RadioListTile<IdentityProvisioningMethod>(
            value: IdentityProvisioningMethod.register,
            enabled: enabled,
            contentPadding: wide ? null : EdgeInsets.zero,
            title: Text(strings.get('register_new')),
            secondary: wide ? const Icon(LucideIcons.userPlus) : null,
          ),
          RadioListTile<IdentityProvisioningMethod>(
            value: IdentityProvisioningMethod.registerWithLicense,
            enabled: enabled,
            contentPadding: wide ? null : EdgeInsets.zero,
            title: Text(strings.get('use_license_key')),
            secondary: wide ? const Icon(LucideIcons.badgePlus) : null,
          ),
          if (showZeroTrust)
            RadioListTile<IdentityProvisioningMethod>(
              value: IdentityProvisioningMethod.zeroTrust,
              enabled: enabled,
              contentPadding: wide ? null : EdgeInsets.zero,
              title: Text(strings.get('zero_trust_title')),
              subtitle: Padding(
                padding: const EdgeInsets.only(top: 4),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: <Widget>[
                    Text(strings.get('zero_trust_subtitle')),
                    const SizedBox(height: 6),
                    ZeroTrustExperimentalBadge(strings: strings),
                  ],
                ),
              ),
              secondary: wide ? const Icon(LucideIcons.building2) : null,
            ),
        ],
      ),
    );
  }
}

class ZeroTrustEnrollmentEditor extends StatefulWidget {
  const ZeroTrustEnrollmentEditor({
    required this.controller,
    required this.enabled,
    this.initialTeam = '',
    this.teamReadOnly = false,
    this.onValidityChanged,
    this.onSubmitted,
    super.key,
  });

  final AppController controller;
  final bool enabled;
  final String initialTeam;
  final bool teamReadOnly;
  final ValueChanged<bool>? onValidityChanged;
  final VoidCallback? onSubmitted;

  @override
  State<ZeroTrustEnrollmentEditor> createState() =>
      ZeroTrustEnrollmentEditorState();
}

class ZeroTrustEnrollmentEditorState extends State<ZeroTrustEnrollmentEditor>
    with WidgetsBindingObserver {
  late final TextEditingController _teamController;
  late final TextEditingController _callbackController;
  late final FocusNode _teamFocusNode;
  late final FocusNode _callbackFocusNode;
  String? _teamError;
  String? _callbackError;
  String? _operationError;
  bool _startingLogin = false;
  bool _callbackReceived = false;
  bool? _reportedValidity;
  late int _seenZeroTrustTicket;

  AppStrings get _strings => widget.controller.strings;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _teamController = TextEditingController(text: widget.initialTeam);
    _callbackController = TextEditingController();
    _teamFocusNode = FocusNode()..addListener(_validateTeamAfterEditing);
    _callbackFocusNode = FocusNode();
    _seenZeroTrustTicket = widget.controller.zeroTrustCallbackTicket;
    widget.controller.addListener(_onControllerChanged);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      _emitValidity();
      if (_normalizedTeam() != null) {
        unawaited(_consumeAutomaticCallback());
      }
    });
  }

  @override
  void didUpdateWidget(covariant ZeroTrustEnrollmentEditor oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.removeListener(_onControllerChanged);
      widget.controller.addListener(_onControllerChanged);
      _seenZeroTrustTicket = widget.controller.zeroTrustCallbackTicket;
    }
    if (oldWidget.initialTeam != widget.initialTeam &&
        _teamController.text == oldWidget.initialTeam) {
      _teamController.text = widget.initialTeam;
      _emitValidity();
    }
  }

  @override
  void dispose() {
    widget.controller.removeListener(_onControllerChanged);
    WidgetsBinding.instance.removeObserver(this);
    unawaited(widget.controller.cancelZeroTrustLogin());
    _callbackController.clear();
    _teamFocusNode.dispose();
    _callbackFocusNode.dispose();
    _teamController.dispose();
    _callbackController.dispose();
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      unawaited(_consumeAutomaticCallback());
    }
  }

  String? _normalizedTeam() =>
      ZeroTrustCallbackSession.tryNormalizeTeam(_teamController.text);

  bool get _isValid {
    final team = _normalizedTeam();
    final callback = _callbackController.text.trim();
    return team != null &&
        callback.isNotEmpty &&
        callback.length <= ZeroTrustCallbackSession.maxCallbackChars &&
        ZeroTrustCallbackSession.isValidCallback(team, callback);
  }

  void _emitValidity() {
    final valid = _isValid;
    if (_reportedValidity == valid) return;
    _reportedValidity = valid;
    widget.onValidityChanged?.call(valid);
  }

  void _validateTeamAfterEditing() {
    if (_teamFocusNode.hasFocus || !mounted) return;
    final invalid =
        _teamController.text.trim().isNotEmpty && _normalizedTeam() == null;
    setState(() {
      _teamError = invalid ? _strings.get('zero_trust_team_invalid') : null;
    });
    _emitValidity();
  }

  void _onControllerChanged() {
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
    if (_startingLogin || !widget.enabled) return;
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
      _emitValidity();
      return;
    }
    _callbackController.text = text;
    setState(() {
      _callbackReceived = false;
      _callbackError = _callbackValidationError(submitting: false);
    });
    _emitValidity();
  }

  Future<void> _beginZeroTrustLogin() async {
    if (_startingLogin || !widget.enabled) return;
    final team = _normalizedTeam();
    if (team == null) {
      setState(() => _teamError = _strings.get('zero_trust_team_invalid'));
      _teamFocusNode.requestFocus();
      _emitValidity();
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
    _emitValidity();
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
    if (!ZeroTrustCallbackSession.isValidCallback(team, callback)) return;
    _callbackController.text = callback;
    setState(() {
      _callbackReceived = true;
      _callbackError = null;
      _operationError = null;
    });
    _emitValidity();
  }

  ZeroTrustEnrollmentDraft? validateAndRead() {
    final team = _normalizedTeam();
    if (team == null) {
      setState(() => _teamError = _strings.get('zero_trust_team_invalid'));
      _teamFocusNode.requestFocus();
      _emitValidity();
      return null;
    }
    final callbackError = _callbackValidationError(submitting: true);
    if (callbackError != null) {
      setState(() => _callbackError = callbackError);
      _callbackFocusNode.requestFocus();
      _emitValidity();
      return null;
    }
    _teamController.text = team;
    return ZeroTrustEnrollmentDraft(
      teamName: team,
      callbackUri: _callbackController.text.trim(),
    );
  }

  Future<void> clearSensitive() async {
    _callbackController.clear();
    if (mounted) {
      setState(() {
        _callbackReceived = false;
        _callbackError = null;
        _operationError = null;
      });
      _emitValidity();
    }
    await widget.controller.cancelZeroTrustLogin();
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        DialogGroup(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: <Widget>[
              TextField(
                controller: _teamController,
                focusNode: _teamFocusNode,
                enabled: widget.enabled,
                readOnly: widget.teamReadOnly,
                autocorrect: false,
                enableSuggestions: false,
                textInputAction: TextInputAction.next,
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
                  _emitValidity();
                  if (_callbackController.text.isEmpty) {
                    unawaited(_consumeAutomaticCallback());
                  }
                },
              ),
              const SizedBox(height: 10),
              FilledButton.tonalIcon(
                onPressed: _startingLogin || !widget.enabled
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
                enabled: widget.enabled,
                obscureText: true,
                enableSuggestions: false,
                autocorrect: false,
                textInputAction: TextInputAction.done,
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
                  _emitValidity();
                },
                onSubmitted: (_) => widget.onSubmitted?.call(),
              ),
              const SizedBox(height: 8),
              Align(
                alignment: AlignmentDirectional.centerEnd,
                child: TextButton.icon(
                  onPressed: _startingLogin || !widget.enabled
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

class ZeroTrustExperimentalBadge extends StatelessWidget {
  const ZeroTrustExperimentalBadge({required this.strings, super.key});

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
