import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/encrypted_dns_endpoint.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/widgets/direct_dns_editor.dart';
import 'package:usque/widgets/warp_dns_editor.dart';

import 'audit_forms_test.dart' show decode;

final strings = AppStrings(LocalePreference.english);
Finder input(String key) => find.byWidgetPredicate(
  (widget) =>
      widget is TextField && widget.decoration?.labelText == strings.get(key),
);

Future<void> selectDns(WidgetTester tester, bool warp, String label) async {
  final picker = warp
      ? find.byType(DropdownButtonFormField<WarpDnsMode>)
      : find.byType(DropdownButtonFormField<DirectDnsMode>);
  await tester.ensureVisible(picker);
  await tester.pumpAndSettle();
  await tester.tap(picker);
  await tester.pumpAndSettle();
  await tester.tap(find.text(strings.get(label)).last);
  await tester.pumpAndSettle();
}

void main() {
  test(
    'HTTPS DNS URLs preserve paths and require supported, unambiguous endpoints',
    () {
      for (final url in [
        '',
        'dns.example',
        'http://dns.example/dns-query',
        'https://',
        'https://1.1.1.1/dns-query',
        'https://[::1]/dns-query',
        'https://user:secret@dns.example/dns-query',
        'https://dns.example:0/dns-query',
        'https://dns.example:65536/dns-query',
        'https://dns.example:abc/dns-query',
        'https://dns.example:/dns-query',
        'https://dns.example/dns-query?q=1',
        'https://dns.example/dns-query#fragment',
        'https://dns.example//dns-query',
        'https://dns.example/dns query',
        'https://dns.example/dns-query\n',
        ' https://dns.example/dns-query',
        r'https://dns.example\other/dns-query',
        'https://${'a' * 64}.example/dns-query',
        'https://dns.example/${'a' * 256}',
      ]) {
        expect(DohEndpoint.tryParse(url), isNull, reason: url);
      }
      const rawPath = '/custom%2Fpath/a//b/../c';
      final custom = DohEndpoint.tryParse('https://例子.测试:8443$rawPath')!;
      expect(custom.serverName, '例子.测试');
      expect(custom.port, 8443);
      expect(custom.path, rawPath);
      expect(custom.url, 'https://例子.测试:8443$rawPath');
      expect(DohEndpoint.tryParse('https://dns.example')!.path, '/dns-query');
      expect(DohEndpoint.tryParse('https://dns.example/')!.path, '/');
      expect(
        const DohEndpoint('old.example', 0, '').url,
        'https://old.example/dns-query',
      );
      expect(
        const DohEndpoint('old.example', 443, '/query').url,
        'https://old.example/query',
      );
    },
  );

  for (final warp in [true, false]) {
    for (final port in [0, 443, 8443]) {
      testWidgets(
        'legacy ${warp ? 'WARP' : 'direct'} DoH port=$port displays without writes',
        (tester) async {
          final legacy = <String, Object?>{
            'mode': 'doh',
            'server_name': 'legacy.example',
            'port': port,
            'doh_path': port == 0 ? '' : '/custom%2Fpath/a//b',
            'bootstrap_ips': ['192.0.2.1', '2001:db8::1'],
          };
          var current = legacy;
          var writes = 0;
          final form = GlobalKey<FormState>();
          await tester.pumpWidget(
            MaterialApp(
              home: Scaffold(
                body: SingleChildScrollView(
                  child: StatefulBuilder(
                    builder: (context, setState) {
                      void changed(Map<String, Object?> value) => setState(() {
                        writes++;
                        current = value;
                      });
                      return Form(
                        key: form,
                        child: warp
                            ? WarpDnsEditor(
                                value: WarpDnsSettings.fromMap(current),
                                enabled: true,
                                strings: strings,
                                onChanged: (value) => changed(value.toMap()),
                              )
                            : DirectDnsEditor(
                                value: DirectDnsSettings.fromMap(current),
                                enabled: true,
                                strings: strings,
                                onChanged: (value) => changed(value.toMap()),
                              ),
                      );
                    },
                  ),
                ),
              ),
            ),
          );
          await tester.pumpAndSettle();
          final expected = DohEndpoint(
            'legacy.example',
            port,
            legacy['doh_path']! as String,
          ).url;
          expect(
            tester.widget<TextField>(input('dns_doh_url')).controller!.text,
            expected,
          );
          expect(find.byType(TextFormField), findsNWidgets(2));
          expect(input('nq_dns_server'), findsNothing);
          expect(input('nq_dns_path'), findsNothing);
          expect(input(warp ? 'port' : 'nq_dns_port'), findsNothing);
          expect(form.currentState!.validate(), isTrue);
          expect(writes, 0);
          expect(current, legacy);

          const edited = 'https://custom.example:9443/new%2Fpath';
          await tester.enterText(input('dns_doh_url'), edited);
          await tester.pumpAndSettle();
          expect(current['server_name'], 'custom.example');
          expect(current['port'], 9443);
          expect(current['doh_path'], '/new%2Fpath');
          expect(current['bootstrap_ips'], legacy['bootstrap_ips']);
          final profile = UsqueProfile.defaultProfile();
          final updated = warp
              ? profile.copyWith(warpDns: WarpDnsSettings.fromMap(current))
              : profile.copyWith(directDns: DirectDnsSettings.fromMap(current));
          final decoded = decode(const ControlCodec().encodeProfile(updated));
          expect(
            warp ? decoded.warpDns.toMap() : decoded.directDns.toMap(),
            current,
          );

          await tester.enterText(input('dns_doh_url'), 'dns.example');
          await tester.pumpAndSettle();
          expect(form.currentState!.validate(), isFalse);
          expect(validDirectDnsPath(current['doh_path']! as String), isFalse);
          expect(
            tester.widget<TextField>(input('dns_doh_url')).controller!.text,
            'dns.example',
          );
          await selectDns(tester, warp, 'nq_dot');
          expect(current['server_name'], cloudflareDotServer);
          expect(current['port'], 853);
          expect(current['bootstrap_ips'], cloudflareDnsBootstrapIps);
          await selectDns(tester, warp, 'nq_doh');
          expect(
            tester.widget<TextField>(input('dns_doh_url')).controller!.text,
            'dns.example',
          );
          expect(current['bootstrap_ips'], legacy['bootstrap_ips']);
          expect(form.currentState!.validate(), isFalse);
        },
      );
    }

    testWidgets(
      '${warp ? 'WARP' : 'direct'} explicit reset clears inactive DNS drafts',
      (tester) async {
        var current = <String, Object?>{
          'mode': warp ? 'plain' : 'physicalSystem',
        };
        var revision = 0;
        late StateSetter rebuild;
        await tester.pumpWidget(
          MaterialApp(
            home: Scaffold(
              body: SingleChildScrollView(
                child: StatefulBuilder(
                  builder: (context, setState) {
                    rebuild = setState;
                    void changed(Map<String, Object?> value) =>
                        setState(() => current = value);
                    return Form(
                      child: warp
                          ? WarpDnsEditor(
                              value: WarpDnsSettings.fromMap(current),
                              enabled: true,
                              resetRevision: revision,
                              strings: strings,
                              onChanged: (value) => changed(value.toMap()),
                            )
                          : DirectDnsEditor(
                              value: DirectDnsSettings.fromMap(current),
                              enabled: true,
                              resetRevision: revision,
                              strings: strings,
                              onChanged: (value) => changed(value.toMap()),
                            ),
                    );
                  },
                ),
              ),
            ),
          ),
        );
        await selectDns(tester, warp, 'nq_doh');
        expect(current['server_name'], 'cloudflare-dns.com');
        expect(current['bootstrap_ips'], cloudflareDnsBootstrapIps);
        await tester.enterText(
          input('dns_doh_url'),
          'https://draft.example:8443/custom',
        );
        await selectDns(
          tester,
          warp,
          warp ? 'warp_dns_plain' : 'nq_system_dns',
        );
        rebuild(() => revision++);
        await tester.pumpAndSettle();
        await selectDns(tester, warp, 'nq_doh');
        expect(
          tester.widget<TextField>(input('dns_doh_url')).controller!.text,
          cloudflareDohUrl,
        );
        expect(current['bootstrap_ips'], cloudflareDnsBootstrapIps);
      },
    );
  }
}
