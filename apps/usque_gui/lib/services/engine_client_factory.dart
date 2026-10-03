import 'dart:io';

import 'desktop_engine_client.dart';
import 'engine_client.dart';

EngineClient createDefaultEngineClient() {
  if (Platform.isLinux) {
    throw UnsupportedError(
      'Linux supports the debug UI preview only. Run tool/dev.sh preview.',
    );
  }
  if (Platform.isWindows || Platform.isMacOS) {
    return DesktopEngineClient();
  }
  return MethodChannelEngineClient();
}
