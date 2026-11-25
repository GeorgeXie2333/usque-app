import 'dart:io';
import 'dart:math';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';

import 'audit_forms_test.dart' show decode;
import 'congestion_control_test.dart' show codec, response;

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

  const current = RoutingLimits(perKind: 4096, targetBytes: 384 * 1024);

  List<RoutingRule> cidrs(int count) => [
    for (var index = 0; index < count; index++)
      RoutingRule(
        id: 'c$index',
        kind: RoutingMatch.cidr,
        target: '10.${index >> 8}.${index & 255}.0/24',
        action: RoutingAction.direct,
      ),
  ];

  List<RoutingRule> domains(int count) => [
    for (var index = 0; index < count; index++)
      RoutingRule(
        id: 'd$index',
        kind: RoutingMatch.domain,
        target: 'h$index.example',
        action: RoutingAction.reject,
      ),
  ];

  test('limits follow the engine and keep the legacy message', () {
    final full = RoutingSettings(rules: [...cidrs(4096), ...domains(4096)]);
    expect(full.validate(limits: current).settings.rules, hasLength(8192));
    expect(
      () => RoutingSettings(
        rules: [
          ...full.rules,
          RoutingRule.create('h.test', RoutingAction.proxy),
        ],
      ).validate(limits: current),
      throwsA(
        isA<RoutingRuleError>().having((e) => e.key, 'key', 'routing_limit'),
      ),
    );
    expect(
      () => RoutingSettings(rules: cidrs(257)).validate(),
      throwsA(
        isA<RoutingRuleError>().having((e) => e.key, 'key', 'bypass_limit'),
      ),
    );
    // The longest valid names (253 bytes) fill the budget, then overflow it.
    String name(int index) {
      final label = 'h${index.toString().padLeft(62, '0')}';
      return '$label.$label.$label.${'a' * 61}';
    }

    List<RoutingRule> long(int count) => [
      for (var index = 0; index < count; index++)
        RoutingRule(
          id: 'l$index',
          kind: RoutingMatch.domain,
          target: name(index),
          action: RoutingAction.reject,
        ),
    ];
    final fits = current.targetBytes! ~/ 253;
    expect(name(0).length, 253);
    expect(
      RoutingSettings(
        rules: long(fits),
      ).validate(limits: current).settings.rules,
      hasLength(fits),
    );
    expect(
      () => RoutingSettings(rules: long(fits + 1)).validate(limits: current),
      throwsA(
        isA<RoutingRuleError>().having((e) => e.key, 'key', 'routing_limit'),
      ),
    );
    expect(current.totalRules, 8192);
    expect(current.pasteChars, 768 * 1024);
    expect(RoutingLimits.legacy.pasteChars, RoutingLimits.legacyPasteChars);
  });

  test('indexed overlap notices match the pairwise definition', () {
    bool overlaps(RoutingRule a, RoutingRule b) {
      if (a.kind != b.kind) return false;
      if (a.kind == RoutingMatch.domain) {
        return a.target.endsWith('.${b.target}') ||
            b.target.endsWith('.${a.target}');
      }
      final left = a.target.split('/');
      final right = b.target.split('/');
      final x = InternetAddress.tryParse(left.first)!.rawAddress;
      final y = InternetAddress.tryParse(right.first)!.rawAddress;
      if (x.length != y.length) return false;
      final bits = min(int.parse(left.last), int.parse(right.last));
      for (var index = 0; index < x.length; index++) {
        final keep = (bits - index * 8).clamp(0, 8);
        final mask = (0xff << (8 - keep)) & 0xff;
        if ((x[index] & mask) != (y[index] & mask)) return false;
      }
      return true;
    }

    final random = Random(169);
    const labels = ['a', 'b', 'ex', 'c'];
    for (var round = 0; round < 200; round++) {
      final rules = <RoutingRule>[];
      final seen = <String>{};
      for (var index = 0; index < 40; index++) {
        final action =
            RoutingAction.values[random.nextInt(RoutingAction.values.length)];
        final String target;
        switch (random.nextInt(3)) {
          case 0:
            target = [
              for (var depth = 0; depth <= random.nextInt(4); depth++)
                labels[random.nextInt(labels.length)],
            ].join('.');
          case 1:
            target =
                '192.0.${random.nextInt(2)}.${random.nextInt(256)}/${random.nextInt(33)}';
          default:
            target =
                '2001:db8:${random.nextInt(3)}::${random.nextInt(65536).toRadixString(16)}/${random.nextInt(129)}';
        }
        final rule = RoutingRule.create(target, action, id: 'r$round-$index');
        if (seen.add('${rule.kind.name}:${rule.target}')) rules.add(rule);
      }
      final expected = <String>[
        for (var later = 0; later < rules.length; later++)
          for (var earlier = 0; earlier < later; earlier++)
            if (rules[earlier].action != rules[later].action &&
                overlaps(rules[earlier], rules[later]))
              '${rules[earlier].id}>${rules[later].id}',
      ]..sort();
      final actual = [
        for (final notice in RoutingSettings(rules: rules).validate().notices)
          if (notice.key == 'routing_overlap') notice.ids.join('>'),
      ]..sort();
      expect(actual, expected, reason: 'round $round');
    }
  });

  test('a full rule set validates quickly', () {
    final settings = RoutingSettings(
      rules: [
        RoutingRule.create('0.0.0.0/0', RoutingAction.reject),
        ...cidrs(4095),
        ...domains(4096),
      ],
    );
    final watch = Stopwatch()..start();
    final validation = settings.validate(limits: current);
    watch.stop();
    expect(validation.settings.rules, hasLength(8192));
    expect(
      validation.notices.where((notice) => notice.key == 'routing_overlap'),
      hasLength(4095),
    );
    // Pairwise comparison of 8192 rules took tens of seconds here.
    expect(watch.elapsed, lessThan(const Duration(seconds: 5)));
  });

  test('capabilities carry the engine limits; older engines stay legacy', () {
    final caps = EngineCapabilities.fromMap({
      'routing_rules': true,
      'max_routing_rules_per_kind': 4096,
      'max_routing_target_bytes': 384 * 1024,
    });
    expect(caps.routingLimits, current);
    expect(
      EngineCapabilities.fromMap({'routing_rules': true}).routingLimits,
      RoutingLimits.legacy,
    );
    for (final invalid in <Object?>[0, -1, 65537, 4096.0, '4096', true, null]) {
      expect(
        EngineCapabilities.fromMap({
          'max_routing_rules_per_kind': invalid,
        }).routingLimits,
        RoutingLimits.legacy,
      );
    }
    expect(
      const EngineCapabilities(
        maxRoutingRulesPerKind: 4096,
        maxRoutingTargetBytes: 0,
      ).routingLimits,
      const RoutingLimits(perKind: 4096),
    );
  });

  test('desktop capabilities 58 and 59 carry the limits', () {
    final caps = codec
        .decodeResponse(
          response(
            15,
            (ControlPayloadWriter()
                  ..unsigned(58, 4096)
                  ..unsigned(59, 384 * 1024))
                .takeBytes(),
          ),
          'cc',
        )
        .capabilities!;
    expect(caps.routingLimits, current);
    expect(
      codec
          .decodeResponse(response(15, Uint8List(0)), 'cc')
          .capabilities!
          .routingLimits,
      RoutingLimits.legacy,
    );
  });
}
