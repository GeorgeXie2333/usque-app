import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import 'app.dart';
import 'state/window_frame.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  // The Windows runner removes the native caption, so Flutter has to draw one.
  if (!kIsWeb && Platform.isWindows) {
    WindowFrame.instance.enable();
  }
  runApp(const UsqueBootstrap());
}
