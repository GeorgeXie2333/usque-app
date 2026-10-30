import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/chain_strings.dart';
import 'package:usque/core/l10n/catalogs.dart';
import 'package:usque/core/l10n/ux_refinements.dart';
import 'package:usque/models/app_models.dart';

void main() {
  // Keys the status, routing, quality and onboarding refinements read through
  // AppStrings.get; a missing lookup would render the raw key.
  const keys = <String>[
    'ks_standby',
    'home_exit_unknown',
    'proxy_device_outputs',
    'proxy_exit_group',
    'proxy_listeners_group',
    'routing_action_direct',
    'routing_action_reject',
    'routing_action_proxy',
    'nq_advanced_metrics',
    'nq_attention_count',
    'welcome_body',
    'diag_start',
    'diagnostics',
    'license',
    'routing_overlap',
    'diag_refresh_timeline',
    'technical_details',
    'diag_conclusion_issues',
    'diag_conclusion_clear',
    'nq_attention_hint',
  ];

  test('refinement strings resolve in every catalog', () {
    expect(kUxRefinementCatalogs.keys.toSet(), kCatalogs.keys.toSet());
    for (final preference in LocalePreference.values) {
      final strings = AppStrings(preference);
      for (final key in keys) {
        final value = strings.get(key);
        expect(value, isNot(key), reason: '${strings.catalogId}.$key');
        expect(value.trim(), isNotEmpty, reason: '${strings.catalogId}.$key');
      }
      expect(strings.chain('rename'), isNot('rename'));
      for (final counted in ['nq_attention_count', 'diag_conclusion_issues']) {
        expect(
          strings.get(counted),
          contains('{count}'),
          reason: '${strings.catalogId}.$counted',
        );
      }
    }
  });
}
