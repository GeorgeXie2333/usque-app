import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show StandardMethodCodec;
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/proton_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/chain_proxy_screen.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/widgets/proton_vpn_panel.dart';

import 'chain_proxy_test.dart' show ChainEngine, hostChain;
import 'ui_workflow_test.dart' show workflowHost;

class ProtonEngine extends ChainEngine
    implements ProtonVpnClient, ProtonHumanVerificationClient {
  final requests = <Map<String, Object?>>[];
  final inputs = <String>[];
  bool nativeCancelled = false;
  bool cancelPuzzle = false;
  Map<Object?, Object?> job = const {};
  List<Map<Object?, Object?>> profiles = [];
  static const success =
      '{"type":"HUMAN_VERIFICATION_SUCCESS","payload":{"type":"captcha","token":"synthetic-private-result"}}';

  @override
  Future<EngineCapabilities?> getCapabilities() async =>
      const EngineCapabilities(
        tlsFingerprints: TlsFingerprint.values,
        vpnGateTcp: true,
        automaticEndpoints: true,
        chainProfileImport: true,
        chainWireguard: true,
        chainProtonVpn: true,
        networkSettingsApplication: true,
      );

  @override
  Future<ProtonVpnResult> protonVpn(
    Map<String, Object?> command, {
    Uint8List? sensitiveInput,
  }) async {
    requests.add(Map.of(command));
    if (sensitiveInput != null) inputs.add(utf8.decode(sensitiveInput));
    switch (command['action']) {
      case 'guest':
        job = {
          'id': 'synthetic-job',
          'state': 'waiting_verification',
          'phase': 'credentialless',
          'challenge_id': 'synthetic-challenge',
        };
      case 'login':
        job = {'id': 'synthetic-job', 'state': 'waiting_totp', 'phase': 'totp'};
      case 'totp':
      case 'verify':
        job = {
          'id': 'synthetic-job',
          'state': 'waiting_node',
          'phase': 'directory',
          'nodes': [
            {
              'id': 'node-1',
              'name': 'Free server',
              'country': 'JP',
              'city': '',
            },
          ],
        };
      case 'select_node':
        job = {
          'id': 'synthetic-job',
          'state': 'completed',
          'phase': 'complete',
          'profile_id': 'proton-profile',
        };
      case 'cancel':
        job = {'id': 'synthetic-job', 'state': 'cancelled', 'phase': 'cleanup'};
      case 'forget':
        profiles = [];
        job = {'id': 'forget-job', 'state': 'completed', 'phase': 'complete'};
      case 'handoff':
        return ProtonVpnResult(
          metadata: const {},
          sensitiveHandoff: Uint8List.fromList(
            utf8.encode(
              '{"job_id":"synthetic-job","challenge_id":"synthetic-challenge","url":"https://verify.proton.me/?token=synthetic-private-challenge"}',
            ),
          ),
        );
    }
    return ProtonVpnResult(
      metadata: {
        'job': job.isEmpty ? null : job,
        'profiles': profiles,
        'error': null,
      },
    );
  }

  @override
  Future<Uint8List?> completeProtonHumanVerification(Uint8List handoff) async {
    final value = jsonDecode(utf8.decode(handoff)) as Map<String, dynamic>;
    expect(value['job_id'], 'synthetic-job');
    expect(value['challenge_id'], 'synthetic-challenge');
    return cancelPuzzle ? null : Uint8List.fromList(utf8.encode(success));
  }

  @override
  Future<void> cancelProtonHumanVerification() async {
    nativeCancelled = true;
  }
}

class PendingLogoutEngine extends ProtonEngine {
  @override
  Future<ProtonVpnResult> protonVpn(
    Map<String, Object?> command, {
    Uint8List? sensitiveInput,
  }) async {
    if (command['action'] == 'logout') {
      requests.add(Map.of(command));
      job = {'id': 'logout-job', 'state': 'running', 'phase': 'revoke'};
      return ProtonVpnResult(metadata: {'job': job, 'profiles': <Object?>[]});
    }
    return super.protonVpn(command, sensitiveInput: sensitiveInput);
  }
}

class PendingPuzzleEngine extends ProtonEngine {
  final answer = Completer<Uint8List?>();
  int nativeCloses = 0;
  @override
  Future<Uint8List?> completeProtonHumanVerification(Uint8List handoff) =>
      answer.future;
  @override
  Future<void> cancelProtonHumanVerification() async {
    nativeCloses++;
  }
}

class ReadOnlyPuzzleEngine extends ProtonEngine {
  ReadOnlyPuzzleEngine({required this.readOnlyHandoff});
  final bool readOnlyHandoff;

  @override
  Future<ProtonVpnResult> protonVpn(
    Map<String, Object?> command, {
    Uint8List? sensitiveInput,
  }) async {
    final result = await super.protonVpn(
      command,
      sensitiveInput: sensitiveInput,
    );
    if (readOnlyHandoff && command['action'] == 'handoff') {
      return ProtonVpnResult(
        metadata: result.metadata,
        sensitiveHandoff: result.sensitiveHandoff.asUnmodifiableView(),
      );
    }
    return result;
  }

  @override
  Future<Uint8List?> completeProtonHumanVerification(Uint8List handoff) async =>
      Uint8List.fromList(
        utf8.encode(ProtonEngine.success),
      ).asUnmodifiableView();
}

class CleaningPuzzleEngine extends PendingPuzzleEngine {
  @override
  Future<ProtonVpnResult> protonVpn(
    Map<String, Object?> command, {
    Uint8List? sensitiveInput,
  }) async {
    if (command['action'] == 'cancel') {
      requests.add(Map.of(command));
      job = {'id': 'synthetic-job', 'state': 'running', 'phase': 'cleanup'};
      return ProtonVpnResult(metadata: {'job': job});
    }
    return super.protonVpn(command, sensitiveInput: sensitiveInput);
  }
}

class RejectedTotpEngine extends ProtonEngine {
  bool rejectNext = true;

  @override
  Future<ProtonVpnResult> protonVpn(
    Map<String, Object?> command, {
    Uint8List? sensitiveInput,
  }) async {
    if (command['action'] == 'totp' && rejectNext) {
      rejectNext = false;
      requests.add(Map.of(command));
      return ProtonVpnResult(
        metadata: const {
          'job': null,
          'profiles': [],
          'error': 'unexpected_interaction',
        },
      );
    }
    return super.protonVpn(command, sensitiveInput: sensitiveInput);
  }
}

class DelayedProtonEngine extends ProtonEngine {
  final reply = Completer<ProtonVpnResult>();
  String? delayedAction;

  @override
  Future<ProtonVpnResult> protonVpn(
    Map<String, Object?> command, {
    Uint8List? sensitiveInput,
  }) async {
    if (command['action'] == delayedAction) {
      delayedAction = null;
      requests.add(Map.of(command));
      return reply.future;
    }
    return super.protonVpn(command, sensitiveInput: sensitiveInput);
  }
}

class LostProtonLaunchEngine extends ProtonEngine {
  LostProtonLaunchEngine(this.action);

  final String action;
  Completer<void>? launchAcknowledgement;
  Completer<ProtonVpnResult>? recoveryReply;
  Object? launchFailure;
  Uint8List? launchInput;
  bool lostLaunch = false;
  int recoveryReads = 0;
  int failedRecoveryReads = 0;
  Map<Object?, Object?>? nextRecoveryMetadata;

  @override
  Future<ProtonVpnResult> protonVpn(
    Map<String, Object?> command, {
    Uint8List? sensitiveInput,
  }) async {
    if (command['action'] == action && !lostLaunch) {
      final result = await super.protonVpn(
        command,
        sensitiveInput: sensitiveInput,
      );
      result.clearHandoff();
      launchInput = sensitiveInput;
      lostLaunch = true;
      await launchAcknowledgement?.future;
      if (launchFailure case final Object failure) throw failure;
      throw const EngineException(
        'PROTON_RESULT_UNCONFIRMED',
        'Refresh Proton status before retrying.',
      );
    }
    if (command['action'] == 'get' && lostLaunch) {
      requests.add(Map.of(command));
      recoveryReads++;
      final pending = recoveryReply;
      if (pending != null) {
        recoveryReply = null;
        return pending.future;
      }
      if (failedRecoveryReads > 0) {
        failedRecoveryReads--;
        throw const EngineException(
          'PROTON_UNAVAILABLE',
          'Status unavailable.',
        );
      }
      final metadata = nextRecoveryMetadata;
      nextRecoveryMetadata = null;
      return ProtonVpnResult(
        metadata:
            metadata ??
            {
              'job': job.isEmpty ? null : job,
              'profiles': profiles,
              'error': null,
            },
      );
    }
    return super.protonVpn(command, sensitiveInput: sensitiveInput);
  }
}

class CatalogueEngine extends ProtonEngine {
  bool stalePage = false;
  CatalogueEngine() {
    profiles = [
      {
        'id': 'proton-profile',
        'name': 'Account',
        'needs_login': false,
        'related_ids': ['proton-profile'],
        'server_count': 301,
      },
    ];
  }
  @override
  Future<ProtonVpnResult> protonVpn(
    Map<String, Object?> command, {
    Uint8List? sensitiveInput,
  }) async {
    if (command['action'] == 'servers') {
      requests.add(Map.of(command));
      final offset = command['offset']! as int;
      return ProtonVpnResult(
        metadata: {
          'server_offset': offset,
          'server_total': 301,
          'server_revision': stalePage && offset > 0
              ? 'changed'
              : 'catalogue-revision',
          'servers': [
            for (var i = offset; i < 301 && i < offset + 128; i++)
              {
                'id': 'logical-$i',
                'name': 'Server $i',
                'country': 'IS',
                'city': 'City $i',
                'load': 0,
              },
          ],
        },
      );
    }
    if (command['action'] == 'choose_server') {
      requests.add(Map.of(command));
      final id = command['node_id'] as String?;
      profiles = [
        {
          ...profiles.first,
          'server_id': id,
          'node_name': id == null ? '' : 'Server ${id.split('-').last}',
        },
      ];
      job = {
        'id': 'choice',
        'state': 'completed',
        'phase': 'complete',
        'profile_id': 'proton-profile',
      };
      return ProtonVpnResult(metadata: {'job': job, 'profiles': profiles});
    }
    return super.protonVpn(command, sensitiveInput: sensitiveInput);
  }
}

class RemovalEngine extends CatalogueEngine {
  bool? selectionWasCleared;
  @override
  Future<ProtonVpnResult> protonVpn(
    Map<String, Object?> command, {
    Uint8List? sensitiveInput,
  }) async {
    if (command['action'] == 'forget') {
      selectionWasCleared = storedProfiles.every(
        (profile) => profile.chainExit?.profileId != 'proton-profile',
      );
      library = [];
    }
    return super.protonVpn(command, sensitiveInput: sensitiveInput);
  }
}

void _protonWidgetTest(
  String description,
  WidgetTesterCallback callback, {
  TargetPlatform platform = TargetPlatform.windows,
}) {
  testWidgets(
    description,
    callback,
    variant: TargetPlatformVariant.only(platform),
  );
}

Future<void> _startProtonAuthentication(
  WidgetTester tester,
  String action,
) async {
  await tester.tap(
    find.text(action == 'guest' ? 'Continue as guest' : 'Sign in to Proton'),
  );
  if (action == 'login') {
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).at(0), 'synthetic-user');
    await tester.enterText(find.byType(TextField).at(1), 'synthetic-password');
    await tester.tap(find.widgetWithText(FilledButton, 'Continue'));
  }
}

void main() {
  for (final (action, invalidResponse) in [
    ('guest', false),
    ('login', false),
    ('guest', true),
  ]) {
    _protonWidgetTest(
      '$action with a ${invalidResponse ? 'malformed' : 'lost'} launch reply recovers the existing job without replay',
      (tester) async {
        final engine = LostProtonLaunchEngine(action)
          ..launchFailure = invalidResponse
              ? const FormatException('Invalid Proton response')
              : null
          ..job = {
            'id': 'older-job',
            'state': 'completed',
            'phase': 'complete',
          };
        final app = await hostChain(tester, engine);
        await tester.pumpWidget(
          workflowHost(
            app,
            home: Scaffold(
              body: SingleChildScrollView(
                child: ProtonVpnPanel(controller: app, onChanged: () {}),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        await _startProtonAuthentication(tester, action);
        await tester.pumpAndSettle();
        expect(
          engine.requests.where((r) => r['action'] == action),
          hasLength(1),
        );
        expect(engine.recoveryReads, 1);
        expect(engine.requests.last, {'action': 'get'});
        expect(
          find.text(
            action == 'guest'
                ? 'Verification required'
                : 'Enter your authenticator code to continue.',
          ),
          findsOneWidget,
        );
        if (action == 'login') {
          expect(engine.inputs, hasLength(1));
          expect(engine.launchInput!.every((byte) => byte == 0), isTrue);
        }
        await tester.pump(const Duration(seconds: 2));
        await tester.pumpAndSettle();
        expect(engine.requests.last, {
          'action': 'get',
          'job_id': 'synthetic-job',
        });
        await tester.tap(find.widgetWithText(TextButton, 'Cancel'));
        await tester.pumpAndSettle();
        expect(engine.requests.last, {
          'action': 'cancel',
          'job_id': 'synthetic-job',
        });
        await tester.pumpWidget(const SizedBox.shrink());
      },
    );
  }

  for (final code in ['PROTON_INPUT_TOO_LARGE', 'PROTON_INVALID_REQUEST']) {
    _protonWidgetTest(
      'a definitive $code launch rejection does not query or replay setup',
      (tester) async {
        final engine = DelayedProtonEngine()..delayedAction = 'guest';
        final app = await hostChain(tester, engine);
        await tester.pumpWidget(
          workflowHost(
            app,
            home: Scaffold(
              body: SingleChildScrollView(
                child: ProtonVpnPanel(controller: app, onChanged: () {}),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        await _startProtonAuthentication(tester, 'guest');
        await tester.pump();
        engine.reply.completeError(EngineException(code, 'Invalid input.'));
        await tester.pumpAndSettle();
        expect(
          engine.requests.where((r) => r['action'] == 'get'),
          hasLength(1),
        );
        expect(
          engine.requests.where((r) => r['action'] == 'guest'),
          hasLength(1),
        );
        expect(find.text('Refresh status'), findsNothing);
        expect(find.text(app.strings.proton('failed')), findsOneWidget);
        expect(
          tester
              .widget<OutlinedButton>(
                find.widgetWithText(OutlinedButton, 'Continue as guest'),
              )
              .onPressed,
          isNotNull,
        );
        await tester.pumpWidget(const SizedBox.shrink());
      },
    );
  }

  _protonWidgetTest(
    'deferred lost launch and recovery keep duplicate authentication disabled',
    (tester) async {
      final acknowledgement = Completer<void>();
      final recovery = Completer<ProtonVpnResult>();
      final engine = LostProtonLaunchEngine('guest')
        ..launchAcknowledgement = acknowledgement
        ..recoveryReply = recovery;
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await _startProtonAuthentication(tester, 'guest');
      await tester.pump();
      for (final label in ['Continue as guest', 'Sign in to Proton']) {
        expect(
          tester
              .widget<OutlinedButton>(
                find.widgetWithText(OutlinedButton, label),
              )
              .onPressed,
          isNull,
        );
      }
      acknowledgement.complete();
      await tester.pump();
      expect(engine.recoveryReads, 1);
      expect(
        find.text(app.strings.proton('result_unconfirmed')),
        findsOneWidget,
      );
      await tester.tap(find.text('Continue as guest'));
      await tester.pump(const Duration(seconds: 4));
      expect(
        engine.requests.where((r) => r['action'] == 'guest'),
        hasLength(1),
      );
      expect(engine.recoveryReads, 1);
      recovery.complete(
        ProtonVpnResult(metadata: {'job': engine.job, 'profiles': []}),
      );
      await tester.pumpAndSettle();
      expect(find.text('Verification required'), findsOneWidget);
      expect(find.text(app.strings.proton('result_unconfirmed')), findsNothing);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  _protonWidgetTest(
    'failed recovery reads retain the catalogue and offer read-only status retry',
    (tester) async {
      final engine = LostProtonLaunchEngine('guest')
        ..failedRecoveryReads = 1
        ..profiles = [
          {
            'id': 'existing-profile',
            'name': 'Existing account',
            'server_count': 1,
          },
        ];
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await _startProtonAuthentication(tester, 'guest');
      await tester.pumpAndSettle();
      expect(find.text('Existing account'), findsOneWidget);
      expect(
        find.text(app.strings.proton('result_unconfirmed')),
        findsOneWidget,
      );
      expect(
        tester
            .widget<OutlinedButton>(
              find.widgetWithText(OutlinedButton, 'Sign in to Proton'),
            )
            .onPressed,
        isNull,
      );
      expect(
        tester
            .widget<TextButton>(
              find.widgetWithText(TextButton, 'Remove account'),
            )
            .onPressed,
        isNull,
      );
      await tester.pump(const Duration(seconds: 10));
      expect(engine.recoveryReads, 1);
      engine.nextRecoveryMetadata = {
        'job': null,
        'profiles': [],
        'error': 'secure_storage_failed',
      };
      await tester.tap(find.text('Refresh status'));
      await tester.pumpAndSettle();
      expect(find.text('Existing account'), findsOneWidget);
      expect(
        find.text(app.strings.proton('result_unconfirmed')),
        findsOneWidget,
      );
      await tester.tap(find.text('Refresh status'));
      await tester.pumpAndSettle();
      expect(engine.recoveryReads, 3);
      expect(
        engine.requests.where((r) => r['action'] == 'guest'),
        hasLength(1),
      );
      expect(find.text('Verification required'), findsOneWidget);
      expect(find.text('Refresh status'), findsNothing);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  _protonWidgetTest(
    'a recovered job remains cancellable when its catalogue read failed',
    (tester) async {
      final engine = LostProtonLaunchEngine('guest')
        ..nextRecoveryMetadata = {
          'job': {
            'id': 'synthetic-job',
            'state': 'waiting_verification',
            'challenge_id': 'synthetic-challenge',
          },
          'profiles': [],
          'error': 'secure_storage_failed',
        };
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await _startProtonAuthentication(tester, 'guest');
      await tester.pumpAndSettle();
      expect(find.text('Verification required'), findsOneWidget);
      expect(find.text(app.strings.proton('failed')), findsOneWidget);
      expect(find.text(app.strings.proton('result_unconfirmed')), findsNothing);
      await tester.tap(find.widgetWithText(TextButton, 'Cancel'));
      await tester.pumpAndSettle();
      expect(engine.requests.last, {
        'action': 'cancel',
        'job_id': 'synthetic-job',
      });
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  _protonWidgetTest(
    'Android automatically resumes a recovered guest challenge exactly once',
    (tester) async {
      final engine = LostProtonLaunchEngine('guest');
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await _startProtonAuthentication(tester, 'guest');
      await tester.pumpAndSettle();
      expect(find.text('Choose a server to finish setup.'), findsOneWidget);
      for (final action in ['guest', 'handoff', 'verify']) {
        expect(
          engine.requests.where((r) => r['action'] == action),
          hasLength(1),
        );
      }
      expect(engine.inputs, [ProtonEngine.success]);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    platform: TargetPlatform.android,
  );

  for (final state in [
    'missing',
    'completed',
    'missing-job',
    'unknown-state',
    'missing-id',
  ]) {
    _protonWidgetTest(
      'lost launch recovery handles $state status without replaying setup',
      (tester) async {
        final engine = LostProtonLaunchEngine('guest')
          ..nextRecoveryMetadata = {
            if (state != 'missing-job')
              'job': switch (state) {
                'missing' => null,
                'unknown-state' => {'id': 'saved-job', 'state': 'unsupported'},
                'missing-id' => {'state': 'running'},
                _ => {
                  'id': 'saved-job',
                  'state': 'completed',
                  'profile_id': 'saved-profile',
                },
              },
            'profiles': <Object?>[],
          };
        final app = await hostChain(tester, engine);
        var changed = 0;
        await tester.pumpWidget(
          workflowHost(
            app,
            home: Scaffold(
              body: SingleChildScrollView(
                child: ProtonVpnPanel(
                  controller: app,
                  onChanged: () => changed++,
                ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        await _startProtonAuthentication(tester, 'guest');
        await tester.pumpAndSettle();
        final blocked = !const {'missing', 'completed'}.contains(state);
        expect(
          tester
                  .widget<OutlinedButton>(
                    find.widgetWithText(OutlinedButton, 'Continue as guest'),
                  )
                  .onPressed ==
              null,
          blocked,
        );
        expect(
          find.text('Refresh status'),
          blocked ? findsOneWidget : findsNothing,
        );
        expect(changed, state == 'completed' ? 1 : 0);
        expect(
          engine.requests.where((r) => r['action'] == 'guest'),
          hasLength(1),
        );
        await tester.pumpWidget(const SizedBox.shrink());
      },
    );
  }

  _protonWidgetTest(
    'an old recovery reply cannot cancel a newer panel job after disposal',
    (tester) async {
      final recovery = Completer<ProtonVpnResult>();
      final engine = LostProtonLaunchEngine('guest')..recoveryReply = recovery;
      final app = await hostChain(tester, engine);
      Widget panel() => workflowHost(
        app,
        home: Scaffold(
          body: SingleChildScrollView(
            child: ProtonVpnPanel(controller: app, onChanged: () {}),
          ),
        ),
      );
      await tester.pumpWidget(panel());
      await tester.pumpAndSettle();
      await _startProtonAuthentication(tester, 'guest');
      await tester.pump();
      expect(engine.recoveryReads, 1);
      await tester.pumpWidget(const SizedBox.shrink());
      engine.job = const {};
      await tester.pumpWidget(panel());
      await tester.pumpAndSettle();
      await _startProtonAuthentication(tester, 'guest');
      await tester.pumpAndSettle();
      recovery.complete(
        ProtonVpnResult(metadata: {'job': engine.job, 'profiles': []}),
      );
      await tester.pumpAndSettle();
      expect(engine.requests.where((r) => r['action'] == 'cancel'), isEmpty);
      expect(find.text('Verification required'), findsOneWidget);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pumpAndSettle();
      expect(
        engine.requests.where((r) => r['action'] == 'cancel'),
        hasLength(1),
      );
    },
  );

  _protonWidgetTest(
    'a lost launch reply after disposal does not read or cancel an unknown job',
    (tester) async {
      final acknowledgement = Completer<void>();
      final engine = LostProtonLaunchEngine('guest')
        ..launchAcknowledgement = acknowledgement;
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await _startProtonAuthentication(tester, 'guest');
      await tester.pump();
      await tester.pumpWidget(const SizedBox.shrink());
      acknowledgement.complete();
      await tester.pumpAndSettle();
      expect(engine.recoveryReads, 0);
      expect(engine.requests.where((r) => r['action'] == 'cancel'), isEmpty);
      expect(tester.takeException(), isNull);
    },
  );

  for (final chinese in [false, true]) {
    _protonWidgetTest(
      'Android account and server picker support 200 percent text, Chinese=$chinese',
      (tester) async {
        final engine = CatalogueEngine();
        final app = await hostChain(tester, engine, width: 390);
        if (chinese) app.localePreference = LocalePreference.simplifiedChinese;
        await tester.pumpWidget(
          workflowHost(
            app,
            dark: chinese,
            scale: 2,
            home: Scaffold(
              body: SingleChildScrollView(
                child: ProtonVpnPanel(controller: app, onChanged: () {}),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        final change = find.text(app.strings.proton('change_server'));
        await tester.ensureVisible(change);
        await tester.tap(change);
        await tester.pumpAndSettle();
        await tester.enterText(find.byType(TextField), 'Server 300');
        await tester.pumpAndSettle();
        await tester.ensureVisible(
          find.widgetWithText(RadioListTile<String>, 'Server 300'),
        );
        await tester.tap(
          find.widgetWithText(RadioListTile<String>, 'Server 300'),
        );
        await tester.pumpAndSettle();
        expect(engine.requests.last['node_id'], 'logical-300');
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
      },
      platform: TargetPlatform.android,
    );
  }
  for (final fail in [false, true]) {
    _protonWidgetTest(
      'account removal clears the saved selection before deleting, save fails=$fail',
      (tester) async {
        final engine = RemovalEngine()
          ..library = [
            const ChainProfileSummary(
              id: 'proton-profile',
              revision: 'r1',
              editRevision: 'e1',
              name: 'Account',
              source: ChainSource.protonVpn,
              protocol: 'wireguard',
              host: '192.0.2.1',
              port: 443,
            ),
          ];
        final app = await hostChain(
          tester,
          engine,
          source: ChainSource.protonVpn,
          chainEnabled: true,
        );
        expect(
          await app.saveNetwork(
            app.activeProfile.copyWith(
              chainExit: const ChainExitSettings(
                enabled: true,
                source: ChainSource.protonVpn,
                profileId: 'proton-profile',
                revision: 'r1',
              ),
            ),
            changedFields: ['chain_exit'],
          ),
          isTrue,
          reason: app.lastError,
        );
        await tester.pumpWidget(const SizedBox.shrink());
        await tester.pumpWidget(
          workflowHost(app, home: ChainProxyScreen(controller: app)),
        );
        await tester.pumpAndSettle();
        expect(find.text('Saved configurations'), findsNothing);
        engine.failSave = fail;
        await tester.ensureVisible(find.text('Remove account'));
        await tester.tap(find.text('Remove account'));
        await tester.pumpAndSettle();
        await tester.tap(find.text('Forget locally'));
        await tester.pumpAndSettle();
        if (fail) {
          expect(engine.selectionWasCleared, isNull);
          expect(
            engine.requests.where((r) => r['action'] == 'forget'),
            isEmpty,
          );
          expect(app.activeProfile.chainExit?.profileId, 'proton-profile');
        } else {
          expect(engine.selectionWasCleared, isTrue);
          expect(app.activeProfile.chainExit?.profileId, isNull);
        }
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
      },
      platform: TargetPlatform.android,
    );
  }

  for (final stale in [false, true]) {
    _protonWidgetTest(
      'complete searchable server catalogue rejects stale pages=$stale',
      (tester) async {
        final engine = CatalogueEngine()..stalePage = stale;
        final app = await hostChain(tester, engine, width: 390);
        await tester.pumpWidget(
          workflowHost(
            app,
            home: Scaffold(
              body: SingleChildScrollView(
                child: ProtonVpnPanel(controller: app, onChanged: () {}),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        await tester.ensureVisible(find.text('Change server'));
        await tester.tap(find.text('Change server'));
        await tester.pumpAndSettle();
        final pages = engine.requests
            .where((r) => r['action'] == 'servers')
            .toList();
        expect(pages.map((r) => r['offset']), stale ? [0, 128] : [0, 128, 256]);
        if (stale) {
          expect(
            find.text(app.strings.proton('invalid_directory')),
            findsOneWidget,
          );
          expect(
            engine.requests.where((r) => r['action'] == 'choose_server'),
            isEmpty,
          );
        } else {
          await tester.enterText(find.byType(TextField), 'Server 300');
          await tester.pumpAndSettle();
          expect(
            find.widgetWithText(RadioListTile<String>, 'Server 300'),
            findsOneWidget,
          );
          await tester.tap(
            find.widgetWithText(RadioListTile<String>, 'Server 300'),
          );
          await tester.pumpAndSettle();
          expect(engine.requests.last, {
            'action': 'choose_server',
            'profile_id': 'proton-profile',
            'node_id': 'logical-300',
          });
          expect(
            find.text('Server preference saved for the next connection.'),
            findsOneWidget,
          );
          await tester.ensureVisible(find.text('Change server'));
          await tester.tap(find.text('Change server'));
          await tester.pumpAndSettle();
          await tester.tap(find.text('Automatic'));
          await tester.pumpAndSettle();
          expect(engine.requests.last, {
            'action': 'choose_server',
            'profile_id': 'proton-profile',
            'node_id': null,
          });
          expect(engine.inputs, isEmpty);
        }
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
      },
    );
  }
  _protonWidgetTest(
    'removal waits for selection cleanup and stops when it fails',
    (tester) async {
      final engine = CatalogueEngine();
      final app = await hostChain(tester, engine);
      var allowed = false, calls = 0;
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(
                controller: app,
                onChanged: () {},
                beforeRemove: (ids) async {
                  expect(ids, ['proton-profile']);
                  calls++;
                  return allowed;
                },
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      for (var i = 0; i < 2; i++) {
        await tester.ensureVisible(find.text('Remove account'));
        await tester.tap(find.text('Remove account'));
        await tester.pumpAndSettle();
        await tester.tap(find.text('Forget locally'));
        await tester.pumpAndSettle();
        expect(engine.requests.where((r) => r['action'] == 'forget').length, i);
        allowed = true;
      }
      expect(calls, 2);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  test(
    'Proton handoffs own mutable bytes from read-only platform envelopes',
    () {
      const codec = StandardMethodCodec();
      final native = Uint8List.fromList(
        utf8.encode('synthetic-private-handoff'),
      );
      final message = codec.encodeSuccessEnvelope(native).asUnmodifiableView();
      final borrowed = codec.decodeEnvelope(message) as Uint8List;
      final result = ProtonVpnResult(
        metadata: const {},
        sensitiveHandoff: borrowed,
      );
      expect(utf8.decode(result.sensitiveHandoff), 'synthetic-private-handoff');
      result.clearHandoff();
      result.clearHandoff();
      expect(result.sensitiveHandoff.every((byte) => byte == 0), isTrue);

      final transferred = Uint8List.fromList(native);
      final writable = ProtonVpnResult(
        metadata: const {},
        sensitiveHandoff: transferred,
      );
      expect(transferred.every((byte) => byte == 0), isTrue);
      expect(
        utf8.decode(writable.sensitiveHandoff),
        'synthetic-private-handoff',
      );
      writable.clearHandoff();
    },
  );

  for (final readOnlyHandoff in [true, false]) {
    _protonWidgetTest(
      'Android resumes verification with read-only platform bytes, handoff=$readOnlyHandoff',
      (tester) async {
        final engine = ReadOnlyPuzzleEngine(readOnlyHandoff: readOnlyHandoff);
        final app = await hostChain(tester, engine);
        await tester.pumpWidget(
          workflowHost(
            app,
            home: Scaffold(
              body: SingleChildScrollView(
                child: ProtonVpnPanel(controller: app, onChanged: () {}),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        await tester.tap(find.text('Continue as guest'));
        await tester.pumpAndSettle();
        expect(engine.requests.where((r) => r['action'] == 'verify').length, 1);
        expect(engine.requests.where((r) => r['action'] == 'cancel'), isEmpty);
        expect(engine.inputs, [ProtonEngine.success]);
        expect(find.text('Free server'), findsOneWidget);
        expect(
          find.text(app.strings.proton('verification_failed')),
          findsNothing,
        );
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
      },
      platform: TargetPlatform.android,
    );
  }

  for (final failure in ['BRIDGE', 'EXPIRED', 'unknown']) {
    final expired = failure == 'EXPIRED';
    _protonWidgetTest(
      'Android verification $failure survives cleanup polling without reporting user cancellation',
      (tester) async {
        final engine = CleaningPuzzleEngine();
        final app = await hostChain(tester, engine);
        await tester.pumpWidget(
          workflowHost(
            app,
            home: Scaffold(
              body: SingleChildScrollView(
                child: ProtonVpnPanel(controller: app, onChanged: () {}),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        await tester.tap(find.text('Continue as guest'));
        await tester.pump();
        engine.answer.completeError(
          EngineException(
            failure == 'unknown'
                ? 'synthetic-private-unrecognized-code'
                : 'PROTON_VERIFICATION_$failure',
            'synthetic-private-window-error',
          ),
        );
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 100));
        expect(engine.requests.where((r) => r['action'] == 'cancel').length, 1);
        expect(engine.requests.where((r) => r['action'] == 'verify'), isEmpty);
        engine.job = {
          'id': 'synthetic-job',
          'state': 'cancelled',
          'phase': 'cleanup',
          if (!expired) 'failure': 'cleanup_failed_remote_state_unknown',
        };
        await tester.pump(const Duration(seconds: 2));
        await tester.pumpAndSettle();
        expect(find.text('Setup cancelled'), findsNothing);
        if (failure == 'unknown') {
          expect(find.textContaining('Verification error:'), findsNothing);
        } else {
          expect(find.text('Verification error: $failure'), findsOneWidget);
        }
        expect(
          find.text(
            expired
                ? app.strings.proton('expired')
                : app.strings.proton('verification_failed'),
          ),
          findsOneWidget,
        );
        if (!expired) {
          expect(
            find.text(app.strings.proton('cancelled_remote_unknown')),
            findsOneWidget,
          );
        }
        expect(find.textContaining('synthetic-private'), findsNothing);
        await tester.pumpWidget(const SizedBox.shrink());
      },
      platform: TargetPlatform.android,
    );
  }

  _protonWidgetTest(
    'Android cancellation closes the automatic puzzle and rejects a late result',
    (tester) async {
      final engine = PendingPuzzleEngine();
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('Continue as guest'));
      await tester.pump();
      expect(engine.requests.where((r) => r['action'] == 'handoff').length, 1);
      await tester.tap(find.widgetWithText(TextButton, 'Cancel'));
      await tester.pump();
      expect(engine.nativeCloses, 1);
      expect(engine.requests.where((r) => r['action'] == 'cancel').length, 1);
      engine.answer.complete(
        Uint8List.fromList(
          utf8.encode(ProtonEngine.success),
        ).asUnmodifiableView(),
      );
      await tester.pumpAndSettle();
      expect(engine.requests.where((r) => r['action'] == 'verify'), isEmpty);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    platform: TargetPlatform.android,
  );

  _protonWidgetTest(
    'Android opens each native challenge once and resumes after manual success',
    (tester) async {
      final engine = PendingPuzzleEngine();
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('Continue as guest'));
      await tester.pump();
      expect(engine.requests.where((r) => r['action'] == 'handoff').length, 1);
      await tester.pump(const Duration(seconds: 2));
      expect(engine.requests.where((r) => r['action'] == 'handoff').length, 1);
      engine.answer.complete(
        Uint8List.fromList(utf8.encode(ProtonEngine.success)),
      );
      await tester.pumpAndSettle();
      expect(engine.requests.where((r) => r['action'] == 'verify').length, 1);
      expect(find.text('Free server'), findsOneWidget);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    platform: TargetPlatform.android,
  );
  test('Proton fields append and keep handoffs separate from metadata', () {
    expect(
      () => encodeProtonVpnCommand({
        'action': 'login',
        'password': 'synthetic-secret',
      }),
      throwsFormatException,
    );
    const codec = ControlCodec();
    final caps = ControlPayloadWriter()
      ..string(1, 'caps')
      ..message(15, (ControlPayloadWriter()..boolean(50, true)).takeBytes());
    expect(
      codec
          .decodeResponse(codec.frame(caps.takeBytes()), 'caps')
          .capabilities!
          .chainProtonVpn,
      isTrue,
    );
    final body = ControlPayloadWriter()
      ..string(1, 'result')
      ..message(
        28,
        (ControlPayloadWriter()
              ..string(1, '{"profiles":[]}')
              ..bytes(
                2,
                Uint8List.fromList(utf8.encode('synthetic-private-handoff')),
              ))
            .takeBytes(),
      );
    final result = codec
        .decodeResponse(codec.frame(body.takeBytes()), 'result')
        .protonVpn!;
    expect(result.metadata, {'profiles': <Object?>[]});
    expect(result.toString(), isNot(contains('synthetic-private-handoff')));
    expect(utf8.decode(result.sensitiveHandoff), 'synthetic-private-handoff');
    result.clearHandoff();
    expect(result.sensitiveHandoff.every((byte) => byte == 0), isTrue);
  });

  _protonWidgetTest(
    'guest puzzle and server selection prepare without applying network settings',
    (tester) async {
      final engine = ProtonEngine();
      final app = await hostChain(tester, engine);
      var changed = 0;
      final before = app.activeProfile.chainExit;
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(
                controller: app,
                onChanged: () => changed++,
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('Continue as guest'));
      await tester.pumpAndSettle();
      expect(find.text('Verification required'), findsOneWidget);
      await tester.tap(find.text('Complete verification'));
      await tester.pumpAndSettle();
      expect(engine.inputs, [ProtonEngine.success]);
      expect(engine.requests.where((r) => r['action'] == 'verify').single, {
        'action': 'verify',
        'job_id': 'synthetic-job',
        'challenge_id': 'synthetic-challenge',
      });
      expect(find.textContaining('synthetic-private'), findsNothing);
      await tester.tap(find.text('Free server'));
      await tester.pumpAndSettle();
      expect(changed, 1);
      expect(app.activeProfile.chainExit, before);
      expect(engine.saves, 0);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  _protonWidgetTest(
    'password and TOTP use sensitive input only and clear native input controls',
    (tester) async {
      final engine = ProtonEngine();
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('Sign in to Proton'));
      await tester.pumpAndSettle();
      final fields = find.byType(TextField);
      await tester.enterText(fields.at(0), 'synthetic-private-user');
      await tester.enterText(fields.at(1), 'synthetic-private-password');
      expect(tester.widget<TextField>(fields.at(1)).obscureText, isTrue);
      await tester.tap(find.text('Continue'));
      await tester.pumpAndSettle();
      expect(jsonDecode(engine.inputs.single), {
        'username': 'synthetic-private-user',
        'password': 'synthetic-private-password',
      });
      expect(
        engine.requests.any(
          (r) => r.containsKey('password') || r.containsKey('username'),
        ),
        isFalse,
      );
      expect(find.text('synthetic-private-password'), findsNothing);
      await tester.enterText(find.byType(TextField), '123456');
      await tester.tap(find.text('Continue'));
      await tester.pumpAndSettle();
      expect(engine.inputs.last, '123456');
      expect(engine.requests.last, {
        'action': 'totp',
        'job_id': 'synthetic-job',
      });
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  _protonWidgetTest(
    'closing the native puzzle cancels exactly its authentication job',
    (tester) async {
      final engine = ProtonEngine()..cancelPuzzle = true;
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('Continue as guest'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Complete verification'));
      await tester.pumpAndSettle();
      expect(engine.requests.last, {
        'action': 'cancel',
        'job_id': 'synthetic-job',
      });
      expect(engine.requests.any((r) => r['action'] == 'verify'), isFalse);
      await tester.pumpWidget(const SizedBox.shrink());
      expect(engine.nativeCancelled, isTrue);
    },
  );

  _protonWidgetTest(
    'short TOTP stays editable without submitting an interaction',
    (tester) async {
      final engine = ProtonEngine()
        ..job = {
          'id': 'synthetic-job',
          'state': 'waiting_totp',
          'phase': 'totp',
        };
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), '12345');
      await tester.tap(find.text('Continue'));
      await tester.pumpAndSettle();
      expect(engine.requests.where((r) => r['action'] == 'totp'), isEmpty);
      expect(find.byType(TextField), findsOneWidget);
      expect(find.text('12345'), findsOneWidget);
      expect(find.widgetWithText(TextButton, 'Cancel'), findsOneWidget);
      await tester.enterText(find.byType(TextField), '123456');
      await tester.tap(find.text('Continue'));
      await tester.pumpAndSettle();
      expect(engine.inputs.single, '123456');
      expect(find.text('Free server'), findsOneWidget);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  _protonWidgetTest(
    'an interaction rejection preserves the job, profiles and polling',
    (tester) async {
      final engine = RejectedTotpEngine()
        ..job = {
          'id': 'synthetic-job',
          'state': 'waiting_totp',
          'phase': 'totp',
        }
        ..profiles = [
          {
            'id': 'saved-proton',
            'name': 'Saved Proton',
            'node_name': 'Free server',
          },
        ];
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), '123456');
      await tester.tap(find.text('Continue'));
      await tester.pumpAndSettle();
      expect(find.byType(TextField), findsOneWidget);
      expect(find.text('Saved Proton'), findsOneWidget);
      expect(find.widgetWithText(TextButton, 'Cancel'), findsOneWidget);
      expect(
        find.text('Proton setup failed. Retry when you are ready.'),
        findsOneWidget,
      );
      final before = engine.requests.where((r) => r['action'] == 'get').length;
      await tester.pump(const Duration(seconds: 2));
      await tester.pumpAndSettle();
      expect(
        engine.requests.where((r) => r['action'] == 'get').length,
        before + 1,
      );
      await tester.enterText(find.byType(TextField), '654321');
      await tester.tap(find.text('Continue'));
      await tester.pumpAndSettle();
      expect(find.byType(TextField), findsNothing);
      expect(find.text('Choose a server to finish setup.'), findsOneWidget);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  _protonWidgetTest(
    'a missing native job releases stale controls after a restart',
    (tester) async {
      final engine = DelayedProtonEngine()
        ..job = {
          'id': 'synthetic-job',
          'state': 'waiting_totp',
          'phase': 'totp',
        };
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      engine.delayedAction = 'get';
      await tester.pump(const Duration(seconds: 2));
      engine.reply.complete(
        ProtonVpnResult(
          metadata: const {
            'job': null,
            'profiles': [],
            'error': 'job_not_found',
          },
        ),
      );
      await tester.pumpAndSettle();
      expect(find.byType(TextField), findsNothing);
      expect(find.widgetWithText(TextButton, 'Cancel'), findsNothing);
      await tester.tap(find.text('Continue as guest'));
      await tester.pumpAndSettle();
      expect(
        engine.requests.where((r) => r['action'] == 'guest'),
        hasLength(1),
      );
      expect(find.text('Verification required'), findsOneWidget);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  for (final completed in [false, true]) {
    _protonWidgetTest(
      'leaving during launch ${completed ? 'preserves a completed job' : 'cancels a late live job'}',
      (tester) async {
        final engine = DelayedProtonEngine()..delayedAction = 'guest';
        final app = await hostChain(tester, engine);
        await tester.pumpWidget(
          workflowHost(
            app,
            home: Scaffold(
              body: SingleChildScrollView(
                child: ProtonVpnPanel(controller: app, onChanged: () {}),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        await tester.tap(find.text('Continue as guest'));
        await tester.pump();
        expect(
          engine.requests.where((r) => r['action'] == 'guest'),
          hasLength(1),
        );
        await tester.pumpWidget(const SizedBox.shrink());
        engine.job = {
          'id': 'late-job',
          'state': completed ? 'completed' : 'waiting_node',
          'phase': completed ? 'complete' : 'directory',
        };
        final handoff = Uint8List.fromList([1, 2, 3]);
        engine.reply.complete(
          ProtonVpnResult(
            metadata: {'job': engine.job, 'profiles': <Object?>[]},
            sensitiveHandoff: handoff,
          ),
        );
        await tester.pumpAndSettle();
        expect(
          engine.requests.where(
            (r) => r['action'] == 'cancel' && r['job_id'] == 'late-job',
          ),
          hasLength(completed ? 0 : 1),
        );
        expect(handoff, [0, 0, 0]);
        expect(tester.takeException(), isNull);
      },
    );
  }

  _protonWidgetTest(
    'a late poll does not cancel an already cancelled job twice',
    (tester) async {
      final engine = DelayedProtonEngine()
        ..job = {
          'id': 'synthetic-job',
          'state': 'waiting_totp',
          'phase': 'totp',
        };
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final snapshot = Map<Object?, Object?>.of(engine.job);
      engine.delayedAction = 'get';
      await tester.pump(const Duration(seconds: 2));
      await tester.pumpWidget(const SizedBox.shrink());
      engine.reply.complete(
        ProtonVpnResult(metadata: {'job': snapshot, 'profiles': []}),
      );
      await tester.pumpAndSettle();
      expect(
        engine.requests.where(
          (r) => r['action'] == 'cancel' && r['job_id'] == 'synthetic-job',
        ),
        hasLength(1),
      );
      expect(tester.takeException(), isNull);
    },
  );

  _protonWidgetTest(
    'asynchronous logout refreshes the library only after its job completes',
    (tester) async {
      final engine = PendingLogoutEngine();
      final app = await hostChain(tester, engine);
      final key = GlobalKey<ProtonVpnPanelState>();
      var changed = 0;
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(
                key: key,
                controller: app,
                onChanged: () => changed++,
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final before = app.chainLibraryRevision;
      await key.currentState!.logout('proton-profile');
      await tester.pump();
      expect(changed, 0);
      expect(app.chainLibraryRevision, before);
      engine.job = {
        'id': 'logout-job',
        'state': 'completed',
        'phase': 'complete',
        'profile_id': null,
      };
      await tester.pump(const Duration(seconds: 2));
      await tester.pumpAndSettle();
      expect(changed, 1);
      expect(app.chainLibraryRevision, before + 1);
      await key.currentState!.logout('proton-profile');
      engine.job = {
        'id': 'logout-job',
        'state': 'completed',
        'phase': 'complete',
        'profile_id': null,
      };
      await tester.pump(const Duration(seconds: 2));
      await tester.pumpAndSettle();
      expect(changed, 1);
      expect(app.chainLibraryRevision, before + 1);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  for (final failure in [
    'api_timeout',
    'api_transport_failed',
    'api_tls_failed',
  ]) {
    _protonWidgetTest(
      '$failure points to the current network and VPN',
      (tester) async {
        final engine = ProtonEngine()
          ..job = {
            'id': 'failed-job',
            'state': 'failed',
            'phase': 'directory',
            'failure': failure,
          };
        final app = await hostChain(tester, engine);
        await tester.pumpWidget(
          workflowHost(
            app,
            home: Scaffold(
              body: SingleChildScrollView(
                child: ProtonVpnPanel(controller: app, onChanged: () {}),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        expect(find.text(app.strings.proton('network')), findsOneWidget);
        expect(find.text('Failed step: Load servers'), findsOneWidget);
        expect(find.textContaining('Check your WARP account'), findsNothing);
        await tester.pumpWidget(const SizedBox.shrink());
      },
      platform: TargetPlatform.android,
    );
  }

  _protonWidgetTest(
    'session refusal shows the failed step without opening captcha',
    (tester) async {
      final engine = ProtonEngine()
        ..job = {
          'id': 'failed-job',
          'state': 'failed',
          'phase': 'credentialless',
          'failure': 'session_not_allowed',
          'api_code': 8004,
          'http_status': 422,
          'server_error': 'synthetic-private-error',
        };
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('API 8004 · HTTP 422'), findsOneWidget);
      expect(
        find.text('Proton does not allow this client to create a session.'),
        findsOneWidget,
      );
      expect(find.text('Failed step: Create a guest account'), findsOneWidget);
      expect(find.text('Verification required'), findsNothing);
      expect(engine.requests.any((r) => r['action'] == 'handoff'), isFalse);
      expect(find.textContaining('synthetic-private'), findsNothing);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    platform: TargetPlatform.android,
  );

  _protonWidgetTest(
    'local forget requires confirmation and refreshes on completion',
    (tester) async {
      final engine = ProtonEngine()
        ..profiles = [
          {
            'id': 'proton-profile',
            'name': 'Saved Proton',
            'node_name': 'Free server',
            'needs_login': true,
          },
        ];
      final app = await hostChain(tester, engine);
      var changed = 0;
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(
                controller: app,
                onChanged: () => changed++,
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('Remove account'));
      await tester.pumpAndSettle();
      expect(
        find.textContaining('Forget locally leaves remote sessions unchanged'),
        findsOneWidget,
      );
      await tester.tap(find.text('Cancel'));
      await tester.pumpAndSettle();
      expect(engine.requests.where((r) => r['action'] == 'forget'), isEmpty);
      await tester.tap(find.text('Remove account'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(TextButton, 'Forget locally'));
      await tester.pumpAndSettle();
      expect(engine.requests.last, {
        'action': 'forget',
        'profile_id': 'proton-profile',
      });
      expect(engine.inputs, isEmpty);
      expect(changed, 1);
      expect(find.text('Saved Proton'), findsNothing);
      expect(
        find.text('The Proton account was removed from this device.'),
        findsOneWidget,
      );
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  for (final closeFromEngine in [false, true]) {
    _protonWidgetTest(
      'pending native puzzle closes on ${closeFromEngine ? 'terminal poll' : 'Cancel'} and ignores a late answer',
      (tester) async {
        final engine = PendingPuzzleEngine();
        final app = await hostChain(tester, engine);
        await tester.pumpWidget(
          workflowHost(
            app,
            home: Scaffold(
              body: SingleChildScrollView(
                child: ProtonVpnPanel(controller: app, onChanged: () {}),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        await tester.tap(find.text('Continue as guest'));
        await tester.pumpAndSettle();
        await tester.tap(find.text('Complete verification'));
        await tester.pump();
        expect(
          tester
              .widget<OutlinedButton>(
                find.widgetWithText(OutlinedButton, 'Continue as guest'),
              )
              .onPressed,
          isNull,
        );
        if (closeFromEngine) {
          engine.job = {
            'id': 'synthetic-job',
            'state': 'failed',
            'phase': 'cleanup',
            'failure': 'verification_expired',
          };
          await tester.pump(const Duration(seconds: 2));
        } else {
          await tester.tap(find.widgetWithText(TextButton, 'Cancel'));
        }
        await tester.pump();
        expect(engine.nativeCloses, 1);
        expect(
          tester
              .widget<OutlinedButton>(
                find.widgetWithText(OutlinedButton, 'Continue as guest'),
              )
              .onPressed,
          isNotNull,
        );
        expect(
          engine.requests.where((r) => r['action'] == 'cancel').length,
          closeFromEngine ? 0 : 1,
        );
        await tester.tap(find.text('Continue as guest'));
        await tester.pump();
        engine.answer.complete(
          Uint8List.fromList(utf8.encode(ProtonEngine.success)),
        );
        await tester.pump();
        expect(engine.requests.where((r) => r['action'] == 'verify'), isEmpty);
        await tester.pumpWidget(const SizedBox.shrink());
      },
    );
  }

  _protonWidgetTest(
    'cancelled cleanup uncertainty stays visible without raw errors',
    (tester) async {
      final engine = ProtonEngine()
        ..job = {
          'id': 'cancelled-job',
          'state': 'cancelled',
          'phase': 'cleanup',
          'failure': 'cleanup_failed_remote_state_unknown',
          'server_error': 'synthetic-private-error',
        };
      final app = await hostChain(tester, engine);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ProtonVpnPanel(controller: app, onChanged: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(
        find.text(
          'Cancelled on this device. Remote session revocation could not be confirmed.',
        ),
        findsOneWidget,
      );
      expect(find.textContaining('synthetic-private'), findsNothing);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );
}
