import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import 'common.dart';
import 'usque_dialog.dart';

/// Shared presentation only: callers own drafts, validation and persistence.
class EndpointFields extends StatelessWidget {
  const EndpointFields({
    required this.strings,
    required this.selection,
    required this.presetMode,
    required this.presetRegion,
    required this.saving,
    required this.automaticAvailable,
    required this.managed,
    required this.presetSupported,
    required this.presetVerified,
    required this.ztEditingSupported,
    required this.ztRegistered,
    required this.ztUnlocked,
    required this.onSelection,
    required this.onPresetMode,
    required this.onRegion,
    required this.onUnlock,
    required this.fields,
    required this.ipPolicyField,
    super.key,
  });
  final AppStrings strings;
  final EndpointSelection selection;
  final EndpointPresetRegion presetRegion;
  final bool presetMode,
      saving,
      automaticAvailable,
      managed,
      presetSupported,
      presetVerified,
      ztEditingSupported,
      ztRegistered,
      ztUnlocked;
  final ValueChanged<EndpointSelection> onSelection;
  final ValueChanged<bool> onPresetMode;
  final ValueChanged<EndpointPresetRegion> onRegion;
  final VoidCallback onUnlock;
  final Widget fields, ipPolicyField;
  bool get usingPreset =>
      presetMode && (managed || selection == EndpointSelection.custom);
  @override
  Widget build(BuildContext context) {
    final custom = managed || selection == EndpointSelection.custom;
    return FormGroup(
      title: strings.get('endpoint_section'),
      children: <Widget>[
        if (!managed) ...<Widget>[
          LayoutBuilder(
            // Two short choices fit side by side on a phone; only very
            // narrow layouts and large text stack them.
            builder: (context, constraints) => Semantics(
              container: true,
              label: strings.get('endpoint_selection'),
              child: SegmentedButton<EndpointSelection>(
                key: const ValueKey('endpoint-selection'),
                direction:
                    constraints.maxWidth < 300 ||
                        MediaQuery.textScalerOf(context).scale(1) > 1.3
                    ? Axis.vertical
                    : Axis.horizontal,
                segments: [
                  ButtonSegment(
                    value: EndpointSelection.automatic,
                    label: Text(strings.get('endpoint_automatic')),
                    enabled: automaticAvailable,
                  ),
                  ButtonSegment(
                    value: EndpointSelection.custom,
                    label: Text(strings.get('endpoint_custom')),
                  ),
                ],
                selected: {selection},
                onSelectionChanged: saving
                    ? null
                    : (values) => onSelection(values.first),
                showSelectedIcon: false,
              ),
            ),
          ),
          if (!usingPreset) ...[
            const SizedBox(height: 8),
            HintText(
              automaticAvailable
                  ? strings.get(
                      selection == EndpointSelection.automatic
                          ? 'endpoint_automatic_help'
                          : 'endpoint_custom_help',
                    )
                  : strings.get('endpoint_unsupported'),
            ),
          ],
          const SizedBox(height: 16),
        ],
        if (custom) ...<Widget>[
          LayoutBuilder(
            builder: (context, constraints) => SegmentedButton<bool>(
              key: const ValueKey('endpoint-custom-source'),
              direction:
                  constraints.maxWidth < 300 ||
                      MediaQuery.textScalerOf(context).scale(1) > 1.3
                  ? Axis.vertical
                  : Axis.horizontal,
              segments: [
                ButtonSegment(
                  value: false,
                  label: Text(strings.get('endpoint_manual')),
                ),
                ButtonSegment(
                  value: true,
                  label: Text(strings.get('endpoint_presets')),
                  enabled: presetSupported && presetVerified,
                ),
              ],
              selected: {presetMode},
              onSelectionChanged: saving
                  ? null
                  : (values) => onPresetMode(values.first),
              showSelectedIcon: false,
            ),
          ),
          if (!presetSupported || !presetVerified) ...[
            const SizedBox(height: 8),
            HintText(
              strings.get(
                !presetSupported
                    ? 'endpoint_preset_unsupported'
                    : 'endpoint_preset_verify',
              ),
            ),
          ],
          if (usingPreset) ...[
            const SizedBox(height: 16),
            DropdownButtonFormField<EndpointPresetRegion>(
              key: const ValueKey('endpoint-preset-region'),
              initialValue: presetRegion,
              style: FieldDropdown.valueStyle(context),
              iconSize: FieldDropdown.iconSize,
              isExpanded: true,
              decoration: FieldDropdown.decoration(
                context,
                labelText: strings.get('endpoint_preset_country'),
              ),
              items: [
                for (final region in EndpointPresetRegion.values)
                  DropdownMenuItem(
                    value: region,
                    child: Text(strings.get('endpoint_preset_${region.name}')),
                  ),
              ],
              onChanged: saving
                  ? null
                  : (region) {
                      if (region != null) onRegion(region);
                    },
            ),
            if (presetVerified) ...[
              const SizedBox(height: 8),
              HintText(strings.get('ipv6_only')),
            ],
            const SizedBox(height: 16),
          ],
          if (!usingPreset) const SizedBox(height: 16),
        ],
        if (managed && !usingPreset && !ztUnlocked) ...<Widget>[
          if (ztEditingSupported && ztRegistered)
            Align(
              alignment: AlignmentDirectional.centerStart,
              child: OutlinedButton.icon(
                key: const ValueKey('zt-endpoint-edit'),
                onPressed: saving ? null : onUnlock,
                icon: const Icon(LucideIcons.pencil),
                label: Text(strings.get('zero_trust_endpoint_edit')),
              ),
            ),
          const SizedBox(height: 8),
          HintText(
            strings.get(
              !ztEditingSupported
                  ? 'zero_trust_endpoint_unsupported'
                  : !ztRegistered
                  ? 'zero_trust_metadata_missing'
                  : 'zero_trust_endpoint_risk_locked',
            ),
          ),
          const SizedBox(height: 16),
        ],
        fields,
        const SizedBox(height: 12),
        ipPolicyField,
      ],
    );
  }
}

class ResponsiveEndpointFields extends StatelessWidget {
  const ResponsiveEndpointFields({required this.children, super.key});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final width = constraints.maxWidth >= 620
            ? (constraints.maxWidth - 12) / 2
            : constraints.maxWidth;
        return Wrap(
          spacing: 12,
          runSpacing: 12,
          children: children
              .map((child) => SizedBox(width: width, child: child))
              .toList(growable: false),
        );
      },
    );
  }
}

Future<IpPolicy?> chooseEndpointPresetPolicy(
  BuildContext context,
  AppStrings strings,
) async {
  var selected = IpPolicy.ipv6Only;
  return showDialog<IpPolicy>(
    context: context,
    builder: (context) => StatefulBuilder(
      builder: (context, update) => UsqueDialog(
        icon: LucideIcons.globe,
        title: strings.get('endpoint_preset_policy'),
        content: RadioGroup<IpPolicy>(
          groupValue: selected,
          onChanged: (value) {
            if (value != null) update(() => selected = value);
          },
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              RadioListTile<IpPolicy>(
                value: IpPolicy.ipv6Only,
                title: Text(strings.get('ipv6_only')),
                subtitle: Text(strings.get('endpoint_preset_only_hint')),
                contentPadding: EdgeInsets.zero,
                autofocus: true,
              ),
              RadioListTile<IpPolicy>(
                value: IpPolicy.preferIpv6,
                title: Text(strings.get('prefer_ipv6')),
                subtitle: Text(strings.get('endpoint_preset_prefer_hint')),
                contentPadding: EdgeInsets.zero,
              ),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: Text(strings.get('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, selected),
            child: Text(strings.get('continue')),
          ),
        ],
      ),
    ),
  );
}

String? validateEndpointSni(AppStrings strings, String? value) {
  final normalized = value?.trim() ?? '';
  final valid = RegExp(
    r'^(?=.{1,253}$)(?!-)(?:[a-zA-Z0-9-]{1,63}\.)+[a-zA-Z0-9-]{2,63}$',
  ).hasMatch(normalized);
  return valid ? null : strings.get('invalid_dns_name');
}
