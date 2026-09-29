import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/bypass_targets.dart';

void main() {
  test('mixed rules canonicalize networks, literals and domain syntax', () {
    final rules = BypassTargets.parse(
      '192.0.2.7/24\n192.0.2.0/24\n192.0.2.1\n2001:db8::1\n Example.COM. \nexample.com\nbücher.example\n',
    );
    expect(rules.cidrs, ['192.0.2.0/24', '192.0.2.1/32', '2001:db8::1/128']);
    expect(rules.domains, ['example.com', 'bücher.example']);
    expect(rules.domainLines, [5, 7]);
  });

  test(
    'invalid input identifies the original line and never partially saves',
    () {
      for (final invalid in [
        '192.0.2.1/33',
        '::1/129',
        '999.1.2.3',
        'https://example.com',
        '*.example.com',
        'example.com:443',
        'a..com',
        'example.com/path',
        'a_b.com',
        'example.com..',
        'fe80::1%3',
      ]) {
        expect(
          () => BypassTargets.parse('example.com\n\n$invalid'),
          throwsA(isA<BypassTargetError>().having((e) => e.line, 'line', 3)),
        );
      }
    },
  );

  test('rule limits are independent and duplicates do not consume slots', () {
    expect(
      BypassTargets.parse(List.filled(257, 'example.com').join('\n')).domains,
      ['example.com'],
    );
    expect(
      () => BypassTargets.parse(
        List.generate(257, (i) => '$i.example.com').join('\n'),
      ),
      throwsA(
        isA<BypassTargetError>().having(
          (e) => e.messageKey,
          'limit',
          'bypass_limit',
        ),
      ),
    );
    expect(BypassTargets.parse('').cidrs, isEmpty);
  });
}
