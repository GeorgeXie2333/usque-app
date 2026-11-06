import 'dart:async';

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/chain_strings.dart';
import '../models/app_models.dart';
import '../models/scenarios.dart';
import '../state/app_controller.dart';
import '../state/scenario_gate_leases.dart';
import '../widgets/common.dart';
import '../widgets/profiles_status.dart'
    show profilesSelectedAccountLabel, scenarioErrorKey;
import '../widgets/save_changes_bar.dart';
import '../widgets/scenario_endpoint_editor.dart';
import '../widgets/tv_text_input.dart';
import '../widgets/unsaved_changes_guard.dart';
import 'chain_proxy_screen.dart';
import 'profiles_hub_screen.dart'
    show previewAndUseScenario, scenarioExitNames, scenarioValueLabel;
import 'profiles_screen.dart' show ScenarioAccountPicker;

class ScenarioEditorScreen extends StatefulWidget {
  const ScenarioEditorScreen({
    required this.controller,
    this.scene,
    this.duplicate = false,
    super.key,
  });
  final AppController controller;
  final ConnectionScenario? scene;
  final bool duplicate;
  @override
  State<ScenarioEditorScreen> createState() => _ScenarioEditorScreenState();
}

class _ScenarioAccountChoice {
  const _ScenarioAccountChoice(this.id);
  final String? id;
}

class _ScenarioEditorScreenState extends State<ScenarioEditorScreen> {
  AppController get app => widget.controller;
  final _form = GlobalKey<FormState>();
  var _endpointKey = GlobalKey<ScenarioEndpointEditorState>();
  late final TextEditingController _name;
  late UsqueProfile _draft;
  String? _binding, _endpointAccountId, _error;
  ConnectionScenario? _savedScene;
  bool _dirty = false, _saving = false;
  Map<String, String> _exitNames = {};
  ScenarioGateLease? _leasedGate;

  UsqueProfile get _accountBase {
    final account = app.profiles.where((p) => p.id == _binding).firstOrNull;
    if (account == null) return app.activeProfile;
    final managed =
        app.identityStatus(account.id).provider == IdentityProvider.zeroTrust;
    return app.sharedNetwork.copyWith(
      id: account.id,
      name: account.name,
      endpointSelection: managed
          ? EndpointSelection.custom
          : app.sharedNetwork.endpointSelection,
      endpointIpv4: managed
          ? account.endpointIpv4
          : app.sharedNetwork.endpointIpv4,
      endpointIpv6: managed
          ? account.endpointIpv6
          : app.sharedNetwork.endpointIpv6,
    );
  }

  @override
  void initState() {
    super.initState();
    _savedScene = widget.duplicate ? null : widget.scene;
    final settings =
        widget.scene?.settings ?? app.scenarios.catalog!.currentSettings!;
    final binding = widget.scene?.boundAccountId;
    _binding = widget.duplicate && !app.profiles.any((a) => a.id == binding)
        ? null
        : binding;
    _draft = settings.toProfile(_accountBase);
    _endpointAccountId = _accountBase.id;
    _name = TextEditingController(text: widget.scene?.name ?? '');
    _dirty = _savedScene == null;
    unawaited(_refreshExitNames());
  }

  @override
  void dispose() {
    _name.dispose();
    final gate = _leasedGate;
    _leasedGate = null;
    unawaited(_releaseGate(gate));
    super.dispose();
  }

  Future<void> _releaseGate(ScenarioGateLease? gate) =>
      ScenarioGateLeases.of(app).release(gate);

  void _edit(UsqueProfile value) {
    setState(() {
      _draft = value;
      _dirty = true;
      _error = null;
    });
  }

  ScenarioSettings _settings() => ScenarioSettings.fromProfile(_draft);

  Future<void> _refreshExitNames() async {
    final names = await scenarioExitNames(app, settings: [_settings().values]);
    if (mounted) setState(() => _exitNames = names);
  }

  Future<void> _chooseAccount() async {
    final choice = await Navigator.of(context).push<_ScenarioAccountChoice>(
      MaterialPageRoute(
        builder: (context) => ScenarioAccountPicker(
          controller: app,
          boundAccountId: _binding,
          onSelected: (id) =>
              Navigator.of(context).pop(_ScenarioAccountChoice(id)),
        ),
      ),
    );
    // Picking the current option changes nothing.
    if (!mounted || choice == null || choice.id == _binding) return;
    setState(() {
      _binding = choice.id;
      _draft = _settings().toProfile(_accountBase);
      _endpointAccountId = _accountBase.id;
      _endpointKey = GlobalKey<ScenarioEndpointEditorState>();
      _dirty = true;
    });
  }

  void _toggleChain(bool enabled) {
    final chain =
        _draft.chainExit ??
        ChainExitSettings(
          source: _draft.vpnGate.hasSelection
              ? ChainSource.vpnGate
              : ChainSource.openvpnCustom,
        );
    _edit(
      _draft.copyWith(
        chainExit: chain.copyWith(enabled: enabled),
        vpnGate: _draft.vpnGate.copyWith(
          enabled: enabled && chain.source == ChainSource.vpnGate,
        ),
      ),
    );
  }

  Future<void> _chooseChain() async {
    ScenarioGateLease? transferredGate;
    final choice = await Navigator.of(context).push<UsqueProfile>(
      MaterialPageRoute(
        builder: (context) => ChainProxyScreen(
          controller: app,
          draftProfile: _draft,
          preparedGateDraft: _leasedGate,
          onDraftGateLease: (gate) => transferredGate = gate,
          onDraftSelected: (value) => Navigator.of(context).pop(value),
        ),
      ),
    );
    if (choice == null) return;
    if (!mounted) {
      await _releaseGate(transferredGate);
      return;
    }
    final gate = transferredGate;
    final previous = _leasedGate;
    if (previous?.sourceEpoch != gate?.sourceEpoch ||
        previous?.selection.serverId != gate?.selection.serverId ||
        previous?.selection.configSha256 != gate?.selection.configSha256) {
      unawaited(_releaseGate(previous));
    }
    _leasedGate = gate;
    _edit(
      _draft.copyWith(
        chainExit: choice.chainExit,
        clearChainExit: choice.chainExit == null,
        vpnGate: choice.vpnGate,
      ),
    );
    unawaited(_refreshExitNames());
  }

  Future<void> _save({bool use = false}) async {
    if (_saving) return;
    if (!await (_endpointKey.currentState?.prepareSave() ??
            Future.value(true)) ||
        !mounted) {
      return;
    }
    if (_saving || !(_form.currentState?.validate() ?? false)) return;
    setState(() {
      _saving = true;
      _error = null;
    });
    final gate = _draft.vpnGate;
    if (gate.hasSelection) {
      final lease = await ScenarioGateLeases.of(app).ensure(
        gate,
        existing: _leasedGate,
        isCurrent: () => mounted && _saving && _draft.vpnGate == gate,
      );
      if (!mounted) {
        unawaited(_releaseGate(lease));
        return;
      }
      if (lease == null) {
        setState(() {
          _saving = false;
          _error = app.strings.get('gate_prepare_error');
        });
        return;
      }
      final previous = _leasedGate;
      _leasedGate = lease;
      if (!identical(previous, lease)) unawaited(_releaseGate(previous));
    }
    final result = await app.scenarios.save(
      previous: _savedScene,
      name: _name.text.trim(),
      boundAccountId: _binding,
      settings: _settings(),
    );
    if (result != null) {
      final gate = _leasedGate;
      _leasedGate = null;
      await _releaseGate(gate);
    }
    if (!mounted) return;
    setState(() {
      _saving = false;
      if (result == null) {
        _error = app.strings.get(scenarioErrorKey(app.scenarios.errorCode));
        return;
      }
      _savedScene = result;
      _binding = result.boundAccountId;
      _draft = result.settings.toProfile(_accountBase);
      _endpointAccountId = _accountBase.id;
      _name.text = result.name;
      _endpointKey = GlobalKey<ScenarioEndpointEditorState>();
      _dirty = false;
    });
    if (result != null && use && mounted) {
      await previewAndUseScenario(context, app, result);
    }
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: app,
    builder: (context, _) => _buildEditor(context),
  );

  Widget _buildEditor(BuildContext context) {
    final s = app.strings;
    final account = _accountBase;
    if (_endpointAccountId != account.id) {
      _draft = _settings().toProfile(account);
      _endpointAccountId = account.id;
      _endpointKey = GlobalKey<ScenarioEndpointEditorState>();
    }
    final bound = app.profiles.where((p) => p.id == _binding).firstOrNull;
    return UnsavedChangesGuard(
      strings: s,
      dirty: _dirty,
      saving: _saving,
      discardTitle: s.get('scenario_discard_title'),
      discardBody: s.get('scenario_discard_body'),
      child: Column(
        children: [
          Expanded(
            child: PageFrame(
              title: _savedScene?.name ?? s.get('scenario_create'),
              // The scope of the whole template, not of any one field.
              subtitle: s.get('scenario_global_hint'),
              contentWidth: 880,
              header: Align(
                alignment: AlignmentDirectional.centerStart,
                child: TextButton.icon(
                  onPressed: _saving ? null : () => Navigator.maybePop(context),
                  icon: const Icon(LucideIcons.arrowLeftDir),
                  label: Text(s.get('back')),
                ),
              ),
              child: Form(
                key: _form,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    tvTextFormField(
                      key: const ValueKey('scenario-name'),
                      controller: _name,
                      enabled: !_saving,
                      decoration: InputDecoration(
                        labelText: s.get('scenario_name'),
                      ),
                      maxLength: 64,
                      validator: (value) =>
                          value == null ||
                              value.trim().isEmpty ||
                              value.trim().runes.length > 64
                          ? s.get('scenario_validation_name')
                          : null,
                      onChanged: (_) => setState(() => _dirty = true),
                    ),
                    const SizedBox(height: 16),
                    LinkRow(
                      key: const ValueKey('scenario-account-management'),
                      icon: LucideIcons.user,
                      title: s.get('scenario_binding'),
                      // Following names the account it would use now.
                      value: _binding == null
                          ? s
                                .get('scenario_follow_account_named')
                                .replaceAll(
                                  '{account}',
                                  profilesSelectedAccountLabel(app),
                                )
                          : bound == null
                          ? s.get('scenario_reference_missing')
                          : s.accountName(bound),
                      onTap: _saving ? null : _chooseAccount,
                    ),
                    const SizedBox(height: 20),
                    // The switch shares the link rows' icon and text columns.
                    RowTileTheme(
                      child: Padding(
                        padding: RowInsets.tile,
                        child: SwitchListTile.adaptive(
                          key: const ValueKey('scenario-chain-toggle'),
                          contentPadding: EdgeInsets.zero,
                          secondary: const Icon(LucideIcons.link),
                          title: Text(s.chain('enable')),
                          value: _draft.chainEnabled,
                          onChanged: _saving ? null : _toggleChain,
                        ),
                      ),
                    ),
                    if (_draft.chainEnabled) ...[
                      LinkRow(
                        key: const ValueKey('scenario-chain-settings'),
                        icon: LucideIcons.route,
                        title: s.chain('title'),
                        value: scenarioValueLabel(
                          s,
                          _settings().values,
                          _draft.chainSource == ChainSource.vpnGate
                              ? 'vpn_gate'
                              : 'chain_exit',
                          exitNames: _exitNames,
                        ),
                        onTap: _saving ? null : _chooseChain,
                      ),
                      Padding(
                        padding: const EdgeInsetsDirectional.only(
                          start: RowInsets.horizontal + RowInsets.textIndent,
                          top: 4,
                        ),
                        child: HintText(s.get('scenario_library_hint')),
                      ),
                    ],
                    const SizedBox(height: 28),
                    ScenarioEndpointEditor(
                      key: _endpointKey,
                      value: _draft,
                      account: account,
                      identity: app.identityStatus(account.id),
                      capabilities: app.engineCapabilities,
                      strings: s,
                      enabled: !_saving,
                      onChanged: _edit,
                    ),
                    const SizedBox(height: 28),
                    ContentHeading(title: s.get('scenario_connection')),
                    const SizedBox(height: 16),
                    DropdownButtonFormField<TransportPolicy>(
                      key: ValueKey(_draft.transport),
                      initialValue: _draft.transport,
                      isExpanded: true,
                      decoration: FieldDropdown.decoration(
                        context,
                        labelText: s.get('transport'),
                      ),
                      items: [
                        for (final policy in TransportPolicy.values)
                          DropdownMenuItem(
                            value: policy,
                            child: Text(s.get(policy.name)),
                          ),
                      ],
                      onChanged: _saving
                          ? null
                          : (value) {
                              if (value != null) {
                                _edit(_draft.copyWith(transport: value));
                              }
                            },
                    ),
                  ],
                ),
              ),
            ),
          ),
          if (_dirty || _saving || _error != null)
            SaveChangesBar(
              strings: s,
              dirty: _dirty,
              saving: _saving,
              error: _error,
              onSave: () => _save(),
              saveLabel: s.get('scenario_save'),
              savingLabel: s.get('scenario_saving'),
              statusLabel: _dirty && !_saving && _error == null
                  ? s.get('scenario_unsaved')
                  : null,
              secondaryAction: OutlinedButton(
                key: const ValueKey('scenario-save-use'),
                onPressed: _saving ? null : () => _save(use: true),
                child: Text(s.get('scenario_save_use')),
              ),
              contentWidth: 880,
              matchPageGutter: true,
            ),
        ],
      ),
    );
  }
}
