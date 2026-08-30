import 'dart:async';
import 'dart:io';

import '../models/app_models.dart';
import 'engine_client.dart';

typedef UpdateProgressCallback = void Function(int downloaded, int total);

class UpdateDownloadCancellation {
  bool _cancelled = false;
  HttpClient? _client;

  bool get isCancelled => _cancelled;

  void cancel() {
    _cancelled = true;
    _client?.close(force: true);
  }

  void attach(HttpClient client) {
    _client = client;
    if (_cancelled) client.close(force: true);
  }

  void detach(HttpClient client) {
    if (identical(_client, client)) _client = null;
  }
}

class UpdateDownloadCancelled implements Exception {
  const UpdateDownloadCancelled();
}

class UpdateDownloadException implements Exception {
  const UpdateDownloadException(this.message);

  final String message;

  @override
  String toString() => message;
}

class UpdateDownloader {
  UpdateDownloader(
    this._engine, {
    HttpClient Function()? clientFactory,
    this.uriPolicy,
    this.responseTimeout = const Duration(seconds: 30),
  }) : _clientFactory = clientFactory ?? HttpClient.new;

  static const Duration _connectionTimeout = Duration(seconds: 8);
  static const Duration _staleAge = Duration(days: 7);
  static const int _maximumRedirects = 5;

  final EngineClient _engine;
  final HttpClient Function() _clientFactory;
  final Duration responseTimeout;
  final bool Function(Uri uri, bool initial)? uriPolicy;

  Future<String> download(
    UpdatePackage package, {
    required UpdateProgressCallback onProgress,
    required UpdateDownloadCancellation cancellation,
  }) async {
    _validatePackage(package);
    final directory = Directory(await _engine.getUpdateCacheDirectory());
    await directory.create(recursive: true);
    await cleanupStale(directory: directory);
    final destination = File(
      '${directory.path}${Platform.pathSeparator}${package.name}',
    );
    final partial = File('${destination.path}.part');
    await _deleteIfPresent(destination);
    await _deleteIfPresent(partial);

    final client = _clientFactory()..connectionTimeout = _connectionTimeout;
    cancellation.attach(client);
    IOSink? sink;
    var retainPartial = false;
    try {
      if (cancellation.isCancelled) throw const UpdateDownloadCancelled();
      final response = await _openResponse(
        client,
        Uri.parse(package.downloadUrl),
      );
      final contentLength = response.contentLength;
      if (contentLength >= 0 && contentLength != package.size) {
        throw const UpdateDownloadException(
          'The downloaded package size did not match the release metadata.',
        );
      }
      sink = partial.openWrite(mode: FileMode.writeOnly);
      var downloaded = 0;
      onProgress(0, package.size);
      await for (final chunk in response.timeout(responseTimeout)) {
        if (cancellation.isCancelled) throw const UpdateDownloadCancelled();
        downloaded += chunk.length;
        if (downloaded > package.size) {
          throw const UpdateDownloadException(
            'The downloaded package exceeded its declared size.',
          );
        }
        sink.add(chunk);
        onProgress(downloaded, package.size);
      }
      await sink.flush();
      await sink.close();
      sink = null;
      if (cancellation.isCancelled) throw const UpdateDownloadCancelled();
      if (downloaded != package.size) {
        throw const UpdateDownloadException(
          'The downloaded package ended before its declared size.',
        );
      }
      retainPartial = true;
      return partial.path;
    } on UpdateDownloadCancelled {
      rethrow;
    } on Object catch (error) {
      if (cancellation.isCancelled) throw const UpdateDownloadCancelled();
      if (error is UpdateDownloadException) rethrow;
      throw UpdateDownloadException('The update download failed: $error');
    } finally {
      if (sink != null) {
        await sink.close();
      }
      client.close(force: true);
      cancellation.detach(client);
      if (!retainPartial) await _deleteIfPresent(partial);
    }
  }

  Future<String> publish(String partialPath, UpdatePackage package) async {
    _validatePackage(package);
    final root = Directory(await _engine.getUpdateCacheDirectory()).absolute;
    final expectedPartial = File(
      '${root.path}${Platform.pathSeparator}${package.name}.part',
    ).absolute;
    final partial = File(partialPath).absolute;
    if (partial.path != expectedPartial.path || !await partial.exists()) {
      throw const UpdateDownloadException(
        'The verified update partial file was no longer available.',
      );
    }
    final destination = File(
      '${root.path}${Platform.pathSeparator}${package.name}',
    );
    return (await partial.rename(destination.path)).path;
  }

  Future<void> cleanupStale({Directory? directory}) async {
    final root =
        directory ?? Directory(await _engine.getUpdateCacheDirectory());
    if (!await root.exists()) return;
    final cutoff = DateTime.now().subtract(_staleAge);
    await for (final entity in root.list(followLinks: false)) {
      if (entity is! File || !_isManagedFile(entity)) continue;
      try {
        final modified = await entity.lastModified();
        if (modified.isBefore(cutoff)) await entity.delete();
      } on FileSystemException {
        // Cleanup is best-effort and must never block app startup.
      }
    }
  }

  Future<void> discard(String? path) async {
    if (path == null || path.isEmpty) return;
    final root = Directory(await _engine.getUpdateCacheDirectory()).absolute;
    final file = File(path).absolute;
    final prefix = '${root.path}${Platform.pathSeparator}';
    if (!file.path.startsWith(prefix) || !_isManagedFile(file)) return;
    await _deleteIfPresent(file);
  }

  Future<HttpClientResponse> _openResponse(
    HttpClient client,
    Uri initial,
  ) async {
    var uri = initial;
    for (var redirects = 0; redirects <= _maximumRedirects; redirects += 1) {
      _validateDownloadUri(uri, initial: redirects == 0);
      final request = await client.getUrl(uri);
      request.followRedirects = false;
      request.headers.set(HttpHeaders.userAgentHeader, 'Usque update-download');
      request.headers.set(HttpHeaders.acceptHeader, 'application/octet-stream');
      final response = await request.close().timeout(responseTimeout);
      if (response.isRedirect) {
        final location = response.headers.value(HttpHeaders.locationHeader);
        if (location == null || location.isEmpty) {
          throw const UpdateDownloadException(
            'The update server returned a redirect without a destination.',
          );
        }
        await response.timeout(responseTimeout).drain<void>();
        uri = uri.resolve(location);
        continue;
      }
      if (response.statusCode != HttpStatus.ok) {
        await response.timeout(responseTimeout).drain<void>();
        throw UpdateDownloadException(
          'The update server returned HTTP ${response.statusCode}.',
        );
      }
      return response;
    }
    throw const UpdateDownloadException(
      'The update download exceeded the redirect limit.',
    );
  }

  void _validatePackage(UpdatePackage package) {
    if (package.size <= 0 || package.size > 512 * 1024 * 1024) {
      throw const UpdateDownloadException(
        'The release declared an invalid update package size.',
      );
    }
    if (package.name.isEmpty ||
        package.name.contains('/') ||
        package.name.contains(r'\') ||
        package.name.contains('..')) {
      throw const UpdateDownloadException(
        'The release declared an invalid update package name.',
      );
    }
    final expectedExtension = package.platform == 'windows' ? '.msi' : '.apk';
    if (!package.name.endsWith(expectedExtension)) {
      throw const UpdateDownloadException(
        'The release package did not match the current platform.',
      );
    }
    final validVariant = switch (package.platform) {
      'windows' => const <String>{'x64-v2', 'arm64'}.contains(package.variant),
      'android' => const <String>{
        'arm64-v8a',
        'x86_64',
        'armeabi-v7a',
      }.contains(package.variant),
      _ => false,
    };
    if (!validVariant) {
      throw const UpdateDownloadException(
        'The release package platform or architecture was invalid.',
      );
    }
    if (uriPolicy == null) {
      final uri = Uri.tryParse(package.downloadUrl);
      final segments = uri?.pathSegments ?? const <String>[];
      final exactRepositoryAsset =
          uri != null &&
          segments.length == 6 &&
          segments[0] == 'GeorgeXie2333' &&
          segments[1] == 'usque-app' &&
          segments[2] == 'releases' &&
          segments[3] == 'download' &&
          segments[5] == package.name &&
          package.name ==
              'usque-${segments[4]}-${package.platform}-${package.variant}'
                  '$expectedExtension';
      if (!exactRepositoryAsset) {
        throw const UpdateDownloadException(
          'The update package was not an exact Usque GitHub release asset.',
        );
      }
    }
  }

  void _validateDownloadUri(Uri uri, {required bool initial}) {
    final policy = uriPolicy;
    if (policy != null) {
      if (!policy(uri, initial)) {
        throw const UpdateDownloadException(
          'The update download URL was rejected by the active policy.',
        );
      }
      return;
    }
    if (uri.scheme != 'https' || uri.userInfo.isNotEmpty || uri.hasFragment) {
      throw const UpdateDownloadException(
        'The update download URL was not an approved HTTPS URL.',
      );
    }
    final host = uri.host.toLowerCase();
    final allowed = switch (host) {
      'github.com' ||
      'release-assets.githubusercontent.com' ||
      'objects.githubusercontent.com' => true,
      _ when host.endsWith('.githubusercontent.com') => true,
      _ => false,
    };
    if (!allowed || (initial && host != 'github.com')) {
      throw const UpdateDownloadException(
        'The update download redirected outside the approved GitHub hosts.',
      );
    }
  }

  bool _isManagedFile(File file) {
    final name = file.uri.pathSegments.last;
    return name.startsWith('usque-v') &&
        (name.endsWith('.msi') ||
            name.endsWith('.apk') ||
            name.endsWith('.msi.part') ||
            name.endsWith('.apk.part'));
  }

  Future<void> _deleteIfPresent(File file) async {
    try {
      await file.delete();
    } on FileSystemException catch (error) {
      if (error.osError?.errorCode == 2 || !await file.exists()) return;
      rethrow;
    }
  }
}
