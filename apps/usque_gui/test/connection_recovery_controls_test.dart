import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/connection_presentation.dart';
import 'package:usque/core/user_facing_errors.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/home_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/services/platform_shell_bridge.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/connection_ring.dart';

import 'app_test.dart' show FakeEngineClient;

class _RecoveryControlsEngine extends FakeEngineClient {
  final StreamController<EngineSnapshotEvent> events =
      StreamController<EngineSnapshotEvent>.broadcast();
  Completer<EngineSnapshot>? pendingRetry;
  Completer<EngineSnapshot>? pendingDisconnect;
  Completer<void>? saveBlocker;
  Object? disconnectError;
  Object? forcedRetryError;
  EngineSnapshot? snapshotReply;
  Completer<EngineSnapshot>? pendingSnapshot;
  int disconnectCalls = 0;
  int retryCalls = 0;

  @override
  bool get supportsSnapshotEvents => true;

  @override
  Stream<EngineSnapshotEvent> get snapshotEvents => events.stream;

  @override
  Future<void> upsertProfile(UsqueProfile profile) async {
    final blocker = saveBlocker;
    if (blocker != null) await blocker.future;
    await super.upsertProfile(profile);
  }

  @override
  Future<EngineSnapshot> snapshot() async {
    final pending = pendingSnapshot;
    if (pending != null) return pending.future;
    final reply = snapshotReply;
    if (reply != null) return reply;
    return super.snapshot();
  }

  @override
  Future<EngineSnapshot> disconnect() async {
    disconnectCalls += 1;
    final error = disconnectError;
    if (error != null) throw error;
    final pending = pendingDisconnect;
    if (pending != null) return pending.future;
    return const EngineSnapshot(phase: ConnectionPhase.disconnected);
  }

  @override
  Future<EngineSnapshot> retry() async {
    retryCalls += 1;
    final error = forcedRetryError;
    if (error != null) throw error;
    final pending = pendingRetry;
    if (pending != null) return pending.future;
    return super.retry();
  }

  void emit(EngineSnapshot snapshot) =>
      events.add(EngineSnapshotEvent(snapshot: snapshot));

  Future<AppController> openController() async {
    final app = AppController(this);
    addTearDown(() async {
      app.dispose();
      final save = saveBlocker;
      if (save != null && !save.isCompleted) save.complete();
      for (final pending in [
        pendingConnect,
        pendingRetry,
        pendingDisconnect,
        pendingSnapshot,
      ]) {
        if (pending != null && !pending.isCompleted) {
          pending.complete(
            const EngineSnapshot(phase: ConnectionPhase.disconnected),
          );
        }
      }
      await events.close();
    });
    await app.initialize();
    return app;
  }
}

Future<void> _pumpHome(WidgetTester tester, AppController app) =>
    tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: HomeScreen(controller: app),
      ),
    );

const _trayChannel = MethodChannel('io.github.georgexie2333.usque/engine');

List<Map<Object?, Object?>> _attachTray(AppController app) {
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  final updates = <Map<Object?, Object?>>[];
  messenger.setMockMethodCallHandler(_trayChannel, (call) async {
    if (call.method == 'updateTrayState') {
      updates.add(Map<Object?, Object?>.from(call.arguments as Map));
    }
    return null;
  });
  final bridge = PlatformShellBridge(app);
  addTearDown(() {
    bridge.dispose();
    messenger.setMockMethodCallHandler(_trayChannel, null);
  });
  return updates;
}

Future<void> _toggleTray() => TestDefaultBinaryMessengerBinding
    .instance
    .defaultBinaryMessenger
    .handlePlatformMessage(
      _trayChannel.name,
      const StandardMethodCodec().encodeMethodCall(
        const MethodCall('trayCommand', 'toggle'),
      ),
      (_) {},
    );

void main() {
  setUp(() {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
  });

  testWidgets(
    'home exposes Disconnect when error retains active Kill Switch, except when recovery is blocked',
    (tester) async {
      final engine = _RecoveryControlsEngine();
      final controller = await engine.openController();

      await _pumpHome(tester, controller);

      engine.emit(
        const EngineSnapshot(
          phase: ConnectionPhase.error,
          killSwitchState: 'active',
          errorCode: 'ENDPOINT_UNREACHABLE',
          warning: 'network error',
        ),
      );
      await tester.pumpAndSettle();

      final disconnectButton = find.byKey(
        const ValueKey('home-error-disconnect'),
      );
      expect(disconnectButton, findsOneWidget);
      await tester.tap(disconnectButton);
      await tester.pumpAndSettle();
      expect(engine.disconnectCalls, 1);
      expect(controller.snapshot.phase, ConnectionPhase.disconnected);

      // Recovery-blocked state keeps the disconnect button hidden.
      engine.emit(
        const EngineSnapshot(
          phase: ConnectionPhase.error,
          killSwitchState: 'active',
          errorCode: 'WINDOWS_RECOVERY_BLOCKED',
          warning: 'blocked',
        ),
      );
      await tester.pumpAndSettle();
      expect(disconnectButton, findsNothing);
    },
  );

  test(
    'disconnect failure preserves Kill Switch, Lockdown and Always-on state',
    () async {
      final engine = _RecoveryControlsEngine()
        ..disconnectError = const EngineException(
          'DISCONNECT_CLEANUP_FAILED',
          'cleanup failed',
        );
      final controller = await engine.openController();

      controller.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.connected,
        killSwitchState: 'active',
        platformLockdown: true,
        alwaysOn: true,
      );

      await controller.disconnect();
      expect(controller.snapshot.phase, ConnectionPhase.error);
      expect(controller.snapshot.killSwitchState, 'active');
      expect(controller.snapshot.platformLockdown, isTrue);
      expect(controller.snapshot.alwaysOn, isTrue);
      expect(controller.canDisconnectProtectedError, isTrue);
    },
  );

  testWidgets(
    'cancel while reconnecting disconnects directly without opening identity dialog',
    (tester) async {
      final engine = _RecoveryControlsEngine()
        ..storedIdentityStatuses = <String, ProfileIdentityStatus>{
          UsqueProfile.defaultProfileId: const ProfileIdentityStatus(
            state: ProfileIdentityState.missing,
          ),
        };
      final controller = await engine.openController();

      controller.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.reconnecting,
      );
      await _pumpHome(tester, controller);
      await tester.pump();

      await tester.tap(find.byType(ConnectionRing));
      await tester.pumpAndSettle();
      expect(engine.disconnectCalls, 1);
      expect(find.byType(AlertDialog), findsNothing);
    },
  );

  test(
    'retry from error stays cancellable while waiting and preserves protection state',
    () async {
      final pending = Completer<EngineSnapshot>();
      final engine = _RecoveryControlsEngine()..pendingRetry = pending;
      final controller = await engine.openController();

      controller.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.error,
        killSwitchState: 'active',
        platformLockdown: true,
        alwaysOn: true,
        errorCode: 'ENDPOINT_UNREACHABLE',
        warning: 'unreachable',
      );

      final retryFuture = controller.retry();
      await Future<void>.delayed(Duration.zero);

      expect(controller.snapshot.phase, ConnectionPhase.preparing);
      expect(controller.snapshot.killSwitchState, 'active');
      expect(controller.snapshot.platformLockdown, isTrue);
      expect(controller.snapshot.alwaysOn, isTrue);
      expect(controller.connectionActionEnabled, isTrue);

      await controller.connectOrDisconnect();
      expect(engine.disconnectCalls, 1);
      expect(controller.snapshot.phase, ConnectionPhase.disconnected);

      pending.complete(const EngineSnapshot(phase: ConnectionPhase.connected));
      await retryFuture;
      expect(controller.snapshot.phase, ConnectionPhase.disconnected);
    },
  );

  test(
    'dismissed connection error stays hidden across identical 1s snapshot events and returns on new errors',
    () async {
      final engine = _RecoveryControlsEngine();
      final controller = await engine.openController();

      const firstError = EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'ENDPOINT_UNREACHABLE',
        warning: 'endpoint unreachable',
      );
      engine.emit(firstError);
      await Future<void>.delayed(Duration.zero);
      expect(controller.lastError, isNotNull);

      controller.clearError();
      expect(controller.lastError, isNull);

      // Identical periodic event and an event with only a fresh quality sample
      // timestamp must not resurrect the dismissed banner.
      engine.emit(firstError);
      engine.emit(
        EngineSnapshot(
          phase: ConnectionPhase.error,
          errorCode: 'ENDPOINT_UNREACHABLE',
          warning: 'endpoint unreachable',
          networkQuality: NetworkQualitySnapshot(
            sampledAt: DateTime.utc(2026, 11, 22, 12, 0, 1),
          ),
        ),
      );
      await Future<void>.delayed(Duration.zero);
      expect(controller.lastError, isNull);

      // A different error surfaces again.
      engine.emit(
        const EngineSnapshot(
          phase: ConnectionPhase.error,
          errorCode: 'AUTHENTICATION_FAILED',
          warning: 'auth failed',
        ),
      );
      await Future<void>.delayed(Duration.zero);
      expect(
        controller.lastError,
        controller.strings.get('diag_fix_refresh_or_replace_identity'),
      );
    },
  );

  test(
    'killSwitchStatusKey, error code mapping, and disconnecting presentation match contracts',
    () {
      final strings = AppStrings(LocalePreference.english);
      final expected = strings.get('diag_fix_refresh_or_replace_identity');
      for (final code in const <String>[
        'AUTHENTICATION_FAILED',
        'AUTHENTICATIONFAILED',
        'INVALID_STORED_IDENTITY',
        'MISSING_CREDENTIAL',
        'MISSINGCREDENTIAL',
      ]) {
        expect(userFacingFailure(strings, code: code), expected, reason: code);
      }

      final proxyOnly = UsqueProfile.defaultProfile().copyWith(
        killSwitch: false,
        frontends: const FrontendSettings(
          tunnel: false,
          socks5: true,
          http: true,
        ),
      );
      expect(
        killSwitchStatusKey(
          profile: proxyOnly,
          snapshot: const EngineSnapshot(
            phase: ConnectionPhase.error,
            killSwitchState: 'active',
          ),
        ),
        'ks_active',
      );
      expect(
        killSwitchStatusKey(
          profile: proxyOnly,
          snapshot: const EngineSnapshot(
            phase: ConnectionPhase.error,
            killSwitchState: 'error',
          ),
        ),
        'ks_error',
      );
      expect(
        ConnectionPresentation.of(ConnectionPhase.disconnecting).actionKey,
        'disconnecting',
      );
    },
  );

  testWidgets(
    'retry stays cancellable when a stale error event arrives during pending profile save',
    (tester) async {
      final saveBlocker = Completer<void>();
      final engine = _RecoveryControlsEngine()..saveBlocker = saveBlocker;
      final controller = await engine.openController();

      await _pumpHome(tester, controller);
      const staleError = EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'ENDPOINT_UNREACHABLE',
        warning: 'stale error',
      );
      engine.emit(staleError);
      await tester.pumpAndSettle();

      // Queue a slow profile write, then start retry.
      controller.updateProfile(controller.activeProfile.copyWith(mtu: 1320));
      final retryFuture = controller.retry();
      await tester.pump();

      // A stale 1s error snapshot arrives before the write finishes.
      engine.emit(staleError);
      await tester.pump();
      expect(controller.connectionActionEnabled, isTrue);
      expect(find.text('Cancel'), findsOneWidget);

      await tester.tap(find.byType(ConnectionRing));
      await tester.pump();
      expect(engine.disconnectCalls, 1);

      saveBlocker.complete();
      await retryFuture;
      await tester.pumpAndSettle();
      expect(engine.retryCalls, 0);
    },
  );

  testWidgets(
    'home exposes Disconnect when error retains an Android TUN interface even with Kill Switch inactive',
    (tester) async {
      final engine = _RecoveryControlsEngine();
      final controller = await engine.openController();

      await _pumpHome(tester, controller);

      final retainedTunError = EngineSnapshot.fromMap(const <Object?, Object?>{
        'phase': 'error',
        'kill_switch_state': 'inactive',
        'tun_interface_present': true,
        'error_code': 'ENDPOINT_UNREACHABLE',
        'warning': 'error while holding tun',
      });
      engine.emit(retainedTunError);
      await tester.pumpAndSettle();

      expect(controller.canDisconnectProtectedError, isTrue);
      final disconnectButton = find.byKey(
        const ValueKey('home-error-disconnect'),
      );
      expect(disconnectButton, findsOneWidget);
      await tester.tap(disconnectButton);
      await tester.pumpAndSettle();
      expect(engine.disconnectCalls, 1);
    },
  );

  for (final error in const [
    EngineException('VPN_PERMISSION_DENIED', 'denied'),
    EngineException('PROFILE_STORE_FAILED', 'Could not read saved account'),
    EngineException('VPN_PERMISSION_LAUNCH_FAILED', 'permission precheck'),
    EngineException('VPN_PERMISSION_IN_PROGRESS', 'permission precheck'),
  ]) {
    test(
      'Android retry preserves the running proxy after ${error.code}',
      () async {
        const running = EngineSnapshot(
          phase: ConnectionPhase.connected,
          transport: 'h3',
        );
        final engine = _RecoveryControlsEngine()
          ..forcedRetryError = error
          ..snapshotReply = running;
        final app = await engine.openController();
        final denied = error.code == 'VPN_PERMISSION_DENIED';
        app.snapshot = denied
            ? const EngineSnapshot(
                phase: ConnectionPhase.degraded,
                transport: 'h3',
              )
            : running;

        await app.retry();
        await Future<void>.delayed(Duration.zero);
        expect(app.snapshot, running);
        expect(app.lastError, denied ? isNull : isNotNull);
      },
    );
  }
  test(
    'dismissing error A then receiving error B and returning to error A shows error A',
    () async {
      final engine = _RecoveryControlsEngine();
      final controller = await engine.openController();

      const errorA = EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'AUTHENTICATION_FAILED',
        warning: 'auth failed',
      );
      const errorB = EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'WINDOWS_RECOVERY_FAILED',
        warning: 'recovery failed',
      );
      engine.emit(errorA);
      await Future<void>.delayed(Duration.zero);
      final expectedA = controller.lastError;
      expect(expectedA, isNotNull);

      controller.clearError();
      expect(controller.lastError, isNull);

      engine.emit(errorB);
      await Future<void>.delayed(Duration.zero);
      expect(controller.lastError, isNot(expectedA));

      engine.emit(errorA);
      await Future<void>.delayed(Duration.zero);
      expect(controller.lastError, expectedA);
    },
  );

  for (final (killSwitch, details) in const [
    ('error', 'cleanup failed'),
    ('not_applicable', 'system proxy restore failed'),
  ]) {
    testWidgets('Home retries failed cleanup with Kill Switch $killSwitch', (
      tester,
    ) async {
      final engine = _RecoveryControlsEngine();
      final app = await engine.openController();
      await _pumpHome(tester, app);
      engine.emit(
        EngineSnapshot(
          phase: ConnectionPhase.error,
          killSwitchState: killSwitch,
          errorCode: 'DISCONNECT_CLEANUP_FAILED',
          warning: details,
        ),
      );
      await tester.pumpAndSettle();

      final ring = find.byType(ConnectionRing);
      expect(app.canDisconnectProtectedError, isTrue);
      expect(app.connectionActionDisconnects, isTrue);
      expect(
        tester.widget<ConnectionRing>(ring).actionLabel,
        'Retry disconnecting',
      );
      expect(app.lastError, app.strings.get('disconnect_cleanup_failed'));
      expect(find.byKey(const ValueKey('home-error-disconnect')), findsNothing);
      await tester.tap(ring);
      await tester.pumpAndSettle();
      expect(engine.disconnectCalls, 1);
      expect(engine.retryCalls, 0);

      engine.emit(
        EngineSnapshot(
          phase: ConnectionPhase.error,
          killSwitchState: killSwitch,
          errorCode: 'WINDOWS_RECOVERY_BLOCKED',
          warning: 'recovery blocked',
        ),
      );
      await tester.pumpAndSettle();
      expect(app.canDisconnectProtectedError, isFalse);
      expect(tester.widget<ConnectionRing>(ring).onPressed, isNull);
    });
  }
  test(
    'Android PROFILE_STORE_FAILED during retry preserves an already-running proxy session and reports the error',
    () async {
      final engine = _RecoveryControlsEngine()
        ..forcedRetryError = const EngineException(
          'PROFILE_STORE_FAILED',
          'Could not read saved account',
        )
        ..snapshotReply = const EngineSnapshot(
          phase: ConnectionPhase.connected,
          transport: 'h3',
        );
      final controller = await engine.openController();

      controller.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.connected,
        transport: 'h3',
      );
      await controller.retry();
      await Future<void>.delayed(Duration.zero);

      expect(controller.snapshot.phase, ConnectionPhase.connected);
      expect(controller.lastError, isNotNull);
    },
  );

  testWidgets(
    'tray state and toggle agree during pending retry even if a stale Disconnected event arrives',
    (tester) async {
      final pending = Completer<EngineSnapshot>();
      final engine = _RecoveryControlsEngine()..pendingRetry = pending;
      final controller = await engine.openController();
      final trayUpdates = _attachTray(controller);

      controller.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'ENDPOINT_UNREACHABLE',
        warning: 'error',
      );
      final retryFuture = controller.retry();
      await tester.pump();

      // Simulate a stale Disconnected event arriving while retry RPC is in flight.
      engine.emit(const EngineSnapshot(phase: ConnectionPhase.disconnected));
      await tester.pump();

      expect(controller.hasPendingConnectionStart, isTrue);
      expect(controller.connectionActionDisconnects, isTrue);
      expect(trayUpdates, isNotEmpty);
      expect(trayUpdates.last['connected'], isTrue);

      await _toggleTray();
      await tester.pump();
      expect(engine.disconnectCalls, 1);

      pending.complete(const EngineSnapshot(phase: ConnectionPhase.preparing));
      await retryFuture;
    },
  );

  for (final reply in const [
    EngineSnapshot(phase: ConnectionPhase.preparing),
    EngineSnapshot(phase: ConnectionPhase.connected, transport: 'h3'),
  ]) {
    test(
      'a late ${reply.phase.name} retry reply preserves the newer terminal error',
      () async {
        final pending = Completer<EngineSnapshot>();
        final engine = _RecoveryControlsEngine()..pendingRetry = pending;
        final app = await engine.openController();
        app.snapshot = const EngineSnapshot(
          phase: ConnectionPhase.error,
          errorCode: 'ENDPOINT_UNREACHABLE',
          warning: 'initial error',
        );

        final retry = app.retry();
        await Future<void>.delayed(Duration.zero);
        const failure = EngineSnapshot(
          phase: ConnectionPhase.error,
          errorCode: 'AUTHENTICATION_FAILED',
          warning: 'retry failed',
        );
        engine.emit(failure);
        await Future<void>.delayed(Duration.zero);
        expect(app.snapshot, failure);
        expect(app.lastError, isNotNull);

        engine.snapshotReply = failure;
        pending.complete(reply);
        await retry;
        expect(app.snapshot, failure);
        expect(app.lastError, isNotNull);
      },
    );
  }
  testWidgets(
    'home exposes Disconnect after pure-proxy cleanup failure with killSwitchState=not_applicable',
    (tester) async {
      final engine = _RecoveryControlsEngine();
      final controller = await engine.openController();

      await _pumpHome(tester, controller);

      engine.emit(
        const EngineSnapshot(
          phase: ConnectionPhase.error,
          killSwitchState: 'not_applicable',
          errorCode: 'DISCONNECT_CLEANUP_FAILED',
          warning: 'system proxy restore failed',
        ),
      );
      await tester.pumpAndSettle();

      expect(controller.canDisconnectProtectedError, isTrue);
      expect(controller.connectionActionDisconnects, isTrue);
      final ring = find.byType(ConnectionRing);
      expect(
        tester.widget<ConnectionRing>(ring).actionLabel,
        'Retry disconnecting',
      );
      expect(
        controller.lastError,
        controller.strings.get('disconnect_cleanup_failed'),
      );
      expect(find.byKey(const ValueKey('home-error-disconnect')), findsNothing);
      await tester.tap(ring);
      await tester.pumpAndSettle();
      expect(engine.disconnectCalls, 1);
      expect(engine.retryCalls, 0);
    },
  );

  test(
    'stale Disconnected event during retry does not discard a valid Preparing RPC reply',
    () async {
      final pending = Completer<EngineSnapshot>();
      final engine = _RecoveryControlsEngine()..pendingRetry = pending;
      final controller = await engine.openController();

      controller.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'ENDPOINT_UNREACHABLE',
        warning: 'error',
      );
      final retryFuture = controller.retry();
      await Future<void>.delayed(Duration.zero);

      engine.emit(const EngineSnapshot(phase: ConnectionPhase.disconnected));
      await Future<void>.delayed(Duration.zero);
      expect(controller.snapshot.phase, ConnectionPhase.preparing);

      engine.snapshotReply = const EngineSnapshot(
        phase: ConnectionPhase.preparing,
      );
      pending.complete(const EngineSnapshot(phase: ConnectionPhase.preparing));
      await retryFuture;
      expect(controller.snapshot.phase, ConnectionPhase.preparing);
      expect(controller.connectionActionDisconnects, isTrue);
    },
  );

  test(
    'late Connected RPC reply does not overwrite a newer terminal Error event',
    () async {
      final pending = Completer<EngineSnapshot>();
      final engine = _RecoveryControlsEngine()..pendingRetry = pending;
      final controller = await engine.openController();

      controller.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'ENDPOINT_UNREACHABLE',
        warning: 'initial error',
      );
      final retryFuture = controller.retry();
      await Future<void>.delayed(Duration.zero);

      engine.emit(
        const EngineSnapshot(
          phase: ConnectionPhase.error,
          errorCode: 'AUTHENTICATION_FAILED',
          warning: 'auth failed',
        ),
      );
      await Future<void>.delayed(Duration.zero);
      expect(controller.snapshot.phase, ConnectionPhase.error);
      expect(controller.lastError, isNotNull);

      engine.snapshotReply = controller.snapshot;
      pending.complete(
        const EngineSnapshot(phase: ConnectionPhase.connected, transport: 'h3'),
      );
      await retryFuture;
      expect(controller.snapshot.phase, ConnectionPhase.error);
      expect(controller.snapshot.errorCode, 'AUTHENTICATION_FAILED');
      expect(controller.lastError, isNotNull);
    },
  );

  for (final retry in [false, true]) {
    test(
      'a repeated previous error keeps an accepted ${retry ? 'Retry' : 'Connect'} cancellable',
      () async {
        const previous = EngineSnapshot(
          phase: ConnectionPhase.error,
          errorCode: 'ENDPOINT_UNREACHABLE',
        );
        const preparing = EngineSnapshot(phase: ConnectionPhase.preparing);
        final pending = Completer<EngineSnapshot>();
        final engine = _RecoveryControlsEngine()..snapshotReply = previous;
        if (retry) {
          engine.pendingRetry = pending;
        } else {
          engine.pendingConnect = pending;
        }
        final app = await engine.openController();
        final start = retry ? app.retry() : app.connectOrDisconnect();
        await Future<void>.delayed(Duration.zero);
        engine.emit(previous);
        await Future<void>.delayed(Duration.zero);
        expect(app.snapshot.phase, ConnectionPhase.preparing);
        engine.snapshotReply = preparing;
        pending.complete(preparing);
        await start;
        expect(app.snapshot.phase, ConnectionPhase.preparing);
        await app.connectOrDisconnect();
        expect(engine.disconnectCalls, 1);
      },
    );
  }

  test(
    'an external stop after handshake progress survives the delayed acknowledgement',
    () async {
      const stopped = EngineSnapshot(phase: ConnectionPhase.disconnected);
      final engine = _RecoveryControlsEngine()
        ..pendingRetry = Completer<EngineSnapshot>();
      final app = await engine.openController();
      final start = app.retry();
      await Future<void>.delayed(Duration.zero);
      engine.emit(const EngineSnapshot(phase: ConnectionPhase.connectingH3));
      engine.emit(stopped);
      await Future<void>.delayed(Duration.zero);
      expect(app.snapshot.phase, ConnectionPhase.disconnected);
      expect(app.hasPendingConnectionStart, isFalse);
      expect(app.connectionActionDisconnects, isFalse);
      expect(app.connectionActionEnabled, isTrue);
      engine.snapshotReply = stopped;
      engine.pendingRetry!.complete(
        const EngineSnapshot(phase: ConnectionPhase.preparing),
      );
      await start;
      expect(app.snapshot.phase, ConnectionPhase.disconnected);
      expect(app.connectionActionDisconnects, isFalse);
    },
  );

  test(
    'an authoritative status read takes part in command reconciliation',
    () async {
      const failure = EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'AUTHENTICATION_FAILED',
      );
      final engine = _RecoveryControlsEngine()
        ..pendingRetry = Completer<EngineSnapshot>();
      final app = await engine.openController();
      final start = app.retry();
      await Future<void>.delayed(Duration.zero);
      engine.snapshotReply = failure;
      await app.refreshSnapshot(silent: true);
      expect(app.snapshot.errorCode, failure.errorCode);
      engine.pendingRetry!.complete(
        const EngineSnapshot(phase: ConnectionPhase.connected),
      );
      await start;
      expect(app.snapshot.errorCode, failure.errorCode);
      expect(app.lastError, isNotNull);
    },
  );

  test(
    'a permission error preserves the latest native observation during readback',
    () async {
      const failure = EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'AUTHENTICATION_FAILED',
      );
      final engine = _RecoveryControlsEngine()
        ..snapshotReply = const EngineSnapshot(phase: ConnectionPhase.connected)
        ..pendingRetry = Completer<EngineSnapshot>();
      final app = await engine.openController();
      final start = app.retry();
      await Future<void>.delayed(Duration.zero);
      engine.emit(failure);
      await Future<void>.delayed(Duration.zero);
      engine.pendingSnapshot = Completer<EngineSnapshot>();
      engine.pendingRetry!.completeError(
        const EngineException('VPN_PERMISSION_LAUNCH_FAILED', 'preflight'),
      );
      await start;
      expect(app.snapshot.errorCode, failure.errorCode);
      expect(
        app.lastError,
        userFacingFailure(app.strings, code: 'VPN_PERMISSION_LAUNCH_FAILED'),
      );
      engine.pendingSnapshot!.complete(failure);
      await Future<void>.delayed(Duration.zero);
      final permissionMessage = userFacingFailure(
        app.strings,
        code: 'VPN_PERMISSION_LAUNCH_FAILED',
      );
      expect(app.lastError, permissionMessage);
      engine.emit(failure);
      await Future<void>.delayed(Duration.zero);
      expect(app.lastError, permissionMessage);
      engine.emit(
        const EngineSnapshot(
          phase: ConnectionPhase.error,
          errorCode: 'TLS_FINGERPRINT_UNSUPPORTED',
        ),
      );
      await Future<void>.delayed(Duration.zero);
      expect(
        app.lastError,
        userFacingFailure(app.strings, code: 'TLS_FINGERPRINT_UNSUPPORTED'),
      );
    },
  );

  for (final viaPoll in [false, true]) {
    test(
      'Disconnect reconciles a late clean receipt with a cleanup failure from ${viaPoll ? 'GetStatus' : 'events'}',
      () async {
        const connected = EngineSnapshot(phase: ConnectionPhase.connected);
        const failure = EngineSnapshot(
          phase: ConnectionPhase.error,
          errorCode: 'DISCONNECT_CLEANUP_FAILED',
          killSwitchState: 'not_applicable',
        );
        const stopped = EngineSnapshot(phase: ConnectionPhase.disconnected);
        final engine = _RecoveryControlsEngine()
          ..snapshotReply = connected
          ..pendingDisconnect = Completer<EngineSnapshot>();
        final app = await engine.openController();
        final stop = app.disconnect();
        if (viaPoll) {
          engine.snapshotReply = failure;
          await app.refreshSnapshot(silent: true);
        } else {
          engine.emit(failure);
          await Future<void>.delayed(Duration.zero);
        }
        expect(app.needsDisconnectCleanup, isTrue);
        engine.pendingSnapshot = Completer<EngineSnapshot>();
        engine.pendingDisconnect!.complete(stopped);
        await Future<void>.delayed(Duration.zero);
        expect(app.snapshot.errorCode, failure.errorCode);
        engine.pendingSnapshot!.complete(failure);
        await stop;
        expect(app.needsDisconnectCleanup, isTrue);
        engine.pendingDisconnect = null;
        await app.retry();
        expect(engine.disconnectCalls, 2);
        expect(engine.retryCalls, 0);
        expect(app.snapshot.phase, ConnectionPhase.disconnected);
      },
    );
  }

  test(
    'a newer status event wins over a pending Disconnect confirmation read',
    () async {
      const stopped = EngineSnapshot(phase: ConnectionPhase.disconnected);
      const failure = EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'DISCONNECT_CLEANUP_FAILED',
      );
      final engine = _RecoveryControlsEngine()
        ..snapshotReply = const EngineSnapshot(phase: ConnectionPhase.connected)
        ..pendingDisconnect = Completer<EngineSnapshot>();
      final app = await engine.openController();
      final stop = app.disconnect();
      engine.emit(failure);
      await Future<void>.delayed(Duration.zero);
      engine.pendingSnapshot = Completer<EngineSnapshot>();
      engine.pendingDisconnect!.complete(stopped);
      await Future<void>.delayed(Duration.zero);
      engine.emit(stopped);
      await Future<void>.delayed(Duration.zero);
      engine.pendingSnapshot!.complete(failure);
      await stop;
      expect(app.snapshot.phase, ConnectionPhase.disconnected);
      expect(app.needsDisconnectCleanup, isFalse);
      expect(app.lastError, isNull);
    },
  );

  test(
    'a lost Stop reply without any new event keeps retrying proxy cleanup',
    () async {
      const cleanup = EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'DISCONNECT_CLEANUP_FAILED',
      );
      final engine = _RecoveryControlsEngine()
        ..snapshotReply = cleanup
        ..disconnectError = const EngineException(
          'ENGINE_REQUEST_TIMEOUT',
          'No reply',
        );
      final app = await engine.openController();

      await app.disconnect();
      expect(app.snapshot.errorCode, cleanup.errorCode);
      expect(app.needsDisconnectCleanup, isTrue);
      engine.disconnectError = null;
      await app.retry();
      expect(engine.disconnectCalls, 2);
      expect(engine.retryCalls, 0);
      expect(app.snapshot.phase, ConnectionPhase.disconnected);
    },
  );

  test(
    'same-phase retry replies reconcile changed transport and listeners',
    () async {
      const previous = EngineSnapshot(
        phase: ConnectionPhase.connected,
        transport: 'http3',
        activeListeners: ['127.0.0.1:1080'],
      );
      const replacement = EngineSnapshot(
        phase: ConnectionPhase.connected,
        transport: 'http2',
        activeListeners: ['127.0.0.1:2080'],
      );
      final engine = _RecoveryControlsEngine()
        ..snapshotReply = previous
        ..pendingRetry = Completer<EngineSnapshot>();
      final app = await engine.openController();

      final retry = app.retry();
      await Future<void>.delayed(Duration.zero);
      expect(engine.retryCalls, 1);
      engine.emit(previous);
      await Future<void>.delayed(Duration.zero);
      engine.snapshotReply = replacement;
      engine.pendingRetry!.complete(replacement);
      await retry;
      expect(app.snapshot, replacement);

      // With the opposite delivery order, a failed read must preserve the
      // changed listeners that the event stream already delivered.
      engine.pendingRetry = Completer<EngineSnapshot>();
      final again = app.retry();
      await Future<void>.delayed(Duration.zero);
      engine.emit(previous);
      await Future<void>.delayed(Duration.zero);
      engine.pendingSnapshot = Completer<EngineSnapshot>();
      engine.pendingRetry!.complete(replacement);
      await Future<void>.delayed(Duration.zero);
      engine.pendingSnapshot!.completeError(
        const EngineException('ENGINE_REQUEST_TIMEOUT', 'Read failed'),
      );
      await again;
      expect(app.snapshot, previous);
    },
  );

  test(
    'an unavailable status read after Stop retains an outstanding cleanup error only',
    () async {
      for (final code in [
        'DISCONNECT_CLEANUP_FAILED',
        'AUTHENTICATION_FAILED',
      ]) {
        final previous = EngineSnapshot(
          phase: ConnectionPhase.error,
          errorCode: code,
          killSwitchState: 'active',
        );
        final engine = _RecoveryControlsEngine()
          ..snapshotReply = previous
          ..pendingDisconnect = Completer<EngineSnapshot>();
        final app = await engine.openController();
        final stop = app.disconnect();
        engine.emit(previous);
        await Future<void>.delayed(Duration.zero);
        engine.pendingSnapshot = Completer<EngineSnapshot>();
        engine.pendingDisconnect!.complete(
          const EngineSnapshot(phase: ConnectionPhase.disconnected),
        );
        await Future<void>.delayed(Duration.zero);
        engine.pendingSnapshot!.completeError(
          const EngineException('ENGINE_REQUEST_TIMEOUT', 'read unavailable'),
        );
        await stop;
        expect(app.needsDisconnectCleanup, previous.needsDisconnectCleanup);
        expect(
          app.snapshot.phase,
          previous.needsDisconnectCleanup
              ? ConnectionPhase.error
              : ConnectionPhase.disconnected,
        );
      }
    },
  );

  for (final (observed, rejected, expectedCode) in [
    (
      const EngineSnapshot(phase: ConnectionPhase.disconnected),
      const EngineException('DISCONNECT_CLEANUP_FAILED', 'restore failed'),
      'DISCONNECT_CLEANUP_FAILED',
    ),
    (
      const EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'DISCONNECT_CLEANUP_FAILED',
      ),
      const EngineException('ENGINE_REQUEST_TIMEOUT', 'lost reply'),
      'DISCONNECT_CLEANUP_FAILED',
    ),
    (
      const EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'DISCONNECT_CLEANUP_FAILED',
      ),
      const EngineException('WINDOWS_RECOVERY_BLOCKED', 'recovery blocked'),
      'WINDOWS_RECOVERY_BLOCKED',
    ),
  ]) {
    test(
      'Stop reports $expectedCode when ${rejected.code} follows ${observed.phase.name}',
      () async {
        final engine = _RecoveryControlsEngine()
          ..snapshotReply = const EngineSnapshot(
            phase: ConnectionPhase.connected,
          )
          ..pendingDisconnect = Completer<EngineSnapshot>();
        final app = await engine.openController();
        final stop = app.disconnect();
        engine.emit(observed);
        await Future<void>.delayed(Duration.zero);
        engine.pendingDisconnect!.completeError(rejected);
        await stop;
        expect(app.snapshot.errorCode, expectedCode);
        expect(
          app.needsDisconnectCleanup,
          expectedCode == 'DISCONNECT_CLEANUP_FAILED',
        );
        expect(
          app.connectionActionEnabled,
          expectedCode != 'WINDOWS_RECOVERY_BLOCKED',
        );
      },
    );
  }

  for (final useTray in [false, true]) {
    testWidgets(
      'an external stop clears its connection banner and ${useTray ? 'tray' : 'Home'} starts a new connection before the old reply',
      (tester) async {
        final engine = _RecoveryControlsEngine()
          ..pendingRetry = Completer<EngineSnapshot>();
        final app = await engine.openController();
        final trayUpdates = _attachTray(app);
        await _pumpHome(tester, app);
        final old = app.retry();
        await tester.pump();
        engine.emit(const EngineSnapshot(phase: ConnectionPhase.connectingH3));
        engine.emit(
          const EngineSnapshot(
            phase: ConnectionPhase.error,
            errorCode: 'AUTHENTICATION_FAILED',
          ),
        );
        await tester.pump();
        expect(app.lastError, isNotNull);
        engine.emit(const EngineSnapshot(phase: ConnectionPhase.disconnected));
        await tester.pump();
        final ring = find.byType(ConnectionRing);
        expect(tester.widget<ConnectionRing>(ring).actionLabel, 'Connect');
        expect(app.lastError, isNull);
        expect(app.connectionActionDisconnects, isFalse);
        expect(trayUpdates.last['connected'], isFalse);
        expect(trayUpdates.last['connection_enabled'], isTrue);
        if (useTray) {
          await _toggleTray();
        } else {
          await tester.tap(ring);
        }
        await tester.pump();
        engine.pendingRetry!.complete(
          const EngineSnapshot(phase: ConnectionPhase.preparing),
        );
        await old;
        expect(engine.calls.where((call) => call == 'connect'), hasLength(1));
        expect(engine.disconnectCalls, 0);
        expect(app.snapshot.isConnected, isTrue);
      },
    );
  }

  testWidgets(
    'cancelled macOS proxy restoration retains the Stop action through a legacy status poll',
    (tester) async {
      const permission = EngineException(
        'SYSTEM_PROXY_PERMISSION_CANCELLED',
        'restore permission cancelled',
      );
      final engine = _RecoveryControlsEngine()
        ..snapshotReply = const EngineSnapshot(phase: ConnectionPhase.connected)
        ..disconnectError = permission;
      final app = await engine.openController();
      await _pumpHome(tester, app);
      final ring = find.byType(ConnectionRing);
      await app.disconnect();
      await tester.pump();
      expect(app.lastError, app.strings.get('macos_proxy_permission'));
      expect(
        tester.widget<ConnectionRing>(ring).actionLabel,
        'Retry disconnecting',
      );
      engine.snapshotReply = const EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'PLATFORMSETUPFAILED',
      );
      await app.refreshSnapshot(silent: true);
      await tester.pump();
      expect(
        tester.widget<ConnectionRing>(ring).actionLabel,
        'Retry disconnecting',
      );
      engine.disconnectError = null;
      await tester.tap(ring);
      await tester.pumpAndSettle();
      expect(engine.disconnectCalls, 2);
      expect(engine.retryCalls, 0);
      expect(app.snapshot.phase, ConnectionPhase.disconnected);
      engine.connectError = permission;
      await app.connectOrDisconnect();
      await tester.pumpAndSettle();
      expect(tester.widget<ConnectionRing>(ring).actionLabel, 'Retry');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );
}
