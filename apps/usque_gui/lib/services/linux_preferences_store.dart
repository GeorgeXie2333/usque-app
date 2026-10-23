import 'dart:async';
import 'dart:convert';
import 'dart:isolate';

import 'package:flutter/services.dart';
import 'package:shared_preferences_platform_interface/shared_preferences_platform_interface.dart';
import 'package:shared_preferences_platform_interface/types.dart';

// Top-level boundaries prevent a closure from capturing the store's Future
// queue (which cannot cross an isolate boundary).
Future<String> _encodePreferences(Map<String, Object?> value) =>
    Isolate.run(() => jsonEncode(value));

Future<Map<String, Object?>> _decodePreferences(String raw) => Isolate.run(() {
  final value = jsonDecode(raw);
  if (value is! Map<String, dynamic>) {
    throw const FormatException('Invalid Linux preferences');
  }
  return Map<String, Object?>.from(value);
});

/// Linux production storage: filesystem work belongs to native workers, and
/// potentially large JSON belongs to an isolate. The UI owns only the cache.
class LinuxPreferencesStore extends SharedPreferencesStorePlatform {
  static const _channel = MethodChannel('io.github.georgexie2333.usque/engine');
  Map<String, Object?>? _values;
  Future<void> _tail = Future<void>.value();

  static void register() {
    SharedPreferencesStorePlatform.instance = LinuxPreferencesStore();
  }

  static Future<void> flushPending() async {
    final store = SharedPreferencesStorePlatform.instance;
    if (store is LinuxPreferencesStore) await store._tail;
  }

  Future<T> _serialized<T>(Future<T> Function() action) {
    final result = _tail.then((_) => action());
    _tail = result.then<void>((_) {}, onError: (Object _, StackTrace _) {});
    return result;
  }

  Future<Map<String, Object?>> _load() async {
    if (_values != null) return _values!;
    final raw = await _channel.invokeMethod<String>('readLinuxPreferences');
    if (raw == null) throw StateError('Linux preferences are unavailable');
    final decoded = await _decodePreferences(raw);
    return _values = decoded;
  }

  Future<bool> _change(void Function(Map<String, Object?>) change) =>
      _serialized(() async {
        final before = await _load();
        final next = Map<String, Object?>.from(before);
        change(next);
        // No write for absent migration keys or unchanged settings.
        final previous = await _encodePreferences(before);
        final raw = await _encodePreferences(next);
        if (raw == previous) return true;
        final saved = await _channel.invokeMethod<bool>(
          'writeLinuxPreferences',
          raw,
        );
        if (saved != true) return false;
        _values = next;
        return true;
      });

  @override
  Future<bool> remove(String key) => _change((values) => values.remove(key));

  @override
  Future<bool> setValue(String valueType, String key, Object value) =>
      _change((values) => values[key] = value);

  @override
  Future<bool> clear() => clearWithPrefix('flutter.');

  @override
  Future<bool> clearWithPrefix(String prefix) => clearWithParameters(
    ClearParameters(filter: PreferencesFilter(prefix: prefix)),
  );

  @override
  Future<bool> clearWithParameters(ClearParameters parameters) => _change(
    (values) => values.removeWhere(
      (key, _) =>
          key.startsWith(parameters.filter.prefix) &&
          (parameters.filter.allowList?.contains(key) ?? true),
    ),
  );

  @override
  Future<Map<String, Object>> getAll() => getAllWithPrefix('flutter.');

  @override
  Future<Map<String, Object>> getAllWithPrefix(String prefix) =>
      getAllWithParameters(
        GetAllParameters(filter: PreferencesFilter(prefix: prefix)),
      );

  @override
  Future<Map<String, Object>> getAllWithParameters(
    GetAllParameters parameters,
  ) => _serialized(() async {
    final values = await _load();
    final selected = <String, Object>{};
    for (final entry in values.entries) {
      if (!entry.key.startsWith(parameters.filter.prefix) ||
          !(parameters.filter.allowList?.contains(entry.key) ?? true)) {
        continue;
      }
      final value = entry.value;
      if (value is! bool &&
          value is! num &&
          value is! String &&
          !(value is List && value.every((element) => element is String))) {
        throw const FormatException('Unsupported Linux preference value');
      }
      selected[entry.key] = value!;
    }
    return selected;
  });
}
