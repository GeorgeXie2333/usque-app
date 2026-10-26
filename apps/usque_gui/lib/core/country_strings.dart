import 'app_strings.dart';
import 'l10n/countries.dart';

extension CountryStrings on AppStrings {
  String countryName(String? code, {String? fallback}) {
    final normalized = code?.trim().toUpperCase();
    final catalog = kCountryCatalogs[catalogId] ?? kCountryCatalogs['en']!;
    return catalog[normalized] ??
        fallback ??
        (normalized?.isNotEmpty == true
            ? normalized!
            : get('gate_unknown_country'));
  }
}
