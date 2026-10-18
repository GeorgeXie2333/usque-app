import 'dart:async';
import 'dart:ffi';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

import 'support/platform_golden_comparator.dart';

Future<void> testExecutable(FutureOr<void> Function() testMain) async {
  goldenFileComparator = PlatformGoldenComparator(
    goldenFileComparator,
    operatingSystem: Platform.operatingSystem,
    abi: Abi.current(),
  );
  await testMain();
}
