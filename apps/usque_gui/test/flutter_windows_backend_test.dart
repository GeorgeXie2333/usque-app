import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

import '../../../tool/flutter_windows_backend.dart' as backend;

void main() {
  const environment = {
    'FLUTTER_ROOT': r'C:\SDK with spaces',
    'PROJECT_DIR': r'C:\app with spaces',
    'TREE_SHAKE_ICONS': 'true',
  };

  test('desktop mode and icon arguments are exact unquoted values', () {
    for (final architecture in ['windows-x64', 'windows-arm64']) {
      for (final mode in ['Debug', 'Profile', 'Release']) {
        final result = backend.assembleInvocation(
          [architecture, mode],
          environment,
          windows: true,
        );
        expect(result.executable, r'C:\SDK with spaces\bin\flutter.bat');
        expect(result.workingDirectory, environment['PROJECT_DIR']);
        expect(result.arguments, contains('-dTreeShakeIcons=true'));
        expect(result.arguments, contains('-dBuildMode=${mode.toLowerCase()}'));
        expect(
          result.arguments.last,
          '${mode.toLowerCase()}_bundle_${architecture}_assets',
        );
      }
    }
    expect(
      backend
          .assembleInvocation(
            ['windows-x64', 'Debug'],
            {...environment, 'TREE_SHAKE_ICONS': 'false'},
            windows: true,
          )
          .arguments,
      contains('-dTreeShakeIcons=false'),
    );
  });

  test('forwards symbols, analysis, defines and optional engine arguments', () {
    final result = backend.assembleInvocation(
      ['windows-arm64', 'Release'],
      {
        ...environment,
        'SPLIT_DEBUG_INFO': r'C:\symbols with spaces',
        'CODE_SIZE_DIRECTORY': r'C:\analysis with spaces',
        'DART_DEFINES': 'Zm9vPWJhcg==',
        'FLUTTER_TARGET': 'lib/alternate.dart',
        'DART_OBFUSCATION': 'true',
        'TRACK_WIDGET_CREATION': 'true',
        'VERBOSE_SCRIPT_LOGGING': 'true',
        'PREFIXED_ERROR_LOGGING': 'true',
        'FLUTTER_ENGINE': 'engine source',
        'LOCAL_ENGINE': 'host_release',
        'LOCAL_ENGINE_HOST': 'host_release',
        'EXTRA_GEN_SNAPSHOT_OPTIONS': '--snapshot-option',
        'FRONTEND_SERVER_STARTER_PATH': 'frontend path',
        'EXTRA_FRONT_END_OPTIONS': '--frontend-option',
      },
      windows: true,
    );
    expect(
      result.arguments,
      containsAll([
        r'-dSplitDebugInfo=C:\symbols with spaces',
        r'-dCodeSizeDirectory=C:\analysis with spaces',
        '--DartDefines=Zm9vPWJhcg==',
        '-dTargetFile=lib/alternate.dart',
        '-dDartObfuscation=true',
        '-dTrackWidgetCreation=true',
        '--verbose',
        '--prefixed-errors',
        '--local-engine-src-path=engine source',
        '--local-engine=host_release',
        '--local-engine-host=host_release',
        '--ExtraGenSnapshotOptions=--snapshot-option',
        '-dFrontendServerStarterPath=frontend path',
        '--ExtraFrontEndOptions=--frontend-option',
      ]),
    );
  });

  test('rejects absent configuration and unsupported mode or architecture', () {
    for (final arguments in <List<String>>[
      [],
      ['windows-x64'],
      ['linux-x64', 'Release'],
      ['windows-x64', 'Other'],
    ]) {
      expect(
        () => backend.assembleInvocation(arguments, environment, windows: true),
        throwsArgumentError,
      );
    }
    expect(
      () => backend.assembleInvocation(
        ['windows-x64', 'Release'],
        {},
        windows: true,
      ),
      throwsArgumentError,
    );
    expect(
      () => backend.assembleInvocation(
        ['windows-x64', 'Release'],
        {...environment, 'LOCAL_ENGINE': 'host_debug'},
        windows: true,
      ),
      throwsArgumentError,
    );
  });

  test(
    'Windows process forwarding preserves spaces, output and failure code',
    () async {
      final root = Directory.systemTemp.createTempSync(
        'usque backend with spaces ',
      );
      addTearDown(() => root.deleteSync(recursive: true));
      // flutter_tester is under cache/artifacts/engine/windows-x64. Resolve the
      // pinned Dart executable from the app's package configuration instead.
      final config =
          jsonDecode(File('.dart_tool/package_config.json').readAsStringSync())
              as Map<String, dynamic>;
      final flutter = (config['packages'] as List)
          .cast<Map<String, dynamic>>()
          .singleWhere((package) => package['name'] == 'flutter');
      final sdk = Directory.fromUri(
        Uri.parse(flutter['rootUri'] as String),
      ).parent.parent;
      final dartExecutable = File(
        '${sdk.path}/bin/cache/dart-sdk/bin/dart.exe',
      );
      expect(dartExecutable.existsSync(), isTrue);
      Directory('${root.path}/bin').createSync();
      File('${root.path}/capture.dart').writeAsStringSync('''
import 'dart:convert';
import 'dart:io';
void main(List<String> args) {
  stdout.writeln(jsonEncode(args));
  stderr.writeln('inert fixture stderr');
  exitCode = 7;
}
''');
      File('${root.path}/bin/flutter.bat').writeAsStringSync(
        '@echo off\r\n"${dartExecutable.path}" "${root.path}\\capture.dart" %*\r\n',
      );
      final result = await Process.run(
        dartExecutable.path,
        [
          File('../../tool/flutter_windows_backend.dart').absolute.path,
          'windows-x64',
          'Release',
        ],
        environment: {
          'FLUTTER_ROOT': root.path,
          'PROJECT_DIR': root.path,
          'TREE_SHAKE_ICONS': 'true',
          'SPLIT_DEBUG_INFO': '${root.path}/symbols',
        },
      );
      expect(result.exitCode, 7);
      expect(result.stderr, contains('inert fixture stderr'));
      final args = jsonDecode((result.stdout as String).trim()) as List;
      expect(args, contains('-dTreeShakeIcons=true'));
      expect(args, contains('-dSplitDebugInfo=${root.path}/symbols'));
    },
    skip: !Platform.isWindows,
  );
}
