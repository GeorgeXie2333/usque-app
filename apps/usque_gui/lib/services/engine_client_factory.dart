import 'dart:io';

import 'desktop_engine_client.dart';
import 'engine_client.dart';

EngineClient createDefaultEngineClient() {
  if (Platform.isWindows || Platform.isMacOS) {
    return DesktopEngineClient();
  }
  return MethodChannelEngineClient();
}
