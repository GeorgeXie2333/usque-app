import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/country_flags.dart';
import '../core/usque_theme.dart';

/// A decorative, offline flag. The adjacent text supplies its accessible name.
class CountryFlag extends StatelessWidget {
  const CountryFlag({
    required this.countryCode,
    this.size,
    this.enabled = true,
    super.key,
  });

  final String? countryCode;
  final Size? size;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    final code = countryCode?.trim().toLowerCase();
    final dimensions =
        size ??
        (MediaQuery.maybeNavigationModeOf(context) == NavigationMode.directional
            ? const Size(32, 24)
            : const Size(24, 18));
    Widget fallback() => Icon(
      LucideIcons.globe,
      size: dimensions.shortestSide,
      color: Theme.of(context).colorScheme.onSurfaceVariant,
    );
    return ExcludeSemantics(
      child: SizedBox.fromSize(
        size: dimensions,
        child: Opacity(
          opacity: enabled ? 1 : 0.38,
          child: kCountryFlagCodes.contains(code)
              ? Container(
                  foregroundDecoration: BoxDecoration(
                    border: Border.all(color: UsqueTokens.of(context).hairline),
                  ),
                  child: Image.asset(
                    'assets/flags/w80/$code.png',
                    fit: BoxFit.contain,
                    filterQuality: FilterQuality.medium,
                    excludeFromSemantics: true,
                    errorBuilder: (context, error, stackTrace) => fallback(),
                  ),
                )
              : fallback(),
        ),
      ),
    );
  }
}
