import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/proton_strings.dart';
import '../models/proton_vpn_models.dart' show takeProtonSensitiveBytes;
import '../services/engine_client.dart' show EngineException;
import '../state/app_controller.dart';
import 'chain_editor_layout.dart';
import 'common.dart';
import 'country_flag.dart';
import 'usque_dialog.dart';

/// Native controls around a native-owned, cancellable authentication job.
class ProtonVpnPanel extends StatefulWidget {
  const ProtonVpnPanel({
    required this.controller,
    required this.onChanged,
    this.selectedProfileId,
    this.savedProfileId,
    this.currentProfileId,
    this.currentStatus,
    this.onSelectProfile,
    this.beforeRemove,
    this.selectionEnabled = true,
    this.selectedServerId,
    this.onSelectServer,
    super.key,
  });
  final AppController controller;
  final VoidCallback onChanged;
  final String? selectedProfileId, savedProfileId, currentProfileId;

  /// The running session's label and tone for the current account, so a
  /// failed connection never reads as connected.
  final (String, StatusTone)? currentStatus;
  final ValueChanged<String>? onSelectProfile;
  final Future<bool> Function(List<String>)? beforeRemove;
  final bool selectionEnabled;
  final String? selectedServerId;
  final void Function(String profileId, String serverId)? onSelectServer;

  @override
  State<ProtonVpnPanel> createState() => ProtonVpnPanelState();
}

class ProtonVpnPanelState extends State<ProtonVpnPanel> {
  Future<void> logout(String profileId) =>
      _command({'action': 'logout', 'profile_id': profileId});
  Map<Object?, Object?> _job = const {};
  List<Map<Object?, Object?>> _profiles = const [];
  Timer? _timer;
  bool _busy = false;
  bool _launchUnconfirmed = false;
  bool _verifying = false;
  bool _handoffLoading = false;
  bool _serversLoading = false;
  String? _serverChanged;
  int _verificationEpoch = 0;
  Object? _verificationJob, _verificationChallenge;
  Completer<void>? _commandDone;
  String? _error;
  (Object, String, String?)? _verificationFailure;
  final _completedJobs = <String>{};
  final _cancelledJobs = <String>{};
  final _automaticVerifications = <(String, String)>{};
  final _serverNames = <(String, String), String?>{};
  final _nameLoads = <(String, String)>{};
  final _totp = TextEditingController();
  AppController get _app => widget.controller;
  bool get _serverDrafts =>
      widget.onSelectServer != null &&
      _app.engineCapabilities?.protonServerSwitching == true;
  String p(String key) => _app.strings.proton(key);
  String get _state => _job['state'] as String? ?? '';
  String? get _verificationError {
    final failure = _verificationFailure;
    return failure != null && failure.$1 == _job['id']
        ? _failure(failure.$2)
        : null;
  }

  String get _displayState =>
      _state == 'cancelled' && _verificationError != null ? 'failed' : _state;
  static const _verificationCodes = {
    'PROTON_VERIFICATION_EXPIRED',
    'PROTON_VERIFICATION_NAVIGATION',
    'PROTON_VERIFICATION_TLS',
    'PROTON_VERIFICATION_BRIDGE',
    'PROTON_VERIFICATION_LOAD',
    'PROTON_VERIFICATION_DOWNLOAD',
    'PROTON_VERIFICATION_HOST_STOPPED',
    'PROTON_VERIFICATION_BUSY',
    'PROTON_VERIFICATION_INVALID',
  };
  static const _runningStates = {
    'running',
    'waiting_totp',
    'waiting_verification',
    'waiting_node',
  };
  bool get _running => _runningStates.contains(_state);
  bool get _blocked => _busy || _launchUnconfirmed || _running || _verifying;
  bool get _remoteUnknown => const {
    'cleanup_failed_remote_state_unknown',
    'worker_failed_remote_state_unknown',
    'revocation_failed_remote_state_unknown',
  }.contains(_job['failure']);

  @override
  void initState() {
    super.initState();
    unawaited(_command({'action': 'get'}));
    _timer = Timer.periodic(const Duration(seconds: 2), (_) {
      if ((_running || _verifying) &&
          !_launchUnconfirmed &&
          !_busy &&
          !_handoffLoading) {
        unawaited(_command({'action': 'get', 'job_id': _job['id']}));
      }
    });
  }

  @override
  void didUpdateWidget(covariant ProtonVpnPanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    _resolveServerName();
  }

  @override
  void dispose() {
    _verificationEpoch++;
    _timer?.cancel();
    _totp.clear();
    _totp.dispose();
    unawaited(_app.cancelProtonHumanVerification().catchError((Object _) {}));
    if (_running) unawaited(_cancelJob(_app, _job['id']));
    super.dispose();
  }

  Future<void> _cancelJob(AppController app, Object? jobId) async {
    if (jobId is! String || !_cancelledJobs.add(jobId)) return;
    try {
      final result = await app.protonVpn({'action': 'cancel', 'job_id': jobId});
      result.clearHandoff();
    } on Object {
      // The panel is gone; native expiry still bounds an unconfirmed cancel.
    }
  }

  String _failure(Object? code) => p(switch (code) {
    'wrong_password' ||
    'invalid_totp' ||
    'authentication_failed' ||
    'invalid_credentials' => 'bad_credentials',
    'unsupported_second_factor' => 'unsupported_second_factor',
    'reauthentication_required' => 'reauthentication_required',
    'profile_in_use' => 'profile_in_use',
    'session_busy' => 'session_busy',
    'api_rejected' => 'api_rejected',
    'session_not_allowed' => 'session_not_allowed',
    'credentialless_disabled' => 'credentialless_disabled',
    'human_verification_invalid' => 'verification_invalid',
    'selected_server_unavailable' => 'selected_server_unavailable',
    'no_free_wireguard_nodes' => 'no_free_nodes',
    'invalid_server_directory' => 'invalid_directory',
    'invalid_client_configuration' => 'invalid_client_configuration',
    'verification_expired' || 'interaction_expired' => 'expired',
    'underlay_changed' => 'underlay_changed',
    'cleanup_failed_remote_state_unknown' ||
    'worker_failed_remote_state_unknown' ||
    'revocation_failed_remote_state_unknown' => 'remote_unknown',
    'rate_limited' => 'rate_limited',
    'api_timeout' || 'api_transport_failed' || 'api_tls_failed' => 'network',
    'human_verification_expired' ||
    'human_verification_failed' ||
    'verification_unavailable' => 'verification_failed',
    'warp_identity_required' || 'identity_required' => 'fallback_account',
    'underlay_unavailable' ||
    'underlay_start_failed' ||
    'warp_fallback_timeout' ||
    'warp_fallback_unavailable' ||
    'account_network_unavailable' ||
    'account_network_budget_exhausted' => 'fallback_failed',
    'verification_network_failed' ||
    'verification_proxy_unavailable' => 'verification_failed',
    'unavailable' => 'unavailable',
    _ => 'failed',
  });

  Future<void> _command(
    Map<String, Object?> command, {
    Uint8List? input,
  }) async {
    if (!mounted || _busy || _launchUnconfirmed && command['action'] != 'get') {
      input?.fillRange(0, input.length, 0);
      return;
    }
    final app = _app;
    if (app.engineCapabilities?.protonAccountWarpFallback == true &&
        const {
          'guest',
          'login',
          'refresh',
          'logout',
        }.contains(command['action'])) {
      command = {...command, 'network_policy_version': 1};
    }
    final recoveringLaunch = _launchUnconfirmed && command['action'] == 'get';
    var recoverLaunch = false;
    setState(() {
      _busy = true;
      _error = null;
    });
    final done = Completer<void>();
    _commandDone = done;
    try {
      final result = await app.protonVpn(command, sensitiveInput: input);
      try {
        final metadata = result.metadata;
        final job = metadata['job'];
        if (!mounted) {
          // A launch can finish after dispose, before its job ID was known.
          // A late poll may describe a job already cancelled by dispose.
          // A global recovery read does not establish ownership after leaving:
          // a new panel may already have started a different native job.
          if (!recoveringLaunch &&
              job is Map<Object?, Object?> &&
              _runningStates.contains(job['state'])) {
            unawaited(_cancelJob(app, job['id']));
          }
          return;
        }
        final profiles = metadata['profiles'];
        final error = metadata['error'];
        setState(() {
          if (recoveringLaunch) {
            final knownJob =
                job is Map<Object?, Object?> &&
                job['id'] is String &&
                (job['id'] as String).isNotEmpty &&
                const {
                  ..._runningStates,
                  'completed',
                  'cancelled',
                  'failed',
                }.contains(job['state']);
            // A valid job is still authoritative when its catalogue read failed.
            // An absent job needs a successful, complete status response.
            final noJob =
                metadata.containsKey('job') &&
                job == null &&
                profiles is List &&
                error == null;
            if (knownJob || noJob) _launchUnconfirmed = false;
          }
          if (job is Map<Object?, Object?> && !_launchUnconfirmed) {
            _job = Map<Object?, Object?>.from(job);
          } else if (!_launchUnconfirmed &&
              (error == null || error == 'job_not_found')) {
            _job = const {};
          }
          // Command errors carry empty defaults, not a fresh job/catalogue.
          // Keep the live job visible and polling until a status reply arrives.
          if (profiles is List && error == null) {
            _profiles = profiles
                .whereType<Map<Object?, Object?>>()
                .map((v) => Map<Object?, Object?>.from(v))
                .toList();
          }
          _error = error == null ? null : _failure(error);
        });
        _resolveServerName();
        if (_verifying &&
            (_job['id'] != _verificationJob ||
                _job['challenge_id'] != _verificationChallenge ||
                _state != 'waiting_verification')) {
          await _dismissVerification();
        }
        final completedJob = _job['id'];
        if (_state == 'completed' &&
            completedJob is String &&
            _completedJobs.add(completedJob)) {
          widget.onChanged();
        }
      } finally {
        result.clearHandoff();
      }
    } on Object catch (error) {
      if (mounted) {
        // A failed launch reply can leave a live job waiting for input. Read
        // its status once, without resending the credentials or guest request.
        recoverLaunch =
            const {'guest', 'login'}.contains(command['action']) &&
            !(error is EngineException &&
                const {
                  'PROTON_INPUT_TOO_LARGE',
                  'PROTON_INVALID_REQUEST',
                }.contains(error.code));
        setState(() {
          if (recoverLaunch) _launchUnconfirmed = true;
          _error = _launchUnconfirmed ? null : p('failed');
        });
      }
    } finally {
      input?.fillRange(0, input.length, 0);
      done.complete();
      if (identical(_commandDone, done)) _commandDone = null;
      if (mounted) {
        setState(() => _busy = false);
        _openAndroidVerification();
      }
    }
    if (recoverLaunch && mounted) await _command({'action': 'get'});
  }

  void _openAndroidVerification() {
    if ((defaultTargetPlatform != TargetPlatform.android &&
            _resumeVerificationJob != _job['id']) ||
        _state != 'waiting_verification' ||
        _busy ||
        _launchUnconfirmed ||
        _verifying ||
        _error != null) {
      return;
    }
    final job = _job['id'], challenge = _job['challenge_id'];
    if (job is String &&
        challenge is String &&
        _automaticVerifications.add((job, challenge))) {
      unawaited(_verify());
    }
  }

  void _submitTotp() {
    if (!RegExp(r'^[0-9]{6}$').hasMatch(_totp.text)) {
      setState(() => _error = _failure('invalid_totp'));
      return;
    }
    final input = Uint8List.fromList(utf8.encode(_totp.text));
    _totp.clear();
    unawaited(_command({'action': 'totp', 'job_id': _job['id']}, input: input));
  }

  Future<void> _login({String? profileId}) async {
    final input = await showDialog<Uint8List>(
      context: context,
      builder: (_) => _ProtonLoginDialog(controller: _app),
    );
    if (input == null) return;
    if (!mounted || _blocked) {
      input.fillRange(0, input.length, 0);
      return;
    }
    await _command({'action': 'login', 'profile_id': ?profileId}, input: input);
  }

  List<String> _related(Map<Object?, Object?> profile) {
    final ids =
        (profile['related_ids'] as List?)?.whereType<String>().toList() ?? [];
    final id = profile['id'];
    if (id is String && !ids.contains(id)) ids.add(id);
    return ids;
  }

  Future<void> _removeAccount(Map<Object?, Object?> profile) async {
    final id = profile['id'];
    if (id is! String || _blocked) return;
    final action = await showDialog<String>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: LucideIcons.trash2,
        danger: true,
        title: p('remove_account'),
        subtitle: profile['name'] as String?,
        content: Text(p('remove_account_confirm')),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: Text(_app.strings.get('cancel')),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, 'forget'),
            child: Text(p('forget_local')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, 'logout'),
            child: Text(p('sign_out_remove')),
          ),
        ],
      ),
    );
    if (action == null || !mounted || _blocked) return;
    if (!(await widget.beforeRemove?.call(_related(profile)) ?? true) ||
        !mounted) {
      return;
    }
    await _command({'action': action, 'profile_id': id});
  }

  String? _selectedServer(Map<Object?, Object?> profile) =>
      _serverDrafts && _related(profile).contains(widget.selectedProfileId)
      ? widget.selectedServerId ?? profile['server_id'] as String?
      : profile['server_id'] as String?;

  void _resolveServerName() {
    final choice = widget.selectedServerId;
    if (!_serverDrafts || choice == null || choice.isEmpty) return;
    final profile = _profiles
        .where((p) => _related(p).contains(widget.selectedProfileId))
        .firstOrNull;
    if (profile == null || choice == profile['server_id']) return;
    final id = profile['id'] as String;
    if (_serverNames.containsKey((id, choice)) ||
        !_nameLoads.add((id, choice))) {
      return;
    }
    unawaited(() async {
      try {
        final nodes = await _loadServers(id);
        if (mounted) setState(() => _cacheServerName(id, choice, nodes));
      } on Object {
        // Keep the persisted choice visible even when its local catalogue is
        // temporarily unavailable; never silently turn it into Automatic.
        if (mounted) setState(() => _cacheServerName(id, choice, []));
      } finally {
        _nameLoads.remove((id, choice));
      }
    }());
  }

  void _cacheServerName(
    String id,
    String choice,
    List<Map<Object?, Object?>> nodes,
  ) {
    _serverNames[(id, choice)] =
        nodes.where((node) => node['id'] == choice).firstOrNull?['name']
            as String?;
    while (_serverNames.length > 256) {
      _serverNames.remove(_serverNames.keys.first);
    }
  }

  Future<List<Map<Object?, Object?>>> _loadServers(String id) =>
      loadCachedProtonServers(_app, id, isCurrent: () => mounted);

  Future<void> _chooseServer(Map<Object?, Object?> profile) async {
    final id = profile['id'];
    if (id is! String ||
        _serversLoading ||
        _blocked ||
        !widget.selectionEnabled) {
      return;
    }
    if ((profile['server_count'] as int? ?? 0) == 0) {
      if (!_related(profile).contains(widget.currentProfileId)) {
        await _command({'action': 'refresh', 'profile_id': id});
      }
      return;
    }
    setState(() {
      _serversLoading = true;
      _error = null;
    });
    try {
      final nodes = await _loadServers(id);
      if (!mounted) return;
      setState(() {
        _serversLoading = false;
      });
      final choice = await showDialog<String>(
        context: context,
        builder: (_) => _ProtonServerPicker(
          nodes: nodes,
          selected: _selectedServer(profile),
          text: p,
        ),
      );
      if (choice == null || !mounted) return;
      if (_serverDrafts) {
        setState(() => _cacheServerName(id, choice, nodes));
        widget.onSelectServer!(id, choice);
        return;
      }
      await _command({
        'action': 'choose_server',
        'profile_id': id,
        'node_id': choice.isEmpty ? null : choice,
      });
      if (mounted && _error == null) setState(() => _serverChanged = id);
    } on Object {
      if (mounted) setState(() => _error = p('invalid_directory'));
    } finally {
      if (mounted) setState(() => _serversLoading = false);
    }
  }

  Widget _account(Map<Object?, Object?> profile) {
    final id = profile['id'] as String;
    final related = _related(profile);
    final current = related.contains(widget.currentProfileId);
    final choice = _selectedServer(profile);
    final server = choice == profile['server_id']
        ? profile['node_name'] as String? ?? ''
        : _serverNames[(id, choice ?? '')] ??
              p(
                _serverNames.containsKey((id, choice ?? ''))
                    ? 'selected_server_unavailable'
                    : 'load_servers',
              );
    final count = profile['server_count'] as int? ?? 0;
    final waiting = _blocked || _serversLoading;
    final theme = Theme.of(context);
    final status = widget.currentStatus;
    // A connected or connecting session already shows that it uses the saved
    // choice; a failed one keeps both facts visible.
    final live =
        current &&
        status != null &&
        (status.$2 == StatusTone.success || status.$2 == StatusTone.brand);
    final tags = <(String, StatusTone)>[
      if (current && status != null) status,
      if (related.contains(widget.savedProfileId) && !live)
        (p('saved_account'), StatusTone.brand),
    ];
    final hint = theme.textTheme.bodySmall?.copyWith(
      color: theme.colorScheme.onSurfaceVariant,
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ChainExitTile<String>(
          value: id,
          tileKey: ValueKey('proton-account-$id'),
          selected: related.contains(widget.selectedProfileId),
          enabled: widget.selectionEnabled && !waiting,
          title: Text(profile['name'] as String? ?? p('account')),
          subtitle: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                profile['needs_login'] == true
                    ? p('session_requires_login')
                    : p('signed_in'),
              ),
              if (tags.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: Wrap(
                    spacing: 12,
                    runSpacing: 4,
                    children: [
                      for (final (label, tone) in tags)
                        InlineStatus(label: label, tone: tone),
                    ],
                  ),
                ),
            ],
          ),
          trailing: PopupMenuButton<String>(
            key: ValueKey('proton-account-menu-$id'),
            tooltip: p('account_actions'),
            enabled: !waiting,
            onSelected: (action) => unawaited(switch (action) {
              'refresh' => _command({'action': 'refresh', 'profile_id': id}),
              'login' => _login(profileId: id),
              _ => _removeAccount(profile),
            }),
            itemBuilder: (_) => [
              PopupMenuItem(
                value: 'refresh',
                enabled: !current,
                child: Text(p('refresh_servers')),
              ),
              if (profile['needs_login'] == true)
                PopupMenuItem(
                  value: 'login',
                  enabled: !current,
                  child: Text(p('needs_login')),
                ),
              PopupMenuItem(
                value: 'remove',
                enabled: !current,
                child: Text(p('remove_account')),
              ),
            ],
          ),
        ),
        // The server preference reads as one value with its own action,
        // aligned with the account name above it.
        Padding(
          padding: const EdgeInsetsDirectional.only(start: 40),
          child: ListTile(
            key: ValueKey('proton-account-server-$id'),
            dense: true,
            contentPadding: const EdgeInsetsDirectional.only(start: 16),
            title: Text(
              '${p('selected_server')}: ${choice == null || choice.isEmpty ? p('automatic_server') : server}',
            ),
            subtitle: Text(
              count == 0
                  ? p('load_servers')
                  : '${p('available_servers')}: $count',
            ),
            trailing: const Icon(LucideIcons.chevronRight, size: 18),
            enabled:
                !waiting &&
                widget.selectionEnabled &&
                (!current || _serverDrafts && count > 0),
            onTap: () => unawaited(_chooseServer(profile)),
          ),
        ),
        Padding(
          padding: const EdgeInsetsDirectional.only(start: 56),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            spacing: 4,
            children: [
              if (profile['cleanup_pending'] == true)
                InlineStatus(
                  label: p('cleanup_pending'),
                  tone: StatusTone.warning,
                ),
              if (_serverChanged == id) Text(p('next_connection'), style: hint),
              if (current && !_serverDrafts)
                Text(p('disconnect_to_change'), style: hint),
            ],
          ),
        ),
      ],
    );
  }

  Future<void> _dismissVerification() async {
    _verificationEpoch++;
    _verificationJob = null;
    _verificationChallenge = null;
    try {
      await _app.cancelProtonHumanVerification();
    } on Object {
      // Late results remain invalid even when the native close acknowledgement fails.
    }
    if (mounted) {
      setState(() {
        _verifying = false;
        _handoffLoading = false;
      });
    }
  }

  Future<void> _cancel({
    String? verificationFailure,
    String? verificationCode,
  }) async {
    final jobId = _job['id'];
    setState(() {
      _verificationFailure = jobId != null && verificationFailure != null
          ? (jobId, verificationFailure, verificationCode)
          : null;
    });
    await _dismissVerification();
    await _commandDone?.future;
    if (mounted && _job['id'] == jobId && _running) {
      await _command({'action': 'cancel', 'job_id': jobId});
    }
  }

  bool _currentVerification(int epoch, Object jobId, Object challengeId) =>
      mounted &&
      epoch == _verificationEpoch &&
      _verifying &&
      _state == 'waiting_verification' &&
      _job['id'] == jobId &&
      _job['challenge_id'] == challengeId;

  Object? _resumeVerificationJob;

  Future<void> _verify() async {
    if (_busy || _launchUnconfirmed || _verifying) return;
    final jobId = _job['id'], challengeId = _job['challenge_id'];
    if (jobId == null || challengeId == null) return;
    // Manual openings consume the handoff too. A command reply may still
    // contain this challenge until the worker publishes its next snapshot.
    if (jobId is String && challengeId is String) {
      _automaticVerifications.add((jobId, challengeId));
    }
    final epoch = ++_verificationEpoch;
    _verificationJob = jobId;
    _verificationChallenge = challengeId;
    setState(() {
      _verifying = true;
      _handoffLoading = true;
      _error = null;
    });
    Uint8List? answer;
    try {
      final response = await _app.protonVpn({
        'action': 'handoff',
        'job_id': jobId,
        'challenge_id': challengeId,
      });
      try {
        if (!_currentVerification(epoch, jobId, challengeId)) return;
        if (response.sensitiveHandoff.isEmpty) {
          throw StateError('Unavailable handoff');
        }
        setState(() => _handoffLoading = false);
        final nativeAnswer = await _app.completeProtonHumanVerification(
          response.sensitiveHandoff,
        );
        if (nativeAnswer != null) {
          answer = takeProtonSensitiveBytes(nativeAnswer);
        }
      } finally {
        response.clearHandoff();
      }
      await _commandDone?.future;
      if (!_currentVerification(epoch, jobId, challengeId)) return;
      if (answer == null) {
        await _cancel();
      } else {
        if (jsonDecode(utf8.decode(answer))['type'] ==
            'USQUE_VERIFICATION_NETWORK_FAILED') {
          _resumeVerificationJob = jobId;
        }
        await _command({
          'action':
              jsonDecode(utf8.decode(answer))['type'] ==
                  'USQUE_VERIFICATION_NETWORK_FAILED'
              ? 'verification_failed'
              : 'verify',
          'job_id': jobId,
          'challenge_id': challengeId,
        }, input: answer);
      }
    } on Object catch (error) {
      if (_currentVerification(epoch, jobId, challengeId)) {
        final code =
            error is EngineException && _verificationCodes.contains(error.code)
            ? error.code
            : null;
        await _cancel(
          verificationFailure: code == 'PROTON_VERIFICATION_EXPIRED'
              ? 'verification_expired'
              : 'human_verification_failed',
          verificationCode: code?.substring('PROTON_VERIFICATION_'.length),
        );
      }
    } finally {
      answer?.fillRange(0, answer.length, 0);
      if (mounted && epoch == _verificationEpoch) {
        setState(() {
          _verifying = false;
          _handoffLoading = false;
        });
        _openAndroidVerification();
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final strings = _app.strings;
    final nodes = (_job['nodes'] as List? ?? const [])
        .whereType<Map<Object?, Object?>>();
    final canAdd = !_blocked && !_serversLoading;
    void guest() => unawaited(_command({'action': 'guest'}));
    void login() => unawaited(_login());
    return ContentSection(
      title: p('accounts'),
      gap: 12,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        spacing: 12,
        children: [
          HintText(p('privacy')),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              // Setup leads while there is no account; afterwards adding
              // another account is a secondary action under one button.
              if (_profiles.isEmpty) ...[
                FilledButton.tonal(
                  onPressed: canAdd ? guest : null,
                  child: Text(p('guest')),
                ),
                OutlinedButton(
                  onPressed: canAdd ? login : null,
                  child: Text(p('login')),
                ),
              ] else
                MenuAnchor(
                  menuChildren: [
                    MenuItemButton(onPressed: guest, child: Text(p('guest'))),
                    MenuItemButton(onPressed: login, child: Text(p('login'))),
                  ],
                  builder: (context, menu, _) => OutlinedButton.icon(
                    key: const ValueKey('proton-add-account'),
                    onPressed: canAdd
                        ? () => menu.isOpen ? menu.close() : menu.open()
                        : null,
                    icon: const Icon(LucideIcons.userPlus),
                    label: Text(p('add_account')),
                  ),
                ),
              if (_launchUnconfirmed)
                TextButton(
                  onPressed: _busy
                      ? null
                      : () => unawaited(_command({'action': 'get'})),
                  child: Text(strings.get('settings_refresh')),
                ),
              if (_running && !_launchUnconfirmed)
                TextButton(
                  onPressed: _busy || _handoffLoading
                      ? null
                      : () => unawaited(_cancel()),
                  child: Text(strings.get('cancel')),
                ),
            ],
          ),
          if (_busy || _state == 'running' || _verifying)
            const LinearProgressIndicator(minHeight: 2),
          if (_launchUnconfirmed) Text(p('result_unconfirmed')),
          if (_state.isNotEmpty && !_launchUnconfirmed)
            Semantics(
              liveRegion: true,
              child: Text(
                p(
                  _state == 'running'
                      ? 'working'
                      : _state == 'completed' && _job['profile_id'] == null
                      ? 'removed'
                      : _displayState,
                ),
              ),
            ),
          if (_state == 'waiting_totp' && !_launchUnconfirmed)
            Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _totp,
                    obscureText: true,
                    maxLength: 6,
                    keyboardType: TextInputType.number,
                    inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                    autocorrect: false,
                    enableSuggestions: false,
                    decoration: InputDecoration(labelText: p('totp')),
                  ),
                ),
                const SizedBox(width: 8),
                TextButton(
                  onPressed: _busy ? null : _submitTotp,
                  child: Text(p('submit')),
                ),
              ],
            ),
          if (_state == 'waiting_verification' && !_launchUnconfirmed) ...[
            Text(p('verification_hint')),
            Align(
              alignment: AlignmentDirectional.centerStart,
              child: FilledButton(
                onPressed: _busy || _verifying
                    ? null
                    : () => unawaited(_verify()),
                child: Text(p('verify')),
              ),
            ),
          ],
          if (_state == 'waiting_node' && !_launchUnconfirmed) ...[
            Text(p('choose_node')),
            for (final node in nodes)
              ListTile(
                title: Text(node['name'] as String? ?? ''),
                subtitle: Text(
                  [
                    node['country'],
                    node['city'],
                  ].whereType<String>().where((v) => v.isNotEmpty).join(' · '),
                ),
                onTap: _busy
                    ? null
                    : () => unawaited(
                        _command({
                          'action': 'select_node',
                          'job_id': _job['id'],
                          'node_id': node['id'],
                        }),
                      ),
              ),
          ],
          if (_serversLoading) const LinearProgressIndicator(minHeight: 2),
          if (_profiles.isEmpty && !_running && !_launchUnconfirmed)
            Text(p('empty')),
          if (_profiles.isNotEmpty) ...[
            RadioGroup<String>(
              groupValue:
                  _profiles
                          .where(
                            (profile) => _related(
                              profile,
                            ).contains(widget.selectedProfileId),
                          )
                          .firstOrNull?['id']
                      as String?,
              onChanged: (id) {
                if (id != null) widget.onSelectProfile?.call(id);
              },
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                spacing: 16,
                children: _profiles.map(_account).toList(),
              ),
            ),
          ],
          if (_error != null ||
              _verificationError != null ||
              _state == 'failed')
            WarningBanner(
              key: const ValueKey('proton-error'),
              title: null,
              message:
                  _error ?? _verificationError ?? _failure(_job['failure']),
              danger: true,
            ),
          if (_state == 'cancelled' && _remoteUnknown)
            WarningBanner(title: null, message: p('cancelled_remote_unknown')),
          if (_verificationFailure?.$3 case final String code
              when _verificationError != null)
            Text('${p('verification_error')}: $code'),
          if (_running && _job['account_network'] == 'warp')
            HintText(
              p(
                _job['network_phase'] == 'starting'
                    ? 'fallback_starting'
                    : 'fallback_active',
              ),
            ),
          if (_job['phase'] case final String phase
              when _state == 'failed' &&
                  const {
                    'anonymous_session',
                    'credentialless',
                    'session_scopes',
                    'protected_user',
                    'vpn_permissions',
                    'session_refresh',
                    'srp_info',
                    'srp_proof',
                    'totp',
                    'directory',
                    'certificate',
                    'secure_storage',
                    'session_revoke',
                  }.contains(phase))
            Text('${p('stage')}: ${p('stage_$phase')}'),
          if (_state == 'failed' &&
              (_job['api_code'] is int || _job['http_status'] is int))
            Text(
              [
                if (_job['api_code'] case final int code
                    when code >= 0 && code <= 999999)
                  'API $code',
                if (_job['http_status'] case final int status
                    when status >= 100 && status <= 599)
                  'HTTP $status',
              ].join(' · '),
            ),
        ],
      ),
    );
  }
}

Future<List<Map<Object?, Object?>>> loadCachedProtonServers(
  AppController controller,
  String id, {
  bool Function()? isCurrent,
}) async {
  final nodes = <Map<Object?, Object?>>[];
  var offset = 0, total = 0;
  String? revision;
  do {
    final response = await controller.protonVpn({
      'action': 'servers',
      'profile_id': id,
      'offset': offset,
    });
    try {
      if (isCurrent != null && !isCurrent()) return [];
      final metadata = response.metadata;
      final page = (metadata['servers'] as List?)
          ?.whereType<Map<Object?, Object?>>()
          .toList();
      final nextTotal = metadata['server_total'];
      final nextRevision = metadata['server_revision'];
      if (metadata['error'] != null ||
          page == null ||
          page.length > 128 ||
          nextTotal is! int ||
          nextTotal < 0 ||
          nextTotal > 20000 ||
          nextRevision is! String ||
          metadata['server_offset'] != offset ||
          offset > nextTotal ||
          offset != 0 && (total != nextTotal || revision != nextRevision) ||
          page.isEmpty && offset < nextTotal) {
        throw const FormatException('Invalid server page');
      }
      total = nextTotal;
      revision = nextRevision;
      nodes.addAll(page);
      offset += page.length;
      if (offset > total ||
          nodes.map((node) => node['id']).toSet().length != nodes.length) {
        throw const FormatException('Invalid server catalogue');
      }
    } finally {
      response.clearHandoff();
    }
  } while (offset < total);
  return nodes;
}

/// Select an already cached logical server without changing account preferences,
/// starting sign-in, or applying a live network configuration.
Future<String?> chooseSavedProtonServer(
  BuildContext context,
  AppController controller,
  String profileId,
  String? selected,
) async {
  final nodes = await loadCachedProtonServers(
    controller,
    profileId,
    isCurrent: () => context.mounted,
  );
  if (!context.mounted) return null;
  return showDialog<String>(
    context: context,
    builder: (_) => _ProtonServerPicker(
      nodes: nodes,
      selected: selected,
      text: controller.strings.proton,
    ),
  );
}

class _ProtonServerPicker extends StatefulWidget {
  const _ProtonServerPicker({
    required this.nodes,
    required this.selected,
    required this.text,
  });
  final List<Map<Object?, Object?>> nodes;
  final String? selected;
  final String Function(String) text;
  @override
  State<_ProtonServerPicker> createState() => _ProtonServerPickerState();
}

class _ProtonServerPickerState extends State<_ProtonServerPicker> {
  final _search = TextEditingController();
  final _scroll = ScrollController();
  @override
  void dispose() {
    _search.dispose();
    _scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final query = _search.text.trim().toLowerCase();
    final nodes = widget.nodes
        .where(
          (node) => [
            node['name'],
            node['country'],
            node['city'],
          ].whereType<String>().join(' ').toLowerCase().contains(query),
        )
        .toList();
    final available =
        MediaQuery.sizeOf(context).height -
        MediaQuery.viewInsetsOf(context).bottom;
    return UsqueDialog(
      icon: LucideIcons.globe,
      title: widget.text('choose_server'),
      content: SizedBox(
        width: 560,
        height: (available * .55).clamp(100.0, 480.0),
        child: Column(
          children: [
            TextField(
              controller: _search,
              onChanged: (_) => setState(() {}),
              decoration: InputDecoration(
                labelText: widget.text('search_servers'),
                prefixIcon: const Icon(LucideIcons.search),
              ),
            ),
            const SizedBox(height: 8),
            Expanded(
              child: RadioGroup<String>(
                groupValue: widget.selected ?? '',
                onChanged: (value) {
                  if (value != null) Navigator.pop(context, value);
                },
                child: ScrollConfiguration(
                  behavior: ScrollConfiguration.of(
                    context,
                  ).copyWith(scrollbars: false),
                  child: Scrollbar(
                    controller: _scroll,
                    thumbVisibility: true,
                    interactive: true,
                    child: ListView.builder(
                      controller: _scroll,
                      primary: false,
                      padding: const EdgeInsetsDirectional.only(end: 12),
                      itemCount: nodes.length + 1,
                      itemBuilder: (context, index) {
                        if (index == 0) {
                          return RadioListTile<String>(
                            value: '',
                            title: Text(widget.text('automatic_server')),
                            subtitle: Text(widget.text('automatic_hint')),
                          );
                        }
                        final node = nodes[index - 1];
                        return RadioListTile<String>(
                          value: node['id'] as String,
                          title: Text(node['name'] as String? ?? ''),
                          subtitle: Text.rich(
                            TextSpan(
                              children: [
                                WidgetSpan(
                                  alignment: PlaceholderAlignment.middle,
                                  child: Padding(
                                    padding: const EdgeInsetsDirectional.only(
                                      end: 8,
                                    ),
                                    child: CountryFlag(
                                      countryCode: node['country'] as String?,
                                    ),
                                  ),
                                ),
                                TextSpan(
                                  text: [node['country'], node['city']]
                                      .whereType<String>()
                                      .where((value) => value.isNotEmpty)
                                      .join(' · '),
                                ),
                              ],
                            ),
                          ),
                        );
                      },
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: Text(widget.text('close')),
        ),
      ],
    );
  }
}

class _ProtonLoginDialog extends StatefulWidget {
  const _ProtonLoginDialog({required this.controller});
  final AppController controller;
  @override
  State<_ProtonLoginDialog> createState() => _ProtonLoginDialogState();
}

class _ProtonLoginDialogState extends State<_ProtonLoginDialog> {
  final _username = TextEditingController();
  final _password = TextEditingController();
  @override
  void dispose() {
    _username.clear();
    _password.clear();
    _username.dispose();
    _password.dispose();
    super.dispose();
  }

  void _submit() {
    if (_username.text.isEmpty || _password.text.isEmpty) return;
    final input = Uint8List.fromList(
      utf8.encode(
        jsonEncode({'username': _username.text, 'password': _password.text}),
      ),
    );
    _username.clear();
    _password.clear();
    FocusScope.of(context).unfocus();
    Navigator.pop(context, input);
  }

  @override
  Widget build(BuildContext context) {
    final strings = widget.controller.strings;
    return UsqueDialog(
      icon: LucideIcons.user,
      title: strings.proton('login'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          TextField(
            controller: _username,
            autofocus: true,
            autofillHints: const [],
            autocorrect: false,
            enableSuggestions: false,
            decoration: InputDecoration(labelText: strings.proton('username')),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _password,
            obscureText: true,
            autofillHints: const [],
            autocorrect: false,
            enableSuggestions: false,
            decoration: InputDecoration(labelText: strings.proton('password')),
            onSubmitted: (_) => _submit(),
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: Text(strings.get('cancel')),
        ),
        FilledButton(onPressed: _submit, child: Text(strings.proton('submit'))),
      ],
    );
  }
}
