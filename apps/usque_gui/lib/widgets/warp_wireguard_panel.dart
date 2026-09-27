import 'dart:async';
import 'dart:io';
import 'package:flutter/material.dart';
import '../core/chain_strings.dart';
import '../core/warp_strings.dart';
import '../models/app_models.dart';
import '../services/engine_client.dart';
import '../state/app_controller.dart';
import 'common.dart';

class WarpWireguardPanel extends StatefulWidget {
  const WarpWireguardPanel({
    required this.controller,
    required this.endpoint,
    required this.overrideEndpoint,
    required this.onEndpoint,
    required this.onValid,
    required this.onGenerated,
    super.key,
  });
  final AppController controller;
  final ChainEndpoint? endpoint, overrideEndpoint;
  final ValueChanged<ChainEndpoint?> onEndpoint;
  final ValueChanged<bool> onValid;
  final VoidCallback onGenerated;
  @override
  State<WarpWireguardPanel> createState() => WarpWireguardPanelState();
}

class WarpWireguardPanelState extends State<WarpWireguardPanel> {
  Future<void> generate() async {
    if (!_running) await _command('generate');
  }

  final _ip = TextEditingController(), _port = TextEditingController();
  Timer? _timer;
  Map<Object?, Object?> _response = const {};
  bool _busy = false, _invalid = false;
  String? _jobId, _error, _generated;
  Map<Object?, Object?> get _job =>
      _response['job'] as Map<Object?, Object?>? ?? const {};
  bool get _running => _job['state'] == 'running';
  AppController get _app => widget.controller;
  String w(String key) => _app.strings.warp(key);
  @override
  void initState() {
    super.initState();
    _setText();
    unawaited(_command('get'));
    _timer = Timer.periodic(const Duration(seconds: 2), (_) {
      if (_running) unawaited(_command('get'));
    });
  }

  @override
  void didUpdateWidget(covariant WarpWireguardPanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.endpoint != widget.endpoint ||
        oldWidget.overrideEndpoint != widget.overrideEndpoint) {
      _setText();
    }
  }

  void _setText() {
    final endpoint = widget.overrideEndpoint ?? widget.endpoint;
    _ip.text = endpoint?.host ?? '';
    _port.text = '${endpoint?.port ?? 2408}';
    _invalid = false;
  }

  @override
  void dispose() {
    _timer?.cancel();
    _ip.dispose();
    _port.dispose();
    super.dispose();
  }

  void _edit() {
    final address = InternetAddress.tryParse(_ip.text.trim());
    final port = int.tryParse(_port.text);
    final valid =
        address != null &&
        !address.isLoopback &&
        !address.isMulticast &&
        !address.isLinkLocal &&
        address.address != '0.0.0.0' &&
        address.address != '::' &&
        port != null &&
        port > 0 &&
        port <= 65535;
    setState(() => _invalid = !valid);
    widget.onValid(valid);
    if (valid) widget.onEndpoint(ChainEndpoint(address.address, port));
  }

  String _failure(String? code) {
    // Only native codes from this fixed grammar are suitable for user-visible
    // diagnostics. Never echo arbitrary platform exception text.
    if (code != null &&
        RegExp(
          r'^registration_((create|device|activate)_(http_[345][0-9]{2}|timeout|dns|connect|tls|protocol|size_limit|encoding|cancelled)|response_invalid)$',
        ).hasMatch(code)) {
      return '${w('registration_failed')}\n$code';
    }
    return switch (code) {
      'connect_ip_required' => _app.strings.chain('l4'),
      'generation_busy' => '${w('generate')}…',
      'secure_storage_failed' => _app.strings.chain('secure_storage_failed'),
      'identity_required' || 'identity_invalid' => w('identity_required'),
      'underlay_unavailable' || 'underlay_start_failed' => w('underlay_failed'),
      _ => w('unavailable'),
    };
  }

  Future<void> _command(String action) async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final result = await _app.warpWireguard({
        'action': action,
        if (action == 'cancel' && _jobId != null) 'job_id': _jobId,
      });
      if (!mounted) return;
      if (result['error'] case final String error) {
        setState(() => _error = _failure(error));
        return;
      }
      final job = result['job'] as Map<Object?, Object?>?;
      setState(() {
        _response = result;
        _jobId = _job['id'] as String?;
      });
      final generated = job?['profile_id'] as String?;
      if (generated != null && generated != _generated) {
        _generated = generated;
        widget.onGenerated();
      }
    } on EngineException catch (failure) {
      if (mounted) setState(() => _error = _failure(failure.code));
    } on Exception {
      if (mounted) setState(() => _error = w('unavailable'));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final strings = _app.strings;
    return ContentSection(
      title: 'WARP via WireGuard',
      gap: 12,
      children: [
        TextField(
          controller: _ip,
          enabled: widget.endpoint != null,
          decoration: const InputDecoration(labelText: 'Endpoint IP'),
          onChanged: (_) => _edit(),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: _port,
          enabled: widget.endpoint != null,
          keyboardType: TextInputType.number,
          decoration: InputDecoration(
            labelText: strings.get('port'),
            errorText: _invalid ? strings.chain('invalid_endpoint') : null,
          ),
          onChanged: (_) => _edit(),
        ),
        Align(
          alignment: AlignmentDirectional.centerStart,
          child: TextButton(
            onPressed: widget.overrideEndpoint == null
                ? null
                : () {
                    widget.onValid(true);
                    widget.onEndpoint(null);
                  },
            child: Text(strings.get('reset')),
          ),
        ),
        if (_running)
          Align(
            alignment: AlignmentDirectional.centerStart,
            child: TextButton(
              onPressed: _busy ? null : () => unawaited(_command('cancel')),
              child: Text(strings.get('cancel')),
            ),
          ),
        if (_busy || _running) const LinearProgressIndicator(minHeight: 2),
        if (_job.isNotEmpty)
          Text(
            '${w('generate')}${_running ? '…' : ' · ${w(_job['state'] as String? ?? 'failed')}'}',
          ),
        if (_error != null ||
            (_job['failure'] != null && _job['state'] != 'cancelled'))
          Text(
            _error ?? _failure(_job['failure'] as String?),
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
      ],
    );
  }
}
