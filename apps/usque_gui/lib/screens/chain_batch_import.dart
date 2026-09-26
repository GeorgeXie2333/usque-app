part of 'chain_proxy_screen.dart';

class _BatchItem {
  _BatchItem(this.file);
  final ChainConfigurationFile file;
  final name = TextEditingController();
  final username = TextEditingController();
  final password = TextEditingController();
  final keyPassword = TextEditingController();
  ChainProfileSummary? preview;
  String? error;
  bool checked = false, saved = false, uncertain = false;

  bool get complete =>
      name.text.trim().isNotEmpty &&
      name.text.trim().runes.length <= 64 &&
      !name.text.contains(RegExp(r'[\x00-\x1f\x7f-\x9f]')) &&
      (preview?.requiresAuth != true ||
          username.text.isNotEmpty && password.text.isNotEmpty) &&
      (preview?.requiresKeyPassword != true || keyPassword.text.isNotEmpty);
  bool get ready =>
      preview != null && complete && error == null && !saved && !uncertain;

  void dispose() {
    for (final field in [name, username, password, keyPassword]) {
      field.clear();
      field.dispose();
    }
  }
}

class _BatchImportDialog extends StatefulWidget {
  const _BatchImportDialog({
    required this.controller,
    required this.source,
    required this.files,
  });
  final AppController controller;
  final ChainSource source;
  final List<ChainConfigurationFile> files;
  @override
  State<_BatchImportDialog> createState() => _BatchImportDialogState();
}

class _BatchImportDialogState extends State<_BatchImportDialog> {
  late final _items = widget.files.map(_BatchItem.new).toList();
  bool _checking = true, _saving = false, _interrupted = false;
  int _checked = 0;
  String _text(String key) => widget.controller.strings.chain(key);

  @override
  void initState() {
    super.initState();
    unawaited(_check());
  }

  @override
  void dispose() {
    for (final item in _items) {
      item.dispose();
    }
    super.dispose();
  }

  Future<void> _check() async {
    for (final item in _items) {
      if (!mounted) return;
      final file = item.file;
      final content = file.configuration;
      String? error = switch (file.errorCode) {
        'CHAIN_FILE_TOO_LARGE' => 'invalid_size_or_encoding',
        'CHAIN_FILE_ENCODING_INVALID' => 'file_encoding_invalid',
        null => null,
        _ => 'file_read_failed',
      };
      if (error == null) {
        if (content == null || content.isEmpty) {
          error = 'file_read_failed';
        } else if (utf8.encode(content).length > 128 * 1024) {
          error = 'invalid_size_or_encoding';
        } else {
          error = _chainSourceMismatch(widget.source, content);
        }
      }
      if (error != null) {
        item.error = _text(error);
      } else {
        try {
          final result = await widget.controller.chainProfile({
            'action': 'preview',
            'source': widget.source.wire,
            'name': widget.source.label,
            'configuration': content,
          });
          if (!mounted) return;
          if (result.error case final error?) {
            item.error = widget.controller.strings.chainError(error);
          } else if (result.preview case final preview?) {
            if (preview.candidates.length > 1 &&
                !(widget
                        .controller
                        .engineCapabilities
                        ?.chainOpenvpnMultiEndpoint ??
                    false)) {
              item.error = _text('multi_endpoint_unavailable');
            } else {
              item.preview = preview;
              item.name.text = preview.host;
            }
          } else {
            item.error = _text('invalid_configuration');
          }
        } catch (_) {
          if (!mounted) return;
          item.error = _text('secure_storage_failed');
        }
      }
      if (!mounted) return;
      setState(() {
        item.checked = true;
        _checked++;
      });
    }
    if (mounted) setState(() => _checking = false);
  }

  Future<void> _save() async {
    if (_checking || _saving || _interrupted) return;
    final ready = _items.where((item) => item.ready).toList();
    if (ready.isEmpty) return;
    setState(() => _saving = true);
    for (final item in ready) {
      try {
        final result = await widget.controller.chainProfile({
          'action': 'import',
          'source': widget.source.wire,
          'name': item.name.text.trim(),
          'configuration': item.file.configuration,
          'username': item.username.text,
          'password': item.password.text,
          'private_key_password': item.keyPassword.text,
        });
        if (!mounted) return;
        if (result.error case final error?) {
          setState(
            () => item.error = widget.controller.strings.chainError(error),
          );
        } else if (result.preview != null) {
          setState(() {
            item.saved = true;
            item.error = null;
            item.password.clear();
            item.keyPassword.clear();
          });
        } else {
          // A missing acknowledgement cannot establish whether storage succeeded.
          throw const EngineException(
            'CHAIN_IMPORT_UNCERTAIN',
            'Missing import result.',
          );
        }
      } catch (_) {
        if (!mounted) return;
        setState(() {
          item.uncertain = true;
          item.error = _text('batch_uncertain');
          _interrupted = true;
        });
        // Never retry an uncertain write. Refresh before asking the user to
        // inspect the library; closing the dialog reloads the visible list too.
        try {
          await widget.controller.chainProfile({'action': 'list'});
        } catch (_) {
          // Keep the uncertainty visible even when the engine remains offline.
        }
        break;
      }
    }
    if (mounted) setState(() => _saving = false);
  }

  Widget _field(
    _BatchItem item,
    TextEditingController controller,
    String key, {
    bool secret = false,
  }) => Padding(
    padding: const EdgeInsets.only(top: 16),
    child: TextField(
      controller: controller,
      enabled: !_saving && !item.saved && !_interrupted,
      obscureText: secret,
      autocorrect: false,
      enableSuggestions: false,
      maxLength: key == 'name' ? 64 : null,
      decoration: InputDecoration(labelText: _text(key)),
      onChanged: (_) => setState(() => item.error = null),
    ),
  );

  @override
  Widget build(BuildContext context) {
    final ready = _items.where((item) => item.ready).length;
    final pending = _items
        .where(
          (item) =>
              item.preview != null &&
              !item.complete &&
              !item.saved &&
              !item.uncertain,
        )
        .length;
    final failed = _items.where((item) => item.error != null).length;
    final saved = _items.where((item) => item.saved).length;
    return PopScope(
      canPop: !_saving,
      child: UsqueDialog(
        icon: LucideIcons.files,
        title: _text('batch_title'),
        width: 640,
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              _text('batch_counts')
                  .replaceAll('{ready}', '$ready')
                  .replaceAll('{pending}', '$pending')
                  .replaceAll('{failed}', '$failed')
                  .replaceAll('{saved}', '$saved'),
            ),
            if (_checking || _saving) ...[
              const SizedBox(height: 12),
              LinearProgressIndicator(
                value: _checking ? _checked / _items.length : null,
              ),
              Text(
                _checking
                    ? _text('batch_checking')
                          .replaceAll('{done}', '$_checked')
                          .replaceAll('{total}', '${_items.length}')
                    : _text('batch_saving'),
              ),
            ],
            if (_interrupted)
              Padding(
                padding: const EdgeInsets.only(top: 12),
                child: WarningBanner(
                  title: widget.controller.strings.get('error'),
                  message: _text('batch_uncertain'),
                  danger: true,
                ),
              ),
            const SizedBox(height: 12),
            for (final (index, item) in _items.indexed)
              ExpansionTile(
                key: ValueKey('chain-batch-item-$index'),
                tilePadding: EdgeInsets.zero,
                title: Text(
                  item.file.name.isEmpty
                      ? '${widget.source.label} ${index + 1}'
                      : item.file.name,
                ),
                subtitle: Text(
                  item.error ??
                      _text(
                        item.saved
                            ? 'batch_saved'
                            : !item.checked
                            ? 'checking'
                            : item.ready
                            ? 'batch_ready'
                            : 'batch_pending',
                      ),
                ),
                childrenPadding: const EdgeInsets.only(bottom: 16),
                expandedCrossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (item.preview case final preview?) ...[
                    _field(item, item.name, 'name'),
                    const SizedBox(height: 16),
                    _ProfileDetails(
                      profile: preview,
                      controller: widget.controller,
                    ),
                    if (preview.requiresAuth) ...[
                      _field(item, item.username, 'username'),
                      _field(item, item.password, 'password', secret: true),
                    ],
                    if (preview.requiresKeyPassword)
                      _field(
                        item,
                        item.keyPassword,
                        'key_password',
                        secret: true,
                      ),
                  ],
                ],
              ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: _saving ? null : () => Navigator.pop(context),
            child: Text(
              _text(saved > 0 || _interrupted ? 'batch_close' : 'cancel'),
            ),
          ),
          FilledButton(
            key: const ValueKey('chain-batch-save'),
            onPressed: _checking || _saving || _interrupted || ready == 0
                ? null
                : () => unawaited(_save()),
            child: Text(_text('batch_import').replaceAll('{count}', '$ready')),
          ),
        ],
      ),
    );
  }
}
