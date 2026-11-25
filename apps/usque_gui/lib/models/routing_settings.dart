import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:flutter/foundation.dart';

import 'bypass_targets.dart';

enum RoutingAction { direct, reject, proxy }

enum RoutingMatch { domain, cidr }

/// Rule limits reported by the engine. Engines that predate the capability
/// accept 256 rules per kind and have no budget for the targets' total size.
class RoutingLimits {
  const RoutingLimits({this.perKind = legacyPerKind, this.targetBytes});

  static const legacyPerKind = 256;
  static const legacy = RoutingLimits();

  /// Pasted text beyond this many characters is refused before parsing.
  static const legacyPasteChars = 131072;

  final int perKind;

  /// UTF-8 bytes of all targets together, or `null` for no budget.
  final int? targetBytes;

  int get totalRules => perKind * 2;

  /// The legacy message names 256; the current one names 4096 and the budget.
  String get messageKey => this == legacy ? 'bypass_limit' : 'routing_limit';

  int get pasteChars => max(legacyPasteChars, (targetBytes ?? 0) * 2);

  @override
  bool operator ==(Object other) =>
      other is RoutingLimits &&
      perKind == other.perKind &&
      targetBytes == other.targetBytes;
  @override
  int get hashCode => Object.hash(perKind, targetBytes);
}

class RoutingRule {
  const RoutingRule({
    required this.id,
    required this.kind,
    required this.target,
    required this.action,
  });
  final String id;
  final RoutingMatch kind;
  final String target;
  final RoutingAction action;

  factory RoutingRule.create(
    String target,
    RoutingAction action, {
    String? id,
  }) {
    final parsed = BypassTargets.parse(target);
    if (parsed.cidrs.length + parsed.domains.length != 1) {
      throw const BypassTargetError(1, 'invalid_dns_name');
    }
    return RoutingRule(
      id: id ?? _ruleId(),
      kind: parsed.cidrs.isEmpty ? RoutingMatch.domain : RoutingMatch.cidr,
      target: parsed.cidrs.isEmpty
          ? parsed.domains.single
          : parsed.cidrs.single,
      action: action,
    );
  }

  factory RoutingRule.fromMap(Map<Object?, Object?> map) => RoutingRule(
    id: map['id'] as String,
    kind: RoutingMatch.values.byName(map['kind'] as String),
    target: map['target'] as String,
    action: RoutingAction.values.byName(map['action'] as String),
  );
  Map<String, Object?> toMap() => {
    'id': id,
    'kind': kind.name,
    'target': target,
    'action': action.name,
  };
  @override
  bool operator ==(Object other) =>
      other is RoutingRule &&
      id == other.id &&
      kind == other.kind &&
      target == other.target &&
      action == other.action;
  @override
  int get hashCode => Object.hash(id, kind, target, action);
}

class RoutingSettings {
  const RoutingSettings({this.rules = const [], this.adsEnabled = false});
  final List<RoutingRule> rules;
  final bool adsEnabled;
  factory RoutingSettings.fromMap(Map<Object?, Object?> map) => RoutingSettings(
    rules: (map['rules'] as List? ?? [])
        .map((value) => RoutingRule.fromMap(value as Map))
        .toList(growable: false),
    adsEnabled: map['ads_enabled'] == true,
  );
  Map<String, Object?> toMap() => {
    'rules': rules.map((rule) => rule.toMap()).toList(),
    'ads_enabled': adsEnabled,
  };

  RoutingValidation validate({RoutingLimits limits = RoutingLimits.legacy}) {
    final normalized = <RoutingRule>[];
    final seen = <String, RoutingRule>{};
    final notices = <RoutingNotice>[];
    final counts = <RoutingMatch, int>{};
    var targetBytes = 0;
    for (final rule in rules) {
      RoutingRule value;
      try {
        value = RoutingRule.create(rule.target, rule.action, id: rule.id);
      } on BypassTargetError catch (error) {
        throw RoutingRuleError(error.messageKey, [rule.id]);
      }
      final key = '${value.kind.name}:${value.target}';
      final previous = seen[key];
      if (previous != null) {
        if (previous.action != value.action) {
          throw RoutingRuleError('routing_conflict', [previous.id, value.id]);
        }
        notices.add(
          RoutingNotice('routing_duplicate', [previous.id, value.id]),
        );
        continue;
      }
      seen[key] = value;
      normalized.add(value);
      counts[value.kind] = (counts[value.kind] ?? 0) + 1;
      targetBytes += utf8.encode(value.target).length;
    }
    final budget = limits.targetBytes;
    if (counts.values.any((count) => count > limits.perKind) ||
        (budget != null && targetBytes > budget)) {
      throw RoutingRuleError(limits.messageKey, const []);
    }
    notices.addAll(_overlapNotices(normalized));
    return RoutingValidation(
      RoutingSettings(rules: normalized, adsEnabled: adsEnabled),
      notices,
    );
  }

  @override
  bool operator ==(Object other) =>
      other is RoutingSettings &&
      adsEnabled == other.adsEnabled &&
      listEquals(rules, other.rules);
  @override
  int get hashCode => Object.hash(adsEnabled, Object.hashAll(rules));
}

class RoutingValidation {
  const RoutingValidation(this.settings, this.notices);
  final RoutingSettings settings;
  final List<RoutingNotice> notices;
}

class RoutingNotice {
  const RoutingNotice(this.key, this.ids);
  final String key;
  final List<String> ids;
}

class RoutingRuleError implements Exception {
  const RoutingRuleError(this.key, this.ids);
  final String key;
  final List<String> ids;
}

/// Pairs of rules with different actions where one target contains the
/// other. Each rule is checked only against its own ancestors (parent domains,
/// shorter prefixes present in the list), so the cost grows with the number of
/// rules times the label or prefix depth instead of with every pair.
List<RoutingNotice> _overlapNotices(List<RoutingRule> rules) {
  final position = <String, int>{
    for (var index = 0; index < rules.length; index++) rules[index].id: index,
  };
  final domains = <String, RoutingRule>{};
  final networks = <String, RoutingRule>{};
  final prefixes = <int, Set<int>>{};
  final parsed = <RoutingRule, (Uint8List, int)>{};
  for (final rule in rules) {
    if (rule.kind == RoutingMatch.domain) {
      domains[rule.target] = rule;
      continue;
    }
    final parts = rule.target.split('/');
    final address = InternetAddress.tryParse(parts.first)?.rawAddress;
    final prefix = int.tryParse(parts.last);
    if (address == null || prefix == null) continue;
    parsed[rule] = (address, prefix);
    networks[_networkKey(address, prefix)] = rule;
    (prefixes[address.length] ??= <int>{}).add(prefix);
  }
  final notices = <RoutingNotice>[];
  void pair(RoutingRule ancestor, RoutingRule rule) {
    if (ancestor.action == rule.action) return;
    final ordered = position[ancestor.id]! < position[rule.id]!
        ? [ancestor.id, rule.id]
        : [rule.id, ancestor.id];
    notices.add(RoutingNotice('routing_overlap', ordered));
  }

  for (final rule in rules) {
    if (rule.kind == RoutingMatch.domain) {
      var suffix = rule.target;
      for (var dot = suffix.indexOf('.'); dot >= 0; dot = suffix.indexOf('.')) {
        suffix = suffix.substring(dot + 1);
        final ancestor = domains[suffix];
        if (ancestor != null) pair(ancestor, rule);
      }
      continue;
    }
    final network = parsed[rule];
    if (network == null) continue;
    final (address, prefix) = network;
    for (final shorter in prefixes[address.length]!) {
      if (shorter >= prefix) continue;
      final ancestor = networks[_networkKey(address, shorter)];
      if (ancestor != null) pair(ancestor, rule);
    }
  }
  notices.sort((a, b) {
    final first = position[a.ids[0]]!.compareTo(position[b.ids[0]]!);
    return first != 0
        ? first
        : position[a.ids[1]]!.compareTo(position[b.ids[1]]!);
  });
  return notices;
}

String _networkKey(Uint8List address, int prefix) {
  final masked = Uint8List.fromList(address);
  for (var index = 0; index < masked.length; index++) {
    final keep = (prefix - index * 8).clamp(0, 8);
    masked[index] &= (0xff << (8 - keep)) & 0xff;
  }
  return '${masked.length}/$prefix/${masked.join('.')}';
}

String _ruleId() {
  final random = Random.secure();
  final bytes = List.generate(16, (_) => random.nextInt(256));
  bytes[6] = (bytes[6] & 15) | 64;
  bytes[8] = (bytes[8] & 63) | 128;
  final hex = bytes
      .map((byte) => byte.toRadixString(16).padLeft(2, '0'))
      .join();
  return '${hex.substring(0, 8)}-${hex.substring(8, 12)}-${hex.substring(12, 16)}-${hex.substring(16, 20)}-${hex.substring(20)}';
}
