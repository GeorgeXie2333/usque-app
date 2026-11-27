import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../models/endpoint_subscriptions.dart';
import '../state/app_controller.dart';
import 'common.dart';
import 'tv_text_input.dart';
import 'usque_dialog.dart';

/// Opens the Add or Edit Subscription modal dialog and saves through Engine/Android.
Future<EndpointSubscription?> showEndpointSubscriptionDialog(
  BuildContext context, {
  required AppController controller,
  EndpointSubscription? existing,
}) {
  return showDialog<EndpointSubscription?>(
    context: context,
    barrierDismissible: false,
    builder: (_) =>
        _EndpointSubscriptionDialog(controller: controller, existing: existing),
  );
}

class _EndpointSubscriptionDialog extends StatefulWidget {
  const _EndpointSubscriptionDialog({required this.controller, this.existing});

  final AppController controller;
  final EndpointSubscription? existing;

  @override
  State<_EndpointSubscriptionDialog> createState() =>
      _EndpointSubscriptionDialogState();
}

class _EndpointSubscriptionDialogState
    extends State<_EndpointSubscriptionDialog> {
  final _formKey = GlobalKey<FormState>();
  late final TextEditingController _name;
  late final TextEditingController _url;
  late EndpointSubscriptionTier _tier;
  late SubscriptionRefreshInterval _autoRefresh;
  bool _saving = false;
  String? _errorText;

  @override
  void initState() {
    super.initState();
    final existing = widget.existing;
    _name = TextEditingController(text: existing?.name ?? '');
    _url = TextEditingController(text: existing?.url ?? '');
    _tier =
        existing?.accountTier ??
        widget.controller.activeAccountSubscriptionTier ??
        EndpointSubscriptionTier.free;
    _autoRefresh = existing?.autoRefresh ?? SubscriptionRefreshInterval.off;
  }

  @override
  void dispose() {
    _name.dispose();
    _url.dispose();
    super.dispose();
  }

  String? _validateName(String? value) {
    final trimmed = value?.trim() ?? '';
    final count = trimmed.runes.length;
    if (count < 1 || count > 64) {
      return widget.controller.strings.get('sub_err_invalid_name');
    }
    return null;
  }

  String? _validateUrl(String? value) {
    final trimmed = value?.trim() ?? '';
    if (trimmed.isEmpty ||
        !trimmed.startsWith('https://') ||
        utf8.encode(trimmed).length > 2048 ||
        trimmed.contains(RegExp(r'\s'))) {
      return widget.controller.strings.get('sub_err_invalid_url');
    }
    final uri = Uri.tryParse(trimmed);
    if (uri == null ||
        uri.scheme != 'https' ||
        uri.host.isEmpty ||
        uri.userInfo.isNotEmpty) {
      return widget.controller.strings.get('sub_err_invalid_url');
    }
    return null;
  }

  Future<void> _submit() async {
    if (_saving || !(_formKey.currentState?.validate() ?? false)) return;
    FocusScope.of(context).unfocus();
    setState(() {
      _saving = true;
      _errorText = null;
    });
    final strings = widget.controller.strings;
    final subs = widget.controller.endpointSubscriptions;
    final existing = widget.existing;
    if (existing == null) {
      final result = await subs.addSubscription(
        name: _name.text.trim(),
        url: _url.text.trim(),
        accountTier: _tier,
        autoRefresh: _autoRefresh,
      );
      if (!mounted) return;
      if (result.subscription != null && result.errorCode == null) {
        Navigator.of(context).pop(result.subscription);
        return;
      }
      setState(() {
        _saving = false;
        _errorText = strings.subscriptionErrorLabel(
          result.errorCode ?? 'SUBSCRIPTION_UNAVAILABLE',
        );
      });
    } else {
      final result = await subs.updateSubscription(
        id: existing.id,
        name: _name.text.trim(),
        url: _url.text.trim(),
        accountTier: _tier,
        autoRefresh: _autoRefresh,
        expectedRevision: existing.revision,
      );
      if (!mounted) return;
      if (result.ok) {
        Navigator.of(context).pop(subs.catalog?.byId(existing.id));
        return;
      }
      setState(() {
        _saving = false;
        _errorText = strings.subscriptionErrorLabel(
          result.errorCode ?? 'SUBSCRIPTION_UNAVAILABLE',
        );
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final strings = widget.controller.strings;
    final isEdit = widget.existing != null;
    return UsqueDialog(
      icon: isEdit ? LucideIcons.pencil : LucideIcons.listPlus,
      title: strings.get(isEdit ? 'sub_edit' : 'sub_add'),
      width: 520,
      content: Form(
        key: _formKey,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (_errorText case final error?) ...[
              WarningBanner(title: null, message: error, danger: true),
              const SizedBox(height: 12),
            ],
            tvTextFormField(
              key: const ValueKey('subscription-dialog-name'),
              controller: _name,
              enabled: !_saving,
              autofocus: true,
              textInputAction: TextInputAction.next,
              decoration: InputDecoration(labelText: strings.get('sub_name')),
              validator: _validateName,
            ),
            const SizedBox(height: 14),
            tvTextFormField(
              key: const ValueKey('subscription-dialog-url'),
              controller: _url,
              enabled: !_saving,
              // URLs read left to right in every locale, including RTL.
              textDirection: TextDirection.ltr,
              keyboardType: TextInputType.url,
              textInputAction: TextInputAction.done,
              decoration: InputDecoration(
                labelText: strings.get('sub_url'),
                helperText: strings.get('sub_url_hint'),
                helperMaxLines: 2,
              ),
              validator: _validateUrl,
            ),
            const SizedBox(height: 16),
            Text(
              strings.get('sub_account_tier'),
              style: Theme.of(context).textTheme.labelLarge,
            ),
            const SizedBox(height: 8),
            LayoutBuilder(
              builder: (context, constraints) =>
                  SegmentedButton<EndpointSubscriptionTier>(
                    key: const ValueKey('subscription-dialog-tier'),
                    direction:
                        constraints.maxWidth < 400 ||
                            MediaQuery.textScalerOf(context).scale(1) > 1.3
                        ? Axis.vertical
                        : Axis.horizontal,
                    showSelectedIcon: false,
                    segments: [
                      ButtonSegment(
                        value: EndpointSubscriptionTier.free,
                        label: Text(strings.get('sub_tier_free')),
                      ),
                      ButtonSegment(
                        value: EndpointSubscriptionTier.plus,
                        label: Text(strings.get('sub_tier_plus')),
                      ),
                      ButtonSegment(
                        value: EndpointSubscriptionTier.zeroTrust,
                        label: Text(strings.get('sub_tier_zt')),
                      ),
                    ],
                    selected: {_tier},
                    onSelectionChanged: _saving
                        ? null
                        : (selection) =>
                              setState(() => _tier = selection.first),
                  ),
            ),
            const SizedBox(height: 16),
            DropdownButtonFormField<SubscriptionRefreshInterval>(
              key: const ValueKey('subscription-dialog-refresh'),
              initialValue: _autoRefresh,
              isExpanded: true,
              style: FieldDropdown.valueStyle(context),
              iconSize: FieldDropdown.iconSize,
              decoration: FieldDropdown.decoration(
                context,
                labelText: strings.get('sub_auto_refresh'),
              ),
              items: [
                DropdownMenuItem(
                  value: SubscriptionRefreshInterval.off,
                  child: Text(strings.get('sub_refresh_off')),
                ),
                DropdownMenuItem(
                  value: SubscriptionRefreshInterval.daily,
                  child: Text(strings.get('sub_refresh_daily')),
                ),
                DropdownMenuItem(
                  value: SubscriptionRefreshInterval.weekly,
                  child: Text(strings.get('sub_refresh_weekly')),
                ),
              ],
              onChanged: _saving
                  ? null
                  : (value) {
                      if (value != null) {
                        setState(() => _autoRefresh = value);
                      }
                    },
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: _saving ? null : () => Navigator.of(context).pop(),
          child: Text(strings.get('cancel')),
        ),
        FilledButton(
          key: const ValueKey('subscription-dialog-submit'),
          onPressed: _saving ? null : _submit,
          child: Text(
            strings.get(
              _saving
                  ? 'saving'
                  : isEdit
                  ? 'save_changes'
                  : 'sub_add',
            ),
          ),
        ),
      ],
    );
  }
}
