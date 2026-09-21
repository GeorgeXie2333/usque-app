import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/services/update_downloader.dart';
import 'package:usque/state/app_controller.dart';

import 'app_test.dart' show FakeEngineClient;

const package = UpdatePackage(
  name: 'usque-v0.2.8-windows-x64-v2.msi',
  downloadUrl:
      'https://github.com/GeorgeXie2333/usque-app/releases/download/v0.2.8/usque-v0.2.8-windows-x64-v2.msi',
  size: 1,
  sha256: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  platform: 'windows',
  variant: 'x64-v2',
);
const available = UpdateCheckResult(
  available: true,
  version: '0.2.8',
  package: package,
);

class HeldCleanupEngine extends FakeEngineClient {
  Completer<NetworkSettingsState>? settingsReply;
  Completer<DiagnosticSession?>? diagnosticReply;
  @override
  Future<NetworkSettingsState> getNetworkSettingsState() =>
      settingsReply?.future ?? super.getNetworkSettingsState();
  @override
  Future<DiagnosticSession?> getDiagnostics() =>
      diagnosticReply?.future ?? super.getDiagnostics();
}

class HeldPublisher extends UpdateDownloader {
  HeldPublisher(super.engine);
  final entered = Completer<void>();
  final release = Completer<void>();
  final actions = <String>[];
  int downloads = 0;
  @override
  Future<String> download(
    UpdatePackage package, {
    required UpdateProgressCallback onProgress,
    required UpdateDownloadCancellation cancellation,
  }) async {
    actions.add('download-${++downloads}');
    return 'test-update.msi.part';
  }

  @override
  Future<String> publish(String path, UpdatePackage package) async {
    actions.add('publish-$downloads');
    if (downloads == 1) {
      entered.complete();
      await release.future;
    }
    return 'test-update.msi';
  }

  @override
  Future<void> discard(String? path) async {
    if (path != null) actions.add('discard');
  }
}

class FailingVerify extends FakeEngineClient {
  @override
  Future<void> verifyUpdatePackage({
    required String path,
    required String version,
    required UpdatePackage package,
  }) async {
    throw const EngineException('ENGINE_REQUEST_TIMEOUT', 'primary');
  }
}

class FailingDiscard extends HeldPublisher {
  FailingDiscard(super.engine);
  @override
  Future<void> discard(String? path) async =>
      throw StateError('secondary cleanup');
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(
    () => SharedPreferences.setMockInitialValues({
      'update_checks_enabled': false,
    }),
  );

  test(
    'clear resets shared settings and diagnostics and rejects old replies',
    () async {
      final engine = HeldCleanupEngine();
      final app = AppController(engine);
      addTearDown(app.dispose);
      await app.initialize();
      await Future<void>.delayed(Duration.zero);
      await app.saveNetwork(app.activeProfile.copyWith(mtu: 1400));
      final oldSettings = app.networkSettings.state!;
      engine.settingsReply = Completer<NetworkSettingsState>();
      engine.diagnosticReply = Completer<DiagnosticSession?>();
      final settings = app.networkSettings.refresh();
      final diagnostic = app.diagnostics.restore();
      expect(await app.clearAllData(), true);
      engine.settingsReply!.complete(oldSettings);
      engine.diagnosticReply!.complete(
        DiagnosticSession(
          sessionId: 'old',
          state: DiagnosticSessionState.completed,
          startedAt: DateTime.utc(2026),
          mode: DiagnosticMode.standard,
        ),
      );
      await Future.wait([settings, diagnostic]);
      expect(app.sharedNetwork.mtu, UsqueProfile.defaultProfile().mtu);
      expect(app.networkSettings.state, isNull);
      expect(app.diagnostics.session, isNull);
      expect(app.diagnostics.timeline.events, isEmpty);
      expect(app.quality.history, isEmpty);
      engine.settingsReply = null;
      engine.diagnosticReply = null;
      expect(
        await app.saveNetwork(app.activeProfile.copyWith(mtu: 1420)),
        true,
      );
      expect(app.sharedNetwork.mtu, 1420);
    },
  );

  test('publish finishing after clear cannot revive ready state', () async {
    final engine = FakeEngineClient();
    final downloader = HeldPublisher(engine);
    final app = AppController(engine, updateDownloader: downloader)
      ..updateResult = available;
    addTearDown(app.dispose);
    final downloading = app.downloadUpdate();
    await downloader.entered.future;
    expect(await app.clearAllData(), true);
    downloader.release.complete();
    await downloading;
    expect(app.updatePhase, UpdateOperationPhase.idle);
    expect(app.downloadedUpdatePath, isNull);
    expect(downloader.actions.last, 'discard');
  });

  test(
    'new download waits until old publication and cleanup release ownership',
    () async {
      final engine = FakeEngineClient();
      final downloader = HeldPublisher(engine);
      final app = AppController(engine, updateDownloader: downloader)
        ..updateResult = available;
      addTearDown(app.dispose);
      final old = app.downloadUpdate();
      await downloader.entered.future;
      await app.clearAllData();
      app.updateResult = available;
      final fresh = app.downloadUpdate();
      await Future<void>.delayed(Duration.zero);
      expect(downloader.downloads, 1);
      downloader.release.complete();
      await Future.wait([old, fresh]);
      expect(downloader.actions, [
        'download-1',
        'publish-1',
        'discard',
        'download-2',
        'publish-2',
      ]);
      expect(app.updatePhase, UpdateOperationPhase.ready);
    },
  );

  test(
    'cleanup error cannot mask the primary error or retain busy phase',
    () async {
      final engine = FailingVerify();
      final app = AppController(
        engine,
        updateDownloader: FailingDiscard(engine),
      )..updateResult = available;
      addTearDown(app.dispose);
      await app.downloadUpdate();
      expect(app.updatePhase, UpdateOperationPhase.failed);
      expect(app.updateOperationActive, false);
      expect(app.updateError, app.strings.get('operation_timeout'));
    },
  );
}
