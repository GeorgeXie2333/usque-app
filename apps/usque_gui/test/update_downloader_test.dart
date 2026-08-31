import 'dart:async';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/services/update_downloader.dart';

class _CacheEngine implements EngineClient {
  _CacheEngine(this.path);

  final String path;

  @override
  Future<String> getUpdateCacheDirectory() async => path;

  @override
  void dispose() {}

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

UpdatePackage _package(Uri uri, int size) => UpdatePackage(
  name: 'usque-v0.2.4-android-arm64-v8a.apk',
  downloadUrl: uri.toString(),
  size: size,
  sha256: List<String>.filled(32, 'a5').join(),
  platform: 'android',
  variant: 'arm64-v8a',
);

void main() {
  late Directory root;
  late HttpServer server;
  late UpdateDownloader downloader;

  setUp(() async {
    root = await Directory.systemTemp.createTemp('usque-update-test-');
    server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    downloader = UpdateDownloader(
      _CacheEngine(root.path),
      uriPolicy: (uri, initial) =>
          uri.host == InternetAddress.loopbackIPv4.address,
    );
  });

  tearDown(() async {
    await server.close(force: true);
    await root.delete(recursive: true);
  });

  test(
    'streams determinate progress and atomically publishes the package',
    () async {
      final bytes = List<int>.generate(8192, (index) => index & 0xff);
      server.listen((request) async {
        request.response.contentLength = bytes.length;
        request.response.add(bytes.sublist(0, 4096));
        request.response.add(bytes.sublist(4096));
        await request.response.close();
      });
      final progress = <int>[];
      final partialPath = await downloader.download(
        _package(
          Uri.parse('http://127.0.0.1:${server.port}/asset'),
          bytes.length,
        ),
        cancellation: UpdateDownloadCancellation(),
        onProgress: (downloaded, total) => progress.add(downloaded),
      );

      expect(partialPath, endsWith('.part'));
      expect(await File(partialPath).readAsBytes(), bytes);
      final destinationPath = partialPath.substring(0, partialPath.length - 5);
      expect(await File(destinationPath).exists(), isFalse);
      final path = await downloader.publish(
        partialPath,
        _package(
          Uri.parse('http://127.0.0.1:${server.port}/asset'),
          bytes.length,
        ),
      );
      expect(await File(path).readAsBytes(), bytes);
      expect(progress.first, 0);
      expect(progress.last, bytes.length);
      expect(await File(partialPath).exists(), isFalse);
    },
  );

  test('size mismatch fails closed and deletes the partial file', () async {
    server.listen((request) async {
      request.response.contentLength = 3;
      request.response.add(<int>[1, 2, 3]);
      await request.response.close();
    });
    final package = _package(
      Uri.parse('http://127.0.0.1:${server.port}/asset'),
      4,
    );

    await expectLater(
      downloader.download(
        package,
        cancellation: UpdateDownloadCancellation(),
        onProgress: (_, _) {},
      ),
      throwsA(isA<UpdateDownloadException>()),
    );
    expect(await File('${root.path}/${package.name}').exists(), isFalse);
    expect(await File('${root.path}/${package.name}.part').exists(), isFalse);
  });

  test(
    'cancellation closes the transfer and removes the partial file',
    () async {
      final firstChunkSent = Completer<void>();
      server.listen((request) async {
        request.response.contentLength = 64 * 1024;
        request.response.add(List<int>.filled(1024, 7));
        await request.response.flush();
        firstChunkSent.complete();
        await Future<void>.delayed(const Duration(seconds: 2));
        request.response.add(List<int>.filled(63 * 1024, 8));
        await request.response.close();
      });
      final cancellation = UpdateDownloadCancellation();
      final package = _package(
        Uri.parse('http://127.0.0.1:${server.port}/asset'),
        64 * 1024,
      );
      final future = downloader.download(
        package,
        cancellation: cancellation,
        onProgress: (_, _) {},
      );
      await firstChunkSent.future;
      cancellation.cancel();

      await expectLater(future, throwsA(isA<UpdateDownloadCancelled>()));
      expect(await File('${root.path}/${package.name}.part').exists(), isFalse);
    },
  );

  test('redirect response bodies time out and remove partial files', () async {
    final releaseResponse = Completer<void>();
    server.listen((request) async {
      request.response.statusCode = HttpStatus.found;
      request.response.headers.set(HttpHeaders.locationHeader, '/asset');
      request.response.write('redirect body never completes');
      await request.response.flush();
      try {
        await releaseResponse.future;
        await request.response.close();
      } on Object {
        // The downloader force-closes the timed-out connection.
      }
    });
    final timeoutDownloader = UpdateDownloader(
      _CacheEngine(root.path),
      responseTimeout: const Duration(milliseconds: 100),
      uriPolicy: (uri, initial) =>
          uri.host == InternetAddress.loopbackIPv4.address,
    );
    final package = _package(
      Uri.parse('http://127.0.0.1:${server.port}/redirect'),
      1,
    );

    try {
      await expectLater(
        timeoutDownloader.download(
          package,
          cancellation: UpdateDownloadCancellation(),
          onProgress: (_, _) {},
        ),
        throwsA(isA<UpdateDownloadException>()),
      );
    } finally {
      if (!releaseResponse.isCompleted) releaseResponse.complete();
    }
    expect(await File('${root.path}/${package.name}.part').exists(), isFalse);
  });

  test('stale managed packages are deleted after seven days', () async {
    final stale = File('${root.path}/usque-v0.2.1-android-arm64-v8a.apk');
    final recent = File('${root.path}/usque-v0.2.2-android-arm64-v8a.apk');
    await stale.writeAsBytes(<int>[1]);
    await recent.writeAsBytes(<int>[2]);
    await stale.setLastModified(
      DateTime.now().subtract(const Duration(days: 8)),
    );

    await downloader.cleanupStale();

    expect(await stale.exists(), isFalse);
    expect(await recent.exists(), isTrue);
  });

  test(
    'production policy rejects packages outside the exact repository path',
    () async {
      final productionDownloader = UpdateDownloader(_CacheEngine(root.path));
      final package = UpdatePackage(
        name: 'usque-v0.2.4-android-arm64-v8a.apk',
        downloadUrl:
            'https://attacker.invalid/releases/download/v0.2.4/usque-v0.2.4-android-arm64-v8a.apk',
        size: 1,
        sha256: List<String>.filled(32, 'a5').join(),
        platform: 'android',
        variant: 'arm64-v8a',
      );

      await expectLater(
        productionDownloader.download(
          package,
          cancellation: UpdateDownloadCancellation(),
          onProgress: (_, _) {},
        ),
        throwsA(isA<UpdateDownloadException>()),
      );
    },
  );
}
