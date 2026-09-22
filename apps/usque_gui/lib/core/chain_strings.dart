import 'app_strings.dart';
import 'l10n/chain.dart';

extension ChainStrings on AppStrings {
  String chain(String key) {
    final table = kChainCatalogs[catalogId] ?? kChainEn;
    return table[key] ?? kChainEn[key] ?? kChainEn['invalid_configuration']!;
  }

  /// Human-readable location for a validation error reported by the engine.
  ///
  /// Field and line come from the validator; configuration values are never
  /// reproduced. Missing parts are omitted rather than shown as blanks.
  String chainError(Map<String, Object?> error) {
    final message = chain(
      error['reason'] as String? ?? 'invalid_configuration',
    );
    final field = switch (error['field']) {
      final String value when value.trim().isNotEmpty => value.trim(),
      _ => null,
    };
    final line = switch (error['line']) {
      final int value when value > 0 => value,
      _ => null,
    };
    if (field == null) return message;
    return chain(line == null ? 'error_field' : 'error_location')
        .replaceAll('{message}', message)
        .replaceAll('{field}', field)
        .replaceAll('{line}', '${line ?? ''}');
  }
}
