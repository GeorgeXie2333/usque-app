import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/frontend_presentation.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/engine_client.dart';

const _connectedAndroidSnapshot = <Object?, Object?>{
  'phase': 'connected',
  'transport': 'h3',
  'address_family': 'ipv4',
  'active_frontends': <String>['socks5', 'http'],
  'active_listeners': <String>['127.0.0.1:1080', '127.0.0.1:8080'],
  'platform_state_observed': true,
  'vpn_service_state': 'running',
  'vpn_process_state': 'reachable',
  'native_runtime_state': 'running',
  'tun_fd_valid': true,
  'tun_interface_present': true,
  'pending_cleanup': false,
};

const _outputKinds = [
  FrontendKind.tunnel,
  FrontendKind.socks5,
  FrontendKind.http,
];

FrontendPhase? _phaseOf(EngineSnapshot snapshot, FrontendKind kind) => snapshot
    .frontends
    .where((status) => status.kind == kind)
    .firstOrNull
    ?.phase;

List<String> _outputLabels(EngineSnapshot snapshot) => [
  for (final kind in _outputKinds)
    FrontendPresentation.of(
      configured: true,
      connection: snapshot.phase,
      runtime: _phaseOf(snapshot, kind),
    ).labelKey,
];

Future<EngineSnapshot> _readSnapshot(Map<Object?, Object?> value) async {
  const channel = MethodChannel('io.github.georgexie2333.usque/engine');
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  messenger.setMockMethodCallHandler(channel, (call) async => value);
  addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
  return MethodChannelEngineClient().snapshot();
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('reconfigureActiveProfile does not follow up with connect', () async {
    const channel = MethodChannel('io.github.georgexie2333.usque/engine');
    final calls = <String>[];
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (MethodCall call) async {
          calls.add(call.method);
          return null;
        });
    addTearDown(() {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(channel, null);
    });

    final client = MethodChannelEngineClient();
    await client.reconfigureActiveProfile(UsqueProfile.defaultProfile());
    expect(calls, <String>['reconfigureActiveProfile']);
  });

  test(
    'snapshotEvents wraps Android maps as non-null snapshot events',
    () async {
      const events = EventChannel(
        'io.github.georgexie2333.usque/engine_events',
      );
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockStreamHandler(
            events,
            MockStreamHandler.inline(
              onListen: (dynamic arguments, MockStreamHandlerEventSink sink) {
                sink.success(<Object?, Object?>{
                  'phase': 'connected',
                  'transport': 'HTTP/3',
                });
              },
            ),
          );
      addTearDown(() {
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
            .setMockStreamHandler(events, null);
      });

      final client = MethodChannelEngineClient();
      final event = await client.snapshotEvents.first;
      expect(event.snapshot, isNotNull);
      expect(event.snapshot!.phase, ConnectionPhase.connected);
      expect(event.snapshot!.transport, 'HTTP/3');
    },
  );

  for (final method in ['connect', 'retry', 'snapshot', 'disconnect']) {
    test('$method decodes Android runtime outputs from its reply', () async {
      const channel = MethodChannel('io.github.georgexie2333.usque/engine');
      final messenger =
          TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
      messenger.setMockMethodCallHandler(channel, (call) async {
        expect(call.method, method);
        return _connectedAndroidSnapshot;
      });
      addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
      final client = MethodChannelEngineClient();
      final snapshot = await switch (method) {
        'connect' => client.connect(UsqueProfile.defaultProfile()),
        'retry' => client.retry(),
        'disconnect' => client.disconnect(),
        _ => client.snapshot(),
      };
      expect(snapshot.phase, ConnectionPhase.connected);
      expect(snapshot.transport, 'h3');
      expect(snapshot.activeListeners, hasLength(2));
      expect(_outputLabels(snapshot), List.filled(3, 'output_running'));
      // Android reports a shared address list, not per-protocol addresses.
      expect(
        snapshot.frontends.every((status) => status.listeners.isEmpty),
        isTrue,
      );
    });
  }

  test(
    'Android events update outputs when TUN and listeners disappear',
    () async {
      const events = EventChannel(
        'io.github.georgexie2333.usque/engine_events',
      );
      final messenger =
          TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
      messenger.setMockStreamHandler(
        events,
        MockStreamHandler.inline(
          onListen: (dynamic arguments, MockStreamHandlerEventSink sink) {
            sink.success(_connectedAndroidSnapshot);
            sink.success(<Object?, Object?>{
              ..._connectedAndroidSnapshot,
              'tun_fd_valid': false,
              'tun_interface_present': false,
              'active_frontends': <String>['http'],
            });
            sink.success(<Object?, Object?>{'phase': 'connected'});
          },
        ),
      );
      addTearDown(() => messenger.setMockStreamHandler(events, null));
      final snapshots = await MethodChannelEngineClient().snapshotEvents
          .take(3)
          .map((event) => event.snapshot!)
          .toList();
      expect(_outputLabels(snapshots[0]), List.filled(3, 'output_running'));
      expect(_outputLabels(snapshots[1]), [
        'output_waiting',
        'output_waiting',
        'output_running',
      ]);
      expect(_outputLabels(snapshots[2]), List.filled(3, 'output_unknown'));
    },
  );

  for (final active in <List<String>>[
    ['socks5'],
    ['http'],
    [],
  ]) {
    test('proxy-only outputs follow typed runtime kinds: $active', () async {
      final snapshot = await _readSnapshot({
        ..._connectedAndroidSnapshot,
        'tun_fd_valid': false,
        'tun_interface_present': false,
        'active_frontends': active,
        // Addresses and transport IP availability must not imply active outputs.
        'tunnel_ipv4_available': true,
        'tunnel_ipv6_available': true,
      });
      expect(_phaseOf(snapshot, FrontendKind.tunnel), FrontendPhase.disabled);
      for (final kind in [FrontendKind.socks5, FrontendKind.http]) {
        expect(
          _phaseOf(snapshot, kind),
          active.contains(kind.name)
              ? FrontendPhase.active
              : FrontendPhase.disabled,
        );
      }
    });
  }

  for (final entry in {
    'connected': FrontendPhase.active,
    'degraded': FrontendPhase.degraded,
    'reconnecting': FrontendPhase.reconnecting,
    'preparing': FrontendPhase.preparing,
    'connectingH3': FrontendPhase.preparing,
    'connectingH2': FrontendPhase.preparing,
    'disconnecting': FrontendPhase.disabled,
    'disconnected': FrontendPhase.disabled,
    'error': FrontendPhase.error,
  }.entries) {
    test('Android outputs respect connection phase ${entry.key}', () async {
      final snapshot = await _readSnapshot({
        ..._connectedAndroidSnapshot,
        'phase': entry.key,
      });
      expect(
        snapshot.frontends.map((status) => status.phase),
        List.filled(3, entry.value),
      );
    });
  }

  for (final entry in {
    'platform_state_observed': false,
    'vpn_service_state': 'stopped',
    'vpn_process_state': 'unreachable',
    'native_runtime_state': 'stopped',
    'pending_cleanup': true,
    'phase': 'unrecognized',
  }.entries) {
    test(
      'Android does not trust stale outputs with ${entry.key}=${entry.value}',
      () async {
        final snapshot = await _readSnapshot({
          ..._connectedAndroidSnapshot,
          entry.key: entry.value,
        });
        expect(snapshot.frontends, isEmpty);
      },
    );
  }

  for (final key in [
    'platform_state_observed',
    'vpn_service_state',
    'vpn_process_state',
    'native_runtime_state',
  ]) {
    test('Android missing $key does not imply active outputs', () async {
      final map = Map<Object?, Object?>.of(_connectedAndroidSnapshot)
        ..remove(key);
      expect((await _readSnapshot(map)).frontends, isEmpty);
    });
  }

  for (final key in ['tun_fd_valid', 'tun_interface_present']) {
    for (final value in <Object?>[null, 'true', false]) {
      test('Android VPN requires both TUN observations: $key=$value', () async {
        final snapshot = await _readSnapshot({
          ..._connectedAndroidSnapshot,
          key: value,
        });
        expect(
          _phaseOf(snapshot, FrontendKind.tunnel),
          isNot(FrontendPhase.active),
        );
        expect(_phaseOf(snapshot, FrontendKind.socks5), FrontendPhase.active);
        expect(_phaseOf(snapshot, FrontendKind.http), FrontendPhase.active);
      });
    }
  }

  for (final value in <Object?>[
    null,
    'socks5',
    <Object?>['http', 7],
  ]) {
    test('malformed active_frontends stays unknown: $value', () async {
      final snapshot = await _readSnapshot({
        ..._connectedAndroidSnapshot,
        'active_frontends': value,
      });
      expect(_phaseOf(snapshot, FrontendKind.socks5), isNull);
      expect(_phaseOf(snapshot, FrontendKind.http), isNull);
      expect(_phaseOf(snapshot, FrontendKind.tunnel), FrontendPhase.active);
    });
  }

  test('Android ignores unknown kinds and collapses duplicate kinds', () async {
    final snapshot = await _readSnapshot({
      ..._connectedAndroidSnapshot,
      'tun_fd_valid': false,
      'tun_interface_present': false,
      'active_frontends': [
        'socks5',
        'socks5',
        'tunnel',
        'systemProxy',
        'future',
      ],
    });
    expect(snapshot.frontends, hasLength(3));
    expect(_phaseOf(snapshot, FrontendKind.tunnel), FrontendPhase.disabled);
    expect(_phaseOf(snapshot, FrontendKind.socks5), FrontendPhase.active);
    expect(_phaseOf(snapshot, FrontendKind.http), FrontendPhase.disabled);
    expect(_phaseOf(snapshot, FrontendKind.systemProxy), isNull);
  });

  test('explicit structured frontend status remains authoritative', () async {
    final snapshot = await _readSnapshot({
      ..._connectedAndroidSnapshot,
      'frontends': [
        {
          'kind': 'http',
          'phase': 'error',
          'listeners': ['127.0.0.1:9090'],
          'error_code': 'TEST_LISTENER_FAILED',
        },
      ],
    });
    expect(snapshot.frontends, hasLength(1));
    expect(snapshot.frontends.single.kind, FrontendKind.http);
    expect(snapshot.frontends.single.phase, FrontendPhase.error);
    expect(snapshot.frontends.single.listeners, ['127.0.0.1:9090']);
    expect(snapshot.frontends.single.errorCode, 'TEST_LISTENER_FAILED');
    final empty = await _readSnapshot({
      ..._connectedAndroidSnapshot,
      'frontends': <Object?>[],
    });
    expect(empty.frontends, isEmpty);
  });
}
