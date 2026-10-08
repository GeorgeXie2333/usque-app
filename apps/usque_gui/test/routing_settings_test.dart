import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';

import 'audit_forms_test.dart' show decode;

void main() {
  test('normalization, duplicate removal and conflicting actions', () {
    final first = RoutingRule.create('Example.COM.', RoutingAction.direct);
    final duplicate = RoutingRule.create('example.com', RoutingAction.direct);
    final conflict = RoutingRule.create('example.com', RoutingAction.reject);
    final validation = RoutingSettings(rules: [first, duplicate]).validate();
    expect(validation.settings.rules, [first]);
    expect(validation.notices.single.key, 'routing_duplicate');
    expect(
      () => RoutingSettings(rules: [first, conflict]).validate(),
      throwsA(
        isA<RoutingRuleError>().having((error) => error.ids, 'both rows', [
          first.id,
          conflict.id,
        ]),
      ),
    );
  });

  test('more specific domain and IPv4/IPv6 exceptions are valid', () {
    final rules = [
      RoutingRule.create('example.com', RoutingAction.direct),
      RoutingRule.create('ads.example.com', RoutingAction.reject),
      RoutingRule.create('192.0.2.99/24', RoutingAction.reject),
      RoutingRule.create('192.0.2.7', RoutingAction.direct),
      RoutingRule.create('2001:db8::/32', RoutingAction.reject),
      RoutingRule.create('2001:db8::1', RoutingAction.proxy),
    ];
    final validation = RoutingSettings(rules: rules).validate();
    expect(validation.settings.rules, hasLength(6));
    expect(validation.notices, hasLength(3));
    expect(rules[2].target, '192.0.2.0/24');
    expect(rules[5].target, '2001:db8::1/128');
  });

  test('routing survives JSON, protobuf, shared copies and advanced reset', () {
    final settings = RoutingSettings(
      adsEnabled: true,
      rules: [
        RoutingRule.create('ads.test', RoutingAction.reject),
        RoutingRule.create('2001:db8::1', RoutingAction.proxy),
      ],
    );
    final profile = UsqueProfile.defaultProfile().copyWith(routing: settings);
    expect(UsqueProfile.fromMap(profile.toMap()).routing, settings);
    expect(decode(debugEncodeProfilePayload(profile)).routing, settings);
    expect(profile.resetAdvancedDefaults().routing, settings);
    expect(
      () => RoutingSettings.fromMap({
        'rules': [
          {'id': 'id', 'kind': 'domain', 'target': 'test', 'action': 'unknown'},
        ],
      }),
      throwsArgumentError,
    );
  });
}
