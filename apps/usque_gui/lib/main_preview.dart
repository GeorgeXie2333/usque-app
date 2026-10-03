import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import 'dev/preview_app.dart';

void main() {
  if (!kDebugMode || !Platform.isLinux) {
    throw UnsupportedError('The UI preview requires a Linux debug build.');
  }
  WidgetsFlutterBinding.ensureInitialized();
  runApp(const PreviewApp());
}
