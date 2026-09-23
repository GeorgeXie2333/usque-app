import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'app.dart';
import 'state/window_frame.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  LicenseRegistry.addLicense(() async* {
    yield LicenseEntryWithLineBreaks(const [
      'Flagpedia country and region flags',
    ], await rootBundle.loadString('assets/licenses/flagpedia.txt'));
  });
  LicenseRegistry.addLicense(() async* {
    yield LicenseEntryWithLineBreaks(const [
      'BoringTun',
    ], await rootBundle.loadString('assets/licenses/boringtun.txt'));
  });
  LicenseRegistry.addLicense(() async* {
    yield LicenseEntryWithLineBreaks(const [
      'OpenVPN 3 Core',
      'Mbed TLS',
      'Asio',
      'LZ4',
      'xxHash',
    ], await rootBundle.loadString('assets/licenses/vpngate.txt'));
  });
  LicenseRegistry.addLicense(() async* {
    yield LicenseEntryWithLineBreaks(const [
      'WARPSCOUT endpoint data',
    ], await rootBundle.loadString('assets/licenses/warpscout.txt'));
  });
  // The Windows runner removes the native caption, so Flutter has to draw one.
  if (!kIsWeb && Platform.isWindows) {
    WindowFrame.instance.enable();
  }
  runApp(const UsqueBootstrap());
}
