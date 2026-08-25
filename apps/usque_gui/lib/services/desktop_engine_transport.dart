import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'control_codec.dart';
import 'engine_client.dart';

const String _pipePrefix =
    r'\\.\pipe\io.github.georgexie2333.usque.engine.v1-ui-';
const Duration _windowsEngineReadyTimeout = Duration(seconds: 30);
const int _maximumStartupStderrBytes = 4096;

class _EngineStartupOutcome {
  const _EngineStartupOutcome.ready() : exitCode = null;
  const _EngineStartupOutcome.exited(this.exitCode);

  final int? exitCode;
}

class _BoundedStderrCapture {
  final BytesBuilder _bytes = BytesBuilder(copy: false);
  bool _truncated = false;

  void add(List<int> chunk) {
    final remaining = _maximumStartupStderrBytes - _bytes.length;
    if (remaining <= 0) {
      _truncated = true;
      return;
    }
    if (chunk.length <= remaining) {
      _bytes.add(chunk);
      return;
    }
    _bytes.add(chunk.sublist(0, remaining));
    _truncated = true;
  }

  String text() {
    final value = utf8.decode(_bytes.takeBytes(), allowMalformed: true).trim();
    if (value.isEmpty) return '';
    return _truncated ? '$value…' : value;
  }
}

/// Process lifecycle, IPC endpoint, framed exchange, and request sequence
/// numbers for the desktop engine sidecar.
///
/// MethodChannel / EventChannel names and named-pipe / unix-socket framing are
/// part of the public transport contract and must not change.
class DesktopEngineTransport {
  DesktopEngineTransport()
    : _testExchange = null,
      _testEnsureStarted = null,
      _testRawEvents = null,
      _testSupportsSnapshotEvents = null,
      _testRequestIdFactory = null,
      _testSelectDiagnostics = null,
      _testSelectWarpSecret = null,
      _isTestTransport = false,
      _endpoint = Platform.isWindows
          ? '$_pipePrefix$pid-${DateTime.now().microsecondsSinceEpoch}'
          : _macSocketPath();

  /// Fully controlled transport for unit tests (no process spawn).
  @visibleForTesting
  DesktopEngineTransport.forTest({
    required Future<Uint8List> Function(Uint8List request) exchange,
    Future<void> Function()? ensureStarted,
    Stream<Uint8List>? rawEventFrames,
    bool supportsSnapshotEvents = false,
    String Function()? requestIdFactory,
    Future<String?> Function()? selectDiagnosticsDestination,
    Future<String?> Function()? selectWarpSecretDestination,
  }) : _testExchange = exchange,
       _testEnsureStarted = ensureStarted,
       _testRawEvents = rawEventFrames,
       _testSupportsSnapshotEvents = supportsSnapshotEvents,
       _testRequestIdFactory = requestIdFactory,
       _testSelectDiagnostics = selectDiagnosticsDestination,
       _testSelectWarpSecret = selectWarpSecretDestination,
       _isTestTransport = true,
       _endpoint = 'test-endpoint';

  static const MethodChannel _nativeTransport = MethodChannel(
    'io.github.georgexie2333.usque/engine',
  );
  static const EventChannel _nativeEvents = EventChannel(
    'io.github.georgexie2333.usque/engine_events',
  );

  final Future<Uint8List> Function(Uint8List request)? _testExchange;
  final Future<void> Function()? _testEnsureStarted;
  final Stream<Uint8List>? _testRawEvents;
  final bool? _testSupportsSnapshotEvents;
  final String Function()? _testRequestIdFactory;
  final Future<String?> Function()? _testSelectDiagnostics;
  final Future<String?> Function()? _testSelectWarpSecret;
  final bool _isTestTransport;

  final String _endpoint;

  Process? _process;
  Future<void>? _starting;
  int _requestSequence = 0;
  int _startCount = 0;
  bool _disposed = false;

  /// Number of times the engine process start path has completed successfully.
  @visibleForTesting
  int get startCount => _startCount;

  @visibleForTesting
  String get endpoint => _endpoint;

  bool get isDisposed => _disposed;

  bool get supportsSnapshotEvents =>
      _testSupportsSnapshotEvents ?? Platform.isWindows;

  /// Raw length-prefixed event frames from the native event bridge.
  Stream<Uint8List> get rawEventFrames {
    if (_isTestTransport) {
      return _testRawEvents ?? const Stream<Uint8List>.empty();
    }
    if (!Platform.isWindows) {
      return const Stream<Uint8List>.empty();
    }
    return _nativeEvents
        .receiveBroadcastStream(<String, Object>{
          'pipe_name': '$_endpoint.events',
        })
        .map<Uint8List>((Object? value) {
          if (value is! Uint8List) {
            throw const EngineException(
              'ENGINE_EVENT_INVALID',
              'The local Engine returned an invalid event frame.',
            );
          }
          return value;
        });
  }

  String allocateRequestId() {
    final factory = _testRequestIdFactory;
    if (factory != null) {
      return factory();
    }
    return '$pid-${++_requestSequence}';
  }

  Future<void> ensureStarted() async {
    if (_disposed) {
      throw const EngineException(
        'ENGINE_CLOSED',
        'The Usque Engine client has already closed.',
      );
    }
    if (_isTestTransport) {
      // Mirror production coalescing: start at most once, share in-flight work.
      if (_startCount > 0) {
        _throwIfDisposed();
        return;
      }
      final existing = _starting;
      if (existing != null) {
        await existing;
        _throwIfDisposed();
        return;
      }
      final start = () async {
        final testStart = _testEnsureStarted;
        if (testStart != null) {
          await testStart();
        }
        if (_disposed) {
          throw const EngineException(
            'ENGINE_CLOSED',
            'The Usque Engine client has already closed.',
          );
        }
        _startCount++;
      }();
      _starting = start;
      try {
        await start;
      } finally {
        _starting = null;
      }
      _throwIfDisposed();
      return;
    }
    final existing = _starting;
    if (existing != null) {
      await existing;
      _throwIfDisposed();
      return;
    }
    if (_process != null) {
      _throwIfDisposed();
      return;
    }
    final start = _startEngine();
    _starting = start;
    try {
      await start;
      if (!_disposed) {
        _startCount++;
      }
    } finally {
      _starting = null;
    }
    _throwIfDisposed();
  }

  Future<Uint8List> exchangeFrame(Uint8List request) async {
    _throwIfDisposed();
    final testExchange = _testExchange;
    if (testExchange != null) {
      return testExchange(request);
    }
    if (Platform.isWindows) {
      try {
        final response = await _nativeTransport.invokeMethod<Uint8List>(
          'exchangeFrame',
          <String, Object>{'pipe_name': _endpoint, 'request': request},
        );
        if (response == null) {
          throw const EngineException(
            'ENGINE_IPC_INVALID_RESPONSE',
            'The native Named Pipe bridge returned no response.',
          );
        }
        return response;
      } on PlatformException catch (error) {
        throw EngineException(
          error.code,
          error.message ?? 'The native Named Pipe bridge failed.',
        );
      }
    }

    final socket = await Socket.connect(
      InternetAddress(_endpoint, type: InternetAddressType.unix),
      0,
      timeout: const Duration(seconds: 1),
    );
    try {
      socket.add(request);
      await socket.flush();
      return await readFrame(socket);
    } finally {
      await socket.close();
    }
  }

  /// Whether a live process handle is currently held (production path only).
  bool get hasLiveProcess => _process != null;

  Future<String?> selectDiagnosticsDestination() async {
    final testSelect = _testSelectDiagnostics;
    if (testSelect != null) {
      return testSelect();
    }
    return _nativeTransport.invokeMethod<String>(
      'selectDiagnosticsDestination',
    );
  }

  Future<String?> selectWarpSecretDestination() async {
    final testSelect = _testSelectWarpSecret;
    if (testSelect != null) return testSelect();
    return _nativeTransport.invokeMethod<String>('selectWarpSecretDestination');
  }

  Future<T?> invokePlatformMethod<T>(
    String method, [
    Map<String, Object?>? arguments,
  ]) => _nativeTransport.invokeMethod<T>(method, arguments);

  void dispose() {
    _disposed = true;
    final process = _process;
    _process = null;
    process?.kill();
  }

  void _throwIfDisposed() {
    if (_disposed) {
      throw const EngineException(
        'ENGINE_CLOSED',
        'The Usque Engine client has already closed.',
      );
    }
  }

  Future<void> _startEngine() async {
    final executable = _engineExecutable();
    if (!await File(executable).exists()) {
      throw EngineException(
        'ENGINE_BINARY_MISSING',
        'The Rust Engine sidecar is missing at $executable.',
      );
    }
    final configPath = _configPath();
    await Directory(File(configPath).parent.path).create(recursive: true);
    if (Platform.isMacOS) {
      await Directory(File(_endpoint).parent.path).create(recursive: true);
    }
    final arguments = <String>[
      '--config',
      configPath,
      if (Platform.isWindows) ...<String>[
        '--pipe',
        _endpoint,
        '--parent-pid',
        '$pid',
      ],
      if (Platform.isMacOS) ...<String>['--socket', _endpoint],
    ];
    final process = await Process.start(
      executable,
      arguments,
      mode: ProcessStartMode.normal,
    );
    final stderrCapture = _BoundedStderrCapture();
    final stderrDone = Completer<void>();
    unawaited(process.stdout.drain<void>());
    process.stderr.listen(
      stderrCapture.add,
      onError: (Object _, StackTrace _) {
        if (!stderrDone.isCompleted) stderrDone.complete();
      },
      onDone: () {
        if (!stderrDone.isCompleted) stderrDone.complete();
      },
      cancelOnError: true,
    );
    final exitCode = process.exitCode;
    // Dispose may race Process.start. Never publish an orphaned sidecar.
    if (_disposed) {
      process.kill();
      throw const EngineException(
        'ENGINE_CLOSED',
        'The Usque Engine client has already closed.',
      );
    }
    _process = process;
    unawaited(
      exitCode.then((_) {
        if (identical(_process, process)) {
          _process = null;
        }
      }),
    );

    if (Platform.isWindows) {
      _EngineStartupOutcome outcome;
      try {
        outcome = await Future.any<_EngineStartupOutcome>(
          <Future<_EngineStartupOutcome>>[
            _waitForWindowsEnginePipe().then(
              (_) => const _EngineStartupOutcome.ready(),
            ),
            exitCode.then(_EngineStartupOutcome.exited),
          ],
        );
      } on Object {
        if (identical(_process, process)) _process = null;
        process.kill();
        _throwIfDisposed();
        rethrow;
      }
      _throwIfDisposed();
      final code = outcome.exitCode;
      if (code != null) {
        if (identical(_process, process)) _process = null;
        await stderrDone.future;
        final stderr = stderrCapture.text();
        throw EngineException(
          'ENGINE_START_FAILED',
          'The local Usque Engine exited during startup with code $code'
              '${stderr.isEmpty ? '.' : ': $stderr'}',
        );
      }
    }
  }

  Future<void> _waitForWindowsEnginePipe() async {
    try {
      await _nativeTransport
          .invokeMethod<void>('waitForEnginePipe', <String, Object>{
            'pipe_name': _endpoint,
            'timeout_ms': _windowsEngineReadyTimeout.inMilliseconds,
          });
    } on PlatformException catch (error) {
      throw EngineException(
        error.code,
        error.message ??
            'The local Usque Engine did not create its Named Pipe in time.',
      );
    }
  }
}

/// Reads one length-prefixed frame from a byte stream (unix socket path).
@visibleForTesting
Future<Uint8List> readFrame(Stream<List<int>> stream) async {
  final iterator = StreamIterator<List<int>>(stream);
  final buffer = BytesBuilder(copy: false);
  int? expected;
  while (await iterator.moveNext()) {
    buffer.add(iterator.current);
    final bytes = buffer.toBytes();
    if (expected == null && bytes.length >= 4) {
      expected = ByteData.sublistView(bytes).getUint32(0, Endian.big);
      if (expected > kMaximumFrameBytes) {
        throw const EngineException(
          'ENGINE_IPC_FRAME_TOO_LARGE',
          'The local Engine response exceeded 4 MiB.',
        );
      }
    }
    if (expected != null && bytes.length >= expected + 4) {
      if (bytes.length != expected + 4) {
        throw const EngineException(
          'ENGINE_IPC_INVALID_RESPONSE',
          'The local Engine returned trailing frame bytes.',
        );
      }
      return bytes;
    }
  }
  throw const EngineException(
    'ENGINE_IPC_TRUNCATED',
    'The local Engine closed an incomplete response.',
  );
}

String _engineExecutable() {
  final override = Platform.environment['USQUE_ENGINE_PATH'];
  if (kDebugMode && override != null && override.trim().isNotEmpty) {
    return override;
  }
  final appExecutable = File(Platform.resolvedExecutable);
  if (Platform.isWindows) {
    return '${appExecutable.parent.path}${Platform.pathSeparator}'
        'usque-engine.exe';
  }
  final contents = appExecutable.parent.parent;
  return '${contents.path}${Platform.pathSeparator}Resources'
      '${Platform.pathSeparator}usque-engine';
}

String _configPath() {
  if (Platform.isWindows) {
    final localAppData = Platform.environment['LOCALAPPDATA'];
    if (localAppData == null || localAppData.isEmpty) {
      throw const EngineException(
        'APP_DATA_UNAVAILABLE',
        'LOCALAPPDATA is unavailable.',
      );
    }
    return '$localAppData${Platform.pathSeparator}Usque'
        '${Platform.pathSeparator}config.json';
  }
  final home = Platform.environment['HOME'];
  if (home == null || home.isEmpty) {
    throw const EngineException('APP_DATA_UNAVAILABLE', 'HOME is unavailable.');
  }
  return '$home/Library/Application Support/Usque/config.json';
}

String _macSocketPath() {
  final home = Platform.environment['HOME'];
  if (home == null || home.isEmpty) {
    throw const EngineException('APP_DATA_UNAVAILABLE', 'HOME is unavailable.');
  }
  return '$home/Library/Caches/Usque/engine-$pid.sock';
}
