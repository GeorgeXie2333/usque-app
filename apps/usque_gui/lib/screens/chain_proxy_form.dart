part of 'chain_proxy_screen.dart';

class _ProxyDialog extends StatefulWidget {
  const _ProxyDialog({required this.controller, required this.source});
  final AppController controller;
  final ChainSource source;
  @override
  State<_ProxyDialog> createState() => _ProxyDialogState();
}

class _ProxyDialogState extends State<_ProxyDialog> {
  final _name = TextEditingController(),
      _host = TextEditingController(),
      _username = TextEditingController(),
      _password = TextEditingController(),
      _dns = TextEditingController();
  late final _port = TextEditingController(
    text: widget.source == ChainSource.httpProxy ? '8080' : '1080',
  );
  bool _auth = false, _showPassword = false, _busy = false;
  String? _error;
  bool get _canEncrypt =>
      widget.controller.engineCapabilities?.chainProxyEncryptedDns ?? false;
  late String _dnsTransport = _canEncrypt ? 'auto' : 'tcp';

  @override
  void dispose() {
    for (final field in [_name, _host, _port, _username, _password, _dns]) {
      field.clear();
      field.dispose();
    }
    super.dispose();
  }

  Future<void> _save() async {
    if (_busy) return;
    final s = widget.controller.strings;
    final port = int.tryParse(_port.text.trim());
    if (port == null || port < 1 || port > 65535) {
      setState(() => _error = s.chain('invalid_endpoint'));
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final result = await widget.controller.chainProfile({
        'action': 'import',
        'source': widget.source.wire,
        'name': _name.text.trim().isEmpty
            ? _host.text.trim()
            : _name.text.trim(),
        'proxy': {
          'host': _host.text.trim(),
          'port': port,
          'auth_mode': _auth ? 'username_password' : 'none',
          'dns_transport': _dnsTransport,
          'dns_servers': (_dnsTransport == 'doh' ? '' : _dns.text)
              .split(RegExp(r'[\s,]+'))
              .where((v) => v.isNotEmpty)
              .toList(),
        },
        'username': _auth ? _username.text : '',
        'password': _auth ? _password.text : '',
      });
      if (!mounted) return;
      if (result.error != null) {
        setState(() => _error = s.chainError(result.error!));
      } else {
        Navigator.pop(context, true);
      }
    } catch (_) {
      if (mounted) setState(() => _error = s.chain('secure_storage_failed'));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final s = widget.controller.strings;
    return PopScope(
      canPop: !_busy,
      child: UsqueDialog(
        icon: LucideIcons.network,
        title: s.chain('add_proxy'),
        subtitle: widget.source.label,
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          spacing: 12,
          children: [
            Text(s.chain('proxy_hint')),
            TextField(
              key: const ValueKey('chain-proxy-name'),
              controller: _name,
              enabled: !_busy,
              maxLength: 64,
              decoration: InputDecoration(labelText: s.chain('name')),
            ),
            TextField(
              key: const ValueKey('chain-proxy-host'),
              controller: _host,
              enabled: !_busy,
              autocorrect: false,
              enableSuggestions: false,
              decoration: InputDecoration(labelText: s.chain('endpoint')),
            ),
            TextField(
              key: const ValueKey('chain-proxy-port'),
              controller: _port,
              enabled: !_busy,
              keyboardType: TextInputType.number,
              decoration: InputDecoration(labelText: s.get('port')),
            ),
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              title: Text('${s.chain('username')} / ${s.chain('password')}'),
              value: _auth,
              onChanged: _busy
                  ? null
                  : (value) => setState(() => _auth = value),
            ),
            if (_auth) ...[
              TextField(
                key: const ValueKey('chain-proxy-username'),
                controller: _username,
                enabled: !_busy,
                autocorrect: false,
                enableSuggestions: false,
                decoration: InputDecoration(labelText: s.chain('username')),
              ),
              TextField(
                key: const ValueKey('chain-proxy-password'),
                controller: _password,
                enabled: !_busy,
                obscureText: !_showPassword,
                autocorrect: false,
                enableSuggestions: false,
                decoration: InputDecoration(
                  labelText: s.chain('password'),
                  suffixIcon: IconButton(
                    tooltip: s.chain(
                      _showPassword ? 'hide_password' : 'show_password',
                    ),
                    onPressed: () =>
                        setState(() => _showPassword = !_showPassword),
                    icon: Icon(
                      _showPassword ? LucideIcons.eyeOff : LucideIcons.eye,
                    ),
                  ),
                ),
              ),
            ],
            ExpansionTile(
              title: Text(s.chain('dns')),
              children: [
                DropdownButtonFormField<String>(
                  key: const ValueKey('chain-proxy-dns-transport'),
                  initialValue: _dnsTransport,
                  isExpanded: true,
                  items: [
                    for (final mode in [
                      if (_canEncrypt) ...['auto', 'doh'],
                      'tcp',
                    ])
                      DropdownMenuItem(
                        value: mode,
                        child: Text(s.chain('dns_$mode')),
                      ),
                  ],
                  onChanged: _busy
                      ? null
                      : (value) => setState(() => _dnsTransport = value!),
                ),
                Text(
                  s.chain(
                    _dnsTransport == 'tcp' ? 'dns_inherit' : 'dns_auto_hint',
                  ),
                ),
                if (_dnsTransport != 'doh')
                  TextField(
                    key: const ValueKey('chain-proxy-dns'),
                    controller: _dns,
                    enabled: !_busy,
                    maxLines: 3,
                    autocorrect: false,
                    enableSuggestions: false,
                    decoration: const InputDecoration(
                      hintText: '1.1.1.1\n2606:4700:4700::1111',
                    ),
                  ),
              ],
            ),
            if (_error != null)
              Text(
                _error!,
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            if (_busy) const LinearProgressIndicator(),
          ],
        ),
        actions: [
          TextButton(
            onPressed: _busy ? null : () => Navigator.pop(context),
            child: Text(s.chain('cancel')),
          ),
          FilledButton(
            key: const ValueKey('chain-proxy-save'),
            onPressed: _busy ? null : _save,
            child: Text(s.chain('save_import')),
          ),
        ],
      ),
    );
  }
}
