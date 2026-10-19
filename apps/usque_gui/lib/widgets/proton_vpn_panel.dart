import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/proton_strings.dart';
import '../services/engine_client.dart' show EngineException;
import '../state/app_controller.dart';
import 'common.dart';
import 'usque_dialog.dart';

/// Native controls around a native-owned, cancellable authentication job.
class ProtonVpnPanel extends StatefulWidget {
  const ProtonVpnPanel({
    required this.controller,
    required this.onChanged,
    super.key,
  });
  final AppController controller;
  final VoidCallback onChanged;

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
  bool _verifying = false;
  bool _handoffLoading = false;
  int _verificationEpoch = 0;
  Object? _verificationJob, _verificationChallenge;
  Completer<void>? _commandDone;
  String? _error;
  (Object, String, String?)? _verificationFailure;
  final _completedJobs = <String>{};
  final _cancelledJobs = <String>{};
  final _automaticVerifications = <(String, String)>{};
  final _totp = TextEditingController();
  AppController get _app => widget.controller;
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
      if ((_running || _verifying) && !_busy && !_handoffLoading) {
        unawaited(_command({'action': 'get', 'job_id': _job['id']}));
      }
    });
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
    'verification_expired' || 'interaction_expired' => 'expired',
    'underlay_changed' => 'underlay_changed',
    'cleanup_failed_remote_state_unknown' ||
    'worker_failed_remote_state_unknown' ||
    'revocation_failed_remote_state_unknown' => 'remote_unknown',
    'rate_limited' => 'rate_limited',
    'underlay_unavailable' ||
    'underlay_start_failed' ||
    'identity_required' => 'underlay',
    'human_verification_expired' ||
    'human_verification_failed' ||
    'verification_unavailable' => 'verification_failed',
    'unavailable' => 'unavailable',
    _ => 'failed',
  });

  Future<void> _forget(String profileId) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: LucideIcons.trash2,
        title: p('forget'),
        content: Text(p('forget_confirm')),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(_app.strings.get('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(p('forget')),
          ),
        ],
      ),
    );
    if (confirmed == true && mounted && !_busy && !_running && !_verifying) {
      await _command({'action': 'forget', 'profile_id': profileId});
    }
  }

  Future<void> _command(
    Map<String, Object?> command, {
    Uint8List? input,
  }) async {
    if (!mounted || _busy) {
      input?.fillRange(0, input.length, 0);
      return;
    }
    final app = _app;
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
          if (job is Map<Object?, Object?> &&
              _runningStates.contains(job['state'])) {
            unawaited(_cancelJob(app, job['id']));
          }
          return;
        }
        final profiles = metadata['profiles'];
        final error = metadata['error'];
        setState(() {
          if (job is Map<Object?, Object?>) {
            _job = Map<Object?, Object?>.from(job);
          } else if (error == null || error == 'job_not_found') {
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
    } on Object {
      if (mounted) setState(() => _error = p('failed'));
    } finally {
      input?.fillRange(0, input.length, 0);
      done.complete();
      if (identical(_commandDone, done)) _commandDone = null;
      if (mounted) {
        setState(() => _busy = false);
        _openAndroidVerification();
      }
    }
  }

  void _openAndroidVerification() {
    if (defaultTargetPlatform != TargetPlatform.android ||
        _state != 'waiting_verification' ||
        _busy ||
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

  Future<void> _login() async {
    final input = await showDialog<Uint8List>(
      context: context,
      builder: (_) => _ProtonLoginDialog(controller: _app),
    );
    if (input == null) return;
    if (!mounted || _busy || _running || _verifying) {
      input.fillRange(0, input.length, 0);
      return;
    }
    await _command({'action': 'login'}, input: input);
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

  Future<void> _verify() async {
    if (_busy || _verifying) return;
    final jobId = _job['id'], challengeId = _job['challenge_id'];
    if (jobId == null || challengeId == null) return;
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
        answer = await _app.completeProtonHumanVerification(
          response.sensitiveHandoff,
        );
      } finally {
        response.clearHandoff();
      }
      await _commandDone?.future;
      if (!_currentVerification(epoch, jobId, challengeId)) return;
      if (answer == null) {
        await _cancel();
      } else {
        await _command({
          'action': 'verify',
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
    return ContentSection(
      title: p('title'),
      gap: 12,
      children: [
        Text(p('privacy'), style: Theme.of(context).textTheme.bodySmall),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            OutlinedButton(
              onPressed: _busy || _running || _verifying
                  ? null
                  : () => unawaited(_command({'action': 'guest'})),
              child: Text(p('guest')),
            ),
            OutlinedButton(
              onPressed: _busy || _running || _verifying
                  ? null
                  : () => unawaited(_login()),
              child: Text(p('login')),
            ),
            if (_running)
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
        if (_state.isNotEmpty)
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
        if (_state == 'waiting_totp')
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
        if (_state == 'waiting_verification') ...[
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
        if (_state == 'waiting_node') ...[
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
        for (final profile in _profiles)
          Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              ListTile(
                title: Text(profile['name'] as String? ?? ''),
                subtitle: Text(
                  '${profile['node_name'] ?? ''}${profile['needs_login'] == true ? ' · ${p('needs_login')}' : ''}',
                ),
                trailing: IconButton(
                  tooltip: p('refresh'),
                  icon: const Icon(LucideIcons.refreshCw),
                  onPressed: _busy || _running || _verifying
                      ? null
                      : () => unawaited(
                          _command({
                            'action': 'refresh',
                            'profile_id': profile['id'],
                          }),
                        ),
                ),
              ),
              TextButton(
                onPressed: _busy || _running || _verifying
                    ? null
                    : () => unawaited(_forget(profile['id'] as String)),
                child: Text(p('forget')),
              ),
            ],
          ),
        if (_error != null || _verificationError != null || _state == 'failed')
          Text(
            _error ?? _verificationError ?? _failure(_job['failure']),
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        if (_state == 'cancelled' && _remoteUnknown)
          Text(
            p('cancelled_remote_unknown'),
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        if (_verificationFailure?.$3 case final String code
            when _verificationError != null)
          Text('${p('verification_error')}: $code'),
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
