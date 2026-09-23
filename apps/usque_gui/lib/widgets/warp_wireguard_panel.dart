import 'dart:async';
import 'dart:io';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import '../core/chain_strings.dart';
import '../core/warp_strings.dart';
import '../models/app_models.dart';
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

  final _ip = TextEditingController(),
      _port = TextEditingController(),
      _target = TextEditingController();
  Timer? _timer;
  Map<Object?, Object?> _response = const {};
  bool _busy = false, _ipv6 = false, _invalid = false;
  String _mode = 'quick';
  String? _jobId, _country, _error, _generated;
  int _cursor = 0;
  final _previous = <int>[];
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
    _target.dispose();
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

  String _failure(String? code) => switch (code) {
    'connect_ip_required' => _app.strings.chain('l4'),
    'context_changed' => w('stale'),
    'scan_busy' => w('running'),
    'secure_storage_failed' => _app.strings.chain('secure_storage_failed'),
    _ => w('unavailable'),
  };
  Future<void> _command(String action) async {
    if (_busy) return;
    if (action == 'start' &&
        _mode == 'target' &&
        InternetAddress.tryParse(_target.text.trim()) == null) {
      setState(() => _error = _app.strings.chain('invalid_endpoint'));
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final result = await _app.warpWireguard({
        'action': action,
        if (_jobId != null) 'job_id': _jobId,
        'mode': _mode,
        'ipv6': _ipv6,
        if (_mode == 'target' &&
            InternetAddress.tryParse(_target.text.trim()) != null)
          'target': _target.text.trim(),
        'cursor': _cursor,
        if (_country != null) 'country': _country,
      });
      if (!mounted) return;
      if (result['error'] case final String error) {
        setState(() => _error = _failure(error));
        return;
      }
      final job = result['job'] as Map<Object?, Object?>?;
      setState(() {
        _response = result;
        _jobId = job?['id'] as String?;
      });
      final generated = job?['profile_id'] as String?;
      if (generated != null && generated != _generated) {
        _generated = generated;
        widget.onGenerated();
      }
    } on Exception {
      if (mounted) setState(() => _error = w('unavailable'));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  void _resetPage() {
    _cursor = 0;
    _previous.clear();
  }

  @override
  Widget build(BuildContext context) {
    final strings = _app.strings;
    final rows = (_response['results'] as List? ?? const [])
        .whereType<Map<Object?, Object?>>();
    final countries = (_job['countries'] as List? ?? const [])
        .whereType<String>()
        .toList();
    final history = (_response['history'] as List? ?? const [])
        .whereType<Map<Object?, Object?>>()
        .toList();
    final total = _job['total'] as int? ?? 0;
    final done = _job['completed'] as int? ?? 0;
    final selected = widget.overrideEndpoint ?? widget.endpoint;
    return ContentSection(
      title: 'WARP via WireGuard',
      gap: 12,
      children: [
        Text(w('hint')),
        const SizedBox(height: 16),
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
        DropdownButtonFormField<String>(
          key: ValueKey('mode-$_mode'),
          isExpanded: true,
          initialValue: _mode,
          decoration: InputDecoration(labelText: w('scan')),
          items: [
            for (final mode in ['quick', 'target', 'full'])
              DropdownMenuItem(
                value: mode,
                enabled: !(_ipv6 && mode == 'full'),
                child: Text(
                  w(mode),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
          ],
          onChanged: _busy
              ? null
              : (mode) {
                  if (mode != null) setState(() => _mode = mode);
                },
        ),
        SwitchListTile.adaptive(
          contentPadding: EdgeInsets.zero,
          title: const Text('IPv6'),
          value: _ipv6,
          onChanged: _busy
              ? null
              : (value) => setState(() {
                  _ipv6 = value;
                  if (value && _mode == 'full') _mode = 'quick';
                }),
        ),
        if (_mode == 'target')
          TextField(
            controller: _target,
            decoration: InputDecoration(
              labelText: 'Endpoint IP',
              helperText: w('target'),
            ),
          ),
        if (_mode == 'full') Text(w('full_hint')),
        const SizedBox(height: 8),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            FilledButton.icon(
              onPressed: _busy || _running
                  ? null
                  : () {
                      _resetPage();
                      _country = null;
                      unawaited(_command('start'));
                    },
              icon: const Icon(LucideIcons.search),
              label: Text(w('scan')),
            ),
            if (_running && _job['kind'] != 'generate')
              OutlinedButton(
                onPressed: _busy ? null : () => unawaited(_command('pause')),
                child: Text(w('pause')),
              ),
            if (_job['kind'] != 'generate' &&
                (_job['state'] == 'paused' || _job['state'] == 'failed'))
              OutlinedButton(
                onPressed: _busy ? null : () => unawaited(_command('resume')),
                child: Text(w('resume')),
              ),
            if (_running || _job['state'] == 'paused')
              TextButton(
                onPressed: _busy ? null : () => unawaited(_command('cancel')),
                child: Text(strings.get('cancel')),
              ),
          ],
        ),
        if (_busy) const LinearProgressIndicator(minHeight: 2),
        const SizedBox(height: 12),
        if (_job.isNotEmpty)
          Text(
            _job['kind'] == 'generate'
                ? '${w('generate')}${_running ? '…' : ' · ${w(_job['state'] as String? ?? 'unknown')}'}'
                : '${w(_job['state'] as String? ?? 'unknown')} · $done / $total · ${_job['working'] ?? 0}',
          ),
        if (_running && total > 0)
          LinearProgressIndicator(value: (done / total).clamp(0, 1)),
        if (_error != null ||
            (_job['failure'] != null &&
                !['paused', 'cancelled'].contains(_job['state'])))
          Text(
            _error ?? _failure(_job['failure'] as String?),
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        if (history.isNotEmpty) const SizedBox(height: 16),
        if (history.isNotEmpty)
          DropdownButtonFormField<String>(
            key: ValueKey('history-$_jobId'),
            initialValue: history.any((h) => h['id'] == _jobId) ? _jobId : null,
            isExpanded: true,
            decoration: InputDecoration(labelText: w('history')),
            items: [
              for (final h in history)
                DropdownMenuItem(
                  value: h['id'] as String,
                  child: Text(
                    '${h['created_at']} · ${w(h['kind'] == 'generate' ? 'generate' : h['mode'] as String)}',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
            ],
            onChanged: _busy
                ? null
                : (id) {
                    setState(() {
                      _jobId = id;
                      _country = null;
                      _resetPage();
                    });
                    unawaited(_command('get'));
                  },
          ),
        const SizedBox(height: 16),
        DropdownButtonFormField<String>(
          key: ValueKey('country-$_country-$_jobId'),
          initialValue: countries.contains(_country) ? _country : null,
          isExpanded: true,
          decoration: InputDecoration(
            labelText: 'Cloudflare · ${w('country')}',
          ),
          items: [
            DropdownMenuItem<String>(
              value: null,
              child: Text(
                w('all'),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ),
            for (final country in countries)
              DropdownMenuItem(value: country, child: Text(country)),
          ],
          onChanged: _busy
              ? null
              : (country) {
                  setState(() {
                    _country = country;
                    _resetPage();
                  });
                  unawaited(_command('get'));
                },
        ),
        RadioGroup<ChainEndpoint>(
          groupValue: selected,
          onChanged: (value) {
            if (widget.endpoint != null && value != null) {
              widget.onValid(true);
              widget.onEndpoint(value);
            }
          },
          child: Column(
            children: [
              for (final row in rows)
                Builder(
                  builder: (context) {
                    final endpoint = ChainEndpoint.fromMap(
                      row['endpoint'] as Map<Object?, Object?>,
                    );
                    final descriptions = <String>[];
                    for (final family in ['ipv4', 'ipv6']) {
                      final observation = row[family] as Map<Object?, Object?>?;
                      if (observation != null) {
                        descriptions.add(
                          '${family.toUpperCase()} · ${observation['country'] ?? w('unknown')} · ${observation['exit_ip'] ?? '—'} · ${observation['colo'] ?? '—'} · ${observation['response_ms'] ?? '—'} ms',
                        );
                      }
                    }
                    return RadioListTile<ChainEndpoint>(
                      value: endpoint,
                      enabled: widget.endpoint != null,
                      selected: selected == endpoint,
                      contentPadding: EdgeInsets.zero,
                      title: Text(endpoint.label),
                      subtitle: Text(
                        '${descriptions.join('\n')}\n${w('observed')}: ${row['checked_at']}',
                      ),
                    );
                  },
                ),
            ],
          ),
        ),
        Wrap(
          spacing: 8,
          children: [
            if (_previous.isNotEmpty)
              TextButton(
                onPressed: _busy
                    ? null
                    : () {
                        setState(() => _cursor = _previous.removeLast());
                        unawaited(_command('get'));
                      },
                child: const Icon(LucideIcons.chevronLeft),
              ),
            if (_response['next_cursor'] case final int next)
              TextButton(
                onPressed: _busy
                    ? null
                    : () {
                        setState(() {
                          _previous.add(_cursor);
                          _cursor = next;
                        });
                        unawaited(_command('get'));
                      },
                child: Text(w('more')),
              ),
          ],
        ),
      ],
    );
  }
}
