// Copyright 2014 The Flutter Authors. All rights reserved.
// Derived from Flutter 3.44.7 packages/flutter_tools/bin/tool_backend.dart.
// The upstream BSD license is in flutter_windows_backend.LICENSE.
// Local changes: pass the icon flag without literal quotes, expose argument
// construction for tests, validate inputs, and drain output before forwarding
// the child exit code. Keep this adapter aligned with the pinned SDK on upgrades.
import 'dart:io';

typedef AssembleInvocation = ({
  String executable,
  List<String> arguments,
  String workingDirectory,
});

AssembleInvocation assembleInvocation(
  List<String> arguments,
  Map<String, String> environment, {
  required bool windows,
}) {
  if (arguments.length != 2 ||
      !{'windows-x64', 'windows-arm64'}.contains(arguments[0]) ||
      !{'debug', 'profile', 'release'}.contains(arguments[1].toLowerCase())) {
    throw ArgumentError(
      'Expected windows-x64/windows-arm64 and Debug/Profile/Release.',
    );
  }
  final targetPlatform = arguments[0];
  final buildMode = arguments[1].toLowerCase();
  final flutterRoot = environment['FLUTTER_ROOT'];
  final projectDirectory = environment['PROJECT_DIR'];
  if (flutterRoot == null ||
      flutterRoot.isEmpty ||
      projectDirectory == null ||
      projectDirectory.isEmpty) {
    throw ArgumentError('FLUTTER_ROOT and PROJECT_DIR must be non-empty.');
  }
  for (final key in ['LOCAL_ENGINE', 'LOCAL_ENGINE_HOST']) {
    final value = environment[key];
    if (value != null && !value.contains(buildMode)) {
      throw ArgumentError('$key is incompatible with $buildMode.');
    }
  }
  final separator = windows ? r'\' : '/';
  return (
    executable: [
      flutterRoot,
      'bin',
      windows ? 'flutter.bat' : 'flutter',
    ].join(separator),
    workingDirectory: projectDirectory,
    arguments: [
      if (environment['VERBOSE_SCRIPT_LOGGING'] == 'true') '--verbose',
      if (environment['PREFIXED_ERROR_LOGGING'] == 'true') '--prefixed-errors',
      if (environment['FLUTTER_ENGINE'] case final value?)
        '--local-engine-src-path=$value',
      if (environment['LOCAL_ENGINE'] case final value?)
        '--local-engine=$value',
      if (environment['LOCAL_ENGINE_HOST'] case final value?)
        '--local-engine-host=$value',
      'assemble',
      '--no-version-check',
      '--output=build',
      '-dTargetPlatform=$targetPlatform',
      '-dTrackWidgetCreation=${environment['TRACK_WIDGET_CREATION'] == 'true'}',
      '-dBuildMode=$buildMode',
      '-dTargetFile=${environment['FLUTTER_TARGET'] ?? ['lib', 'main.dart'].join(separator)}',
      // Process.start receives an argument list, not a shell-quoted command.
      '-dTreeShakeIcons=${environment['TREE_SHAKE_ICONS'] == 'true'}',
      '-dDartObfuscation=${environment['DART_OBFUSCATION'] == 'true'}',
      if (environment['CODE_SIZE_DIRECTORY'] case final value?)
        '-dCodeSizeDirectory=$value',
      if (environment['SPLIT_DEBUG_INFO'] case final value?)
        '-dSplitDebugInfo=$value',
      if (environment['DART_DEFINES'] case final value?) '--DartDefines=$value',
      if (environment['EXTRA_GEN_SNAPSHOT_OPTIONS'] case final value?)
        '--ExtraGenSnapshotOptions=$value',
      if (environment['FRONTEND_SERVER_STARTER_PATH'] case final value?)
        '-dFrontendServerStarterPath=$value',
      if (environment['EXTRA_FRONT_END_OPTIONS'] case final value?)
        '--ExtraFrontEndOptions=$value',
      '${buildMode}_bundle_${targetPlatform}_assets',
    ],
  );
}

Future<void> main(List<String> arguments) async {
  try {
    final invocation = assembleInvocation(
      arguments,
      Platform.environment,
      windows: Platform.isWindows,
    );
    final process = await Process.start(
      invocation.executable,
      invocation.arguments,
      workingDirectory: invocation.workingDirectory,
    );
    await Future.wait([
      stdout.addStream(process.stdout),
      stderr.addStream(process.stderr),
    ]);
    exitCode = await process.exitCode;
  } on ArgumentError catch (error) {
    stderr.writeln(error.message);
    exitCode = 1;
  } on ProcessException catch (error) {
    stderr.writeln('Flutter assemble could not start: ${error.message}');
    exitCode = 1;
  }
}
