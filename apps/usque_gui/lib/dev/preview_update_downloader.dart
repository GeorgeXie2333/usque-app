import 'dart:io';

import '../models/app_models.dart';
import '../services/update_downloader.dart';

/// Prevent even background update-cache scans in the in-memory preview.
class PreviewUpdateDownloader extends UpdateDownloader {
  PreviewUpdateDownloader(super.engine);

  @override
  Future<void> cleanupStale({Directory? directory}) async {}

  @override
  Future<void> discard(String? path) async {}

  @override
  Future<String> download(
    UpdatePackage package, {
    required UpdateProgressCallback onProgress,
    required UpdateDownloadCancellation cancellation,
  }) async => throw const UpdateDownloadException(
    'Updates are unavailable in the UI preview.',
  );
}
