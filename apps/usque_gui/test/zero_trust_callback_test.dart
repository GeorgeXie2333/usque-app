import 'package:flutter_test/flutter_test.dart';
import 'package:usque/services/zero_trust_callback.dart';

void main() {
  group('ZeroTrustCallbackSession', () {
    test('matchingCallbackIsConsumedOnlyOnce', () {
      final session = ZeroTrustCallbackSession();
      expect(
        session.begin(' Example-Team '),
        'https://example-team.cloudflareaccess.com/warp',
      );
      const callback =
          'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=assertion';
      expect(session.accept(callback), isTrue);
      expect(session.consume(), callback);
      expect(session.consume(), isNull);
      expect(session.accept(callback), isFalse);
    });

    test('callbackRequiresAnActiveSameTeamLogin', () {
      final session = ZeroTrustCallbackSession();
      const callback =
          'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=assertion';
      expect(session.accept(callback), isFalse);
      session.begin('other-team');
      expect(session.accept(callback), isFalse);
      expect(session.consume(), isNull);
    });

    test('cancellationAndProcessReplacementDiscardState', () {
      final session = ZeroTrustCallbackSession();
      session.begin('example-team');
      session.cancel();
      expect(
        session.accept(
          'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=assertion',
        ),
        isFalse,
      );

      final replacementProcess = ZeroTrustCallbackSession();
      expect(replacementProcess.consume(), isNull);
    });

    test('malformedCallbacksAndTeamsAreRejected', () {
      expect(
        () => ZeroTrustCallbackSession.normalizeTeam('team.example'),
        throwsA(isA<ArgumentError>()),
      );
      const invalidCallbacks = <String>[
        'https://example-team.cloudflareaccess.com/auth?token=x',
        'com.cloudflare.warp://example-team.cloudflareaccess.com/warp?token=x',
        'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=x&token=y',
        'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?state=x',
      ];
      for (final callback in invalidCallbacks) {
        final session = ZeroTrustCallbackSession();
        session.begin('example-team');
        expect(session.accept(callback), isFalse, reason: callback);
        expect(
          ZeroTrustCallbackSession.isValidCallback('example-team', callback),
          isFalse,
          reason: callback,
        );
      }
    });

    test('structuralCallbackChecksMatchAndroidAccept', () {
      const extraInvalid = <String>[
        'com.cloudflare.warp://other.cloudflareaccess.com/auth?token=x',
        'com.cloudflare.warp://user@example-team.cloudflareaccess.com/auth?token=x',
        'com.cloudflare.warp://example-team.cloudflareaccess.com:443/auth?token=x',
        'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=x#fragment',
        'com.cloudflare.warp://example-team.cloudflareaccess.com/auth',
        'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=',
      ];
      for (final callback in extraInvalid) {
        expect(
          ZeroTrustCallbackSession.isValidCallback('example-team', callback),
          isFalse,
          reason: callback,
        );
      }
      expect(
        ZeroTrustCallbackSession.isValidCallback(
          'example-team',
          'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=assertion',
        ),
        isTrue,
      );
    });
  });
}
