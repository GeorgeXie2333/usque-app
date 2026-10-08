import 'dart:io';
import 'dart:math';

import 'package:flutter/foundation.dart';

import 'bypass_targets.dart';

enum RoutingAction { direct, reject, proxy }

enum RoutingMatch { domain, cidr }

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

  RoutingValidation validate() {
    final normalized = <RoutingRule>[];
    final seen = <String, RoutingRule>{};
    final notices = <RoutingNotice>[];
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
      for (final other in normalized) {
        if (other.action != value.action && _overlaps(other, value)) {
          notices.add(RoutingNotice('routing_overlap', [other.id, value.id]));
        }
      }
      seen[key] = value;
      normalized.add(value);
    }
    for (final kind in RoutingMatch.values) {
      if (normalized.where((rule) => rule.kind == kind).length > 256) {
        throw const RoutingRuleError('bypass_limit', []);
      }
    }
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

bool _overlaps(RoutingRule a, RoutingRule b) {
  if (a.kind != b.kind) return false;
  if (a.kind == RoutingMatch.domain) {
    return a.target.endsWith('.${b.target}') ||
        b.target.endsWith('.${a.target}');
  }
  final left = a.target.split('/');
  final right = b.target.split('/');
  final x = InternetAddress.tryParse(left.first)?.rawAddress;
  final y = InternetAddress.tryParse(right.first)?.rawAddress;
  if (x == null || y == null || x.length != y.length) return false;
  final bits = min(int.parse(left.last), int.parse(right.last));
  for (var index = 0; index < x.length; index++) {
    final keep = (bits - index * 8).clamp(0, 8);
    final mask = (0xff << (8 - keep)) & 0xff;
    if ((x[index] & mask) != (y[index] & mask)) return false;
  }
  return true;
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
