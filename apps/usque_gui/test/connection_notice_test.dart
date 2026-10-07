import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/services/platform_shell_bridge.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/connection_notice_tracker.dart';

import 'app_test.dart' show FakeEngineClient;

EngineSnapshot _snapshot(ConnectionPhase phase, {String? killSwitch}) =>
    EngineSnapshot(phase: phase, killSwitchState: killSwitch);

class _OutputsEngine extends FakeEngineClient {
  final edits = <List<String>>[];

  @override
  Future<EngineCapabilities?> getCapabilities() async =>
      const EngineCapabilities(
        networkSettingsApplication: true,
        automaticEndpoints: true,
      );

  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) {
    edits.add(List.of(changedFields));
    return super.saveNetworkSettings(
      operationId,
      accountId,
      values,
      changedFields,
    );
  }
}

void main() {
  group('ConnectionNoticeTracker', () {
    late List<ConnectionNotice> notices;
    late ConnectionNoticeTracker tracker;

    setUp(() {
      notices = <ConnectionNotice>[];
      tracker = ConnectionNoticeTracker(onNotice: notices.add);
    });

    tearDown(() => tracker.dispose());

    void phase(ConnectionPhase value, {String? killSwitch, bool armed = true}) {
      tracker.update(
        _snapshot(value, killSwitch: killSwitch),
        killSwitchArmed: armed,
      );
    }

    testWidgets('a reconnect that outlasts the delay is reported once', (
      tester,
    ) async {
      phase(ConnectionPhase.connected);
      phase(ConnectionPhase.reconnecting, killSwitch: 'active');
      await tester.pump(const Duration(seconds: 4));
      expect(notices, isEmpty);
      await tester.pump(const Duration(seconds: 2));
      expect(notices.single.kind, ConnectionNoticeKind.interrupted);
      expect(notices.single.killSwitchBlocking, isTrue);

      phase(ConnectionPhase.connected, killSwitch: 'active');
      expect(notices.last.kind, ConnectionNoticeKind.restored);
      expect(notices, hasLength(2));
    });

    testWidgets('a short reconnect and its recovery stay silent', (
      tester,
    ) async {
      phase(ConnectionPhase.connected);
      phase(ConnectionPhase.reconnecting);
      await tester.pump(const Duration(seconds: 2));
      phase(ConnectionPhase.connected);
      await tester.pump(const Duration(seconds: 10));
      expect(notices, isEmpty);
    });

    testWidgets('a user disconnect is silent and cancels a pending notice', (
      tester,
    ) async {
      phase(ConnectionPhase.connected);
      phase(ConnectionPhase.reconnecting);
      phase(ConnectionPhase.disconnecting);
      phase(ConnectionPhase.disconnected);
      await tester.pump(const Duration(seconds: 10));
      expect(notices, isEmpty);
    });

    test('errors are reported immediately with the Kill Switch state', () {
      phase(ConnectionPhase.preparing);
      phase(ConnectionPhase.error, killSwitch: 'active');
      expect(notices.single.kind, ConnectionNoticeKind.failed);
      expect(notices.single.killSwitchBlocking, isTrue);

      phase(ConnectionPhase.error, killSwitch: 'active');
      expect(notices, hasLength(1));
    });

    test('Kill Switch blocking requires the armed tunnel preference', () {
      phase(ConnectionPhase.connected, armed: false);
      phase(ConnectionPhase.error, killSwitch: 'active', armed: false);
      expect(notices.single.killSwitchBlocking, isFalse);
    });

    test('the first snapshot after start-up is not a change', () {
      phase(ConnectionPhase.error);
      expect(notices, isEmpty);
    });
  });

  test('tray badges follow the connection presentation', () {
    expect(trayBadge(ConnectionPhase.disconnected), 'idle');
    expect(trayBadge(ConnectionPhase.preparing), 'busy');
    expect(trayBadge(ConnectionPhase.connectingH3), 'busy');
    expect(trayBadge(ConnectionPhase.disconnecting), 'busy');
    expect(trayBadge(ConnectionPhase.connected), 'connected');
    expect(trayBadge(ConnectionPhase.degraded), 'warning');
    expect(trayBadge(ConnectionPhase.reconnecting), 'warning');
    expect(trayBadge(ConnectionPhase.error), 'error');
  });

  test('notification copy is fixed and appends the Kill Switch line', () {
    final strings = AppStrings(LocalePreference.simplifiedChinese);
    final (title, body, level) = connectionNoticeText(
      strings,
      const ConnectionNotice(
        ConnectionNoticeKind.failed,
        killSwitchBlocking: true,
      ),
    );
    expect(title, strings.get('error'));
    expect(
      body,
      '${strings.get('notice_connection_failed')}\n'
      '${strings.get('notice_kill_switch_blocking')}',
    );
    expect(level, 'error');

    final restored = connectionNoticeText(
      strings,
      const ConnectionNotice(
        ConnectionNoticeKind.restored,
        killSwitchBlocking: true,
      ),
    );
    expect(restored.$2, strings.get('notice_connection_restored'));
    expect(restored.$3, 'info');
  });

  group('tray output shortcuts', () {
    Future<(AppController, _OutputsEngine)> controller({
      required bool http,
    }) async {
      SharedPreferences.setMockInitialValues({'update_checks_enabled': false});
      final engine = _OutputsEngine()..legacyProfilesImported = true;
      final profile = UsqueProfile.defaultProfile();
      engine.storedProfiles = [
        profile.copyWith(
          frontends: profile.frontends.copyWith(tunnel: false, http: http),
          proxy: profile.proxy.copyWith(systemProxy: false),
        ),
      ];
      final app = AppController(engine);
      addTearDown(app.dispose);
      await app.initialize();
      return (app, engine);
    }

    test('TUN flips only its own field through the apply lifecycle', () async {
      final (app, engine) = await controller(http: true);
      expect(await toggleTunnelShortcut(app), isTrue);
      expect(engine.edits.single, ['frontends.tunnel']);
      expect(app.activeProfile.frontends.tunnel, isTrue);
    });

    test('system proxy cannot be enabled without the HTTP proxy', () async {
      final (app, engine) = await controller(http: false);
      expect(await toggleSystemProxyShortcut(app), isFalse);
      expect(engine.edits, isEmpty);
    });

    test('system proxy flips only its own field', () async {
      final (app, engine) = await controller(http: true);
      expect(await toggleSystemProxyShortcut(app), isTrue);
      expect(engine.edits.single, ['proxy.system_proxy']);
      expect(app.activeProfile.proxy.systemProxy, isTrue);
    });

    test('shortcuts do nothing while a connection is changing', () async {
      final (app, engine) = await controller(http: true);
      app.snapshot = const EngineSnapshot(phase: ConnectionPhase.preparing);
      expect(app.networkShortcutsLocked, isTrue);
      expect(await toggleTunnelShortcut(app), isFalse);
      expect(engine.edits, isEmpty);
    });
  });
}
