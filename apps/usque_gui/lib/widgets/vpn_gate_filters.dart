import 'package:flutter/material.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import 'country_flag.dart';

class VpnGateFilters extends StatelessWidget {
  const VpnGateFilters({
    required this.strings,
    required this.favoritesOnly,
    required this.favoriteCount,
    required this.country,
    required this.countries,
    required this.onScopeChanged,
    required this.onCountryChanged,
    super.key,
  });

  final AppStrings strings;
  final bool favoritesOnly;
  final int favoriteCount;
  final String country;
  final List<VpnGateCountry> countries;
  final ValueChanged<bool> onScopeChanged;
  final ValueChanged<String>? onCountryChanged;

  @override
  Widget build(BuildContext context) {
    final scopes = Wrap(
      key: const ValueKey('vpn-gate-scopes'),
      spacing: 8,
      runSpacing: 8,
      children: [
        for (final favorites in [false, true])
          ChoiceChip(
            key: ValueKey(favorites ? 'vpn-gate-favorites' : 'vpn-gate-pool'),
            label: Text(
              favorites
                  ? '${strings.get('gate_favorites')} ($favoriteCount)'
                  : strings.get('gate_pool'),
            ),
            selected: favoritesOnly == favorites,
            onSelected: (_) => onScopeChanged(favorites),
          ),
      ],
    );
    final field = Column(
      key: const ValueKey('vpn-gate-country-field'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ExcludeSemantics(
          child: Text(
            strings.get('gate_country'),
            style: Theme.of(context).textTheme.labelMedium?.copyWith(
              color: Theme.of(context).colorScheme.onSurfaceVariant,
            ),
          ),
        ),
        const SizedBox(height: 8),
        Semantics(
          label: strings.get('gate_country'),
          child: DropdownButtonFormField<String>(
            key: ValueKey('vpn-gate-country-$country'),
            initialValue: country,
            isExpanded: true,
            items: [
              DropdownMenuItem(
                value: 'ALL',
                child: _countryLabel(null, strings.get('gate_all_countries')),
              ),
              for (final item in countries)
                DropdownMenuItem(
                  value: item.code ?? 'UNKNOWN',
                  child: _countryLabel(
                    item.code,
                    '${item.code == null ? strings.get('gate_unknown_country') : item.name ?? item.code} (${item.count})',
                  ),
                ),
              if (country != 'ALL' &&
                  !countries.any((item) => (item.code ?? 'UNKNOWN') == country))
                DropdownMenuItem(
                  value: country,
                  child: _countryLabel(country, country),
                ),
            ],
            onChanged: onCountryChanged == null
                ? null
                : (value) {
                    if (value != null) onCountryChanged!(value);
                  },
          ),
        ),
      ],
    );
    return Padding(
      padding: const EdgeInsets.only(bottom: 16),
      child: LayoutBuilder(
        builder: (context, constraints) {
          // These constraints exclude the shell's rail; window width alone can
          // leave the field squeezed when the extended navigation is visible.
          if (constraints.maxWidth < 640 ||
              MediaQuery.textScalerOf(context).scale(14) > 21) {
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [scopes, const SizedBox(height: 16), field],
            );
          }
          return Row(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              Expanded(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(minHeight: 56),
                  child: Align(
                    alignment: AlignmentDirectional.centerStart,
                    child: scopes,
                  ),
                ),
              ),
              const SizedBox(width: 24),
              Expanded(child: field),
            ],
          );
        },
      ),
    );
  }

  Widget _countryLabel(String? code, String label) => Row(
    children: [
      CountryFlag(countryCode: code),
      const SizedBox(width: 8),
      Expanded(child: Text(label, overflow: TextOverflow.ellipsis)),
    ],
  );
}
