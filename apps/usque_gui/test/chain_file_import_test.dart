import 'dart:convert';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/services/engine_client.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = 'io.github.georgexie2333.usque/engine';
  const codec = StandardMethodCodec();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  void reply(Uint8List? bytes) {
    messenger.setMockMessageHandler(channel, (message) async {
      expect(
        codec.decodeMethodCall(message!).method,
        'readChainConfigurations',
      );
      // The production Flutter engine returns an unmodifiable platform reply.
      return codec
          .encodeSuccessEnvelope(
            bytes == null
                ? null
                : [
                    {'name': 'sample.conf', 'bytes': bytes},
                  ],
          )
          .asUnmodifiableView();
    });
  }

  tearDown(() => messenger.setMockMessageHandler(channel, null));

  test(
    'batch preserves names, order and independent read/decode failures',
    () async {
      messenger.setMockMessageHandler(
        channel,
        (_) async => codec.encodeSuccessEnvelope([
          {
            'name': 'one.conf',
            'bytes': Uint8List.fromList(utf8.encode('first')),
          },
          {'name': 'bad.conf', 'error': 'CHAIN_FILE_READ_FAILED'},
          {
            'name': 'encoding.conf',
            'bytes': Uint8List.fromList([0xc3, 0x28]),
          },
          {
            'name': 'two.conf',
            'bytes': Uint8List.fromList(utf8.encode('second')),
          },
        ]).asUnmodifiableView(),
      );
      final files = await pickChainConfigurationFiles();
      expect(files.map((f) => f.name), [
        'one.conf',
        'bad.conf',
        'encoding.conf',
        'two.conf',
      ]);
      expect(files.map((f) => f.configuration), [
        'first',
        null,
        null,
        'second',
      ]);
      expect(files[1].errorCode, 'CHAIN_FILE_READ_FAILED');
      expect(files[2].errorCode, 'CHAIN_FILE_ENCODING_INVALID');
    },
  );

  test('batch count boundary is enforced before decoding content', () async {
    for (final count in [128, 129]) {
      messenger.setMockMessageHandler(
        channel,
        (_) async => codec.encodeSuccessEnvelope(
          List.generate(
            count,
            (i) => {'name': '$i.conf', 'error': 'CHAIN_FILE_READ_FAILED'},
          ),
        ),
      );
      if (count == 128) {
        expect(await pickChainConfigurationFiles(), hasLength(128));
      } else {
        await expectLater(
          pickChainConfigurationFiles(),
          throwsA(
            isA<EngineException>().having(
              (e) => e.code,
              'code',
              'CHAIN_FILE_COUNT_LIMIT',
            ),
          ),
        );
      }
    }
  });

  for (final text in [
    'client\ndev tun\nauth-user-pass\n',
    '[Interface]\nAddress = 10.8.0.2/32\n[Peer]\n',
  ]) {
    test(
      'file import decodes an immutable reply: ${text.split('\n').first}',
      () async {
        final bytes = Uint8List.fromList(utf8.encode(text));
        reply(bytes);
        expect(
          (await pickChainConfigurationFiles()).single.configuration,
          text,
        );
        expect(utf8.decode(bytes), text);
      },
    );
  }

  test('file cancellation returns no configuration', () async {
    reply(null);
    expect(await pickChainConfigurationFiles(), isEmpty);
  });

  test('invalid UTF-8 has an encoding error, not a cleanup error', () async {
    reply(Uint8List.fromList([0xc3, 0x28]));
    expect(
      (await pickChainConfigurationFiles()).single.errorCode,
      'CHAIN_FILE_ENCODING_INVALID',
    );
  });

  for (final code in [
    'CHAIN_FILE_UNAVAILABLE',
    'CHAIN_FILE_READ_FAILED',
    'CHAIN_FILE_COUNT_LIMIT',
    'CHAIN_FILE_BUSY',
  ]) {
    test('native picker preserves the error category $code', () async {
      messenger.setMockMessageHandler(
        channel,
        (_) async => codec.encodeErrorEnvelope(code: code),
      );
      await expectLater(
        pickChainConfigurationFiles(),
        throwsA(isA<EngineException>().having((e) => e.code, 'code', code)),
      );
    });
  }
  test('missing plugin is distinct from an empty file', () async {
    messenger.setMockMessageHandler(channel, (_) async => null);
    await expectLater(
      pickChainConfigurationFiles(),
      throwsA(
        isA<EngineException>().having(
          (e) => e.code,
          'code',
          'CHAIN_FILE_UNAVAILABLE',
        ),
      ),
    );
    reply(Uint8List(0));
    expect(
      (await pickChainConfigurationFiles()).single.errorCode,
      'CHAIN_FILE_READ_FAILED',
    );
  });
  test('the exact UTF-8 size boundary is accepted', () async {
    reply(Uint8List.fromList(List.filled(128 * 1024, 65)));
    expect(
      (await pickChainConfigurationFiles()).single.configuration!.length,
      128 * 1024,
    );
  });

  test('oversized file has a size error, not a cleanup error', () async {
    reply(Uint8List(128 * 1024 + 1));
    expect(
      (await pickChainConfigurationFiles()).single.errorCode,
      'CHAIN_FILE_TOO_LARGE',
    );
  });
}
