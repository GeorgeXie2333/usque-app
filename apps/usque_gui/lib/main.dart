import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'app.dart';
import 'services/gui_startup_observation.dart';
import 'services/linux_preferences_store.dart';
import 'state/window_frame.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  if (!kIsWeb && Platform.isLinux) {
    LinuxPreferencesStore.register();
  }
  if (!kIsWeb && (Platform.isLinux || Platform.isMacOS)) {
    GuiStartupObservation.instance.start();
  }
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
      'wgcf registration protocol reference',
    ], await rootBundle.loadString('assets/licenses/wgcf.txt'));
  });
  LicenseRegistry.addLicense(() async* {
    yield LicenseEntryWithLineBreaks(const [
      'Proton go-srp algorithm and interoperability vectors',
    ], await rootBundle.loadString('assets/licenses/proton-srp.txt'));
  });
  // The Windows runner removes the native caption, so Flutter has to draw one.
  if (!kIsWeb && Platform.isWindows) {
    WindowFrame.instance.enable();
  }
  runApp(const UsqueBootstrap());
}
