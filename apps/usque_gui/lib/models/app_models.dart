import 'package:flutter/foundation.dart';

enum AppSection { home, profiles, proxy, settings, diagnostics }

enum ConnectionPhase {
  disconnected,
  preparing,
  connectingH3,
  connectingH2,
  connected,
  degraded,
  reconnecting,
  disconnecting,
  captivePortalPaused,
  error,
}

enum OperatingMode { vpn, socks5, httpProxy }

enum TransportPolicy { automatic, http3, http2 }

enum IpPolicy { automatic, preferIpv4, preferIpv6, ipv4Only, ipv6Only }

enum DnsMode { tunnel, localConfigured, system }

enum ProxyDnsMode { remote, localConfigured, system }

enum IdentityProvisioningMethod { register, importSecret }

enum ProfileIdentityState { ready, missing, invalid }

enum ThemePreference { system, light, dark }

enum LocalePreference { system, english, simplifiedChinese }

class UpdateCheckResult {
  const UpdateCheckResult({
    required this.available,
    this.version,
    this.releaseUrl,
  });

  const UpdateCheckResult.current()
    : available = false,
      version = null,
      releaseUrl = null;

  final bool available;
  final String? version;
  final String? releaseUrl;

  factory UpdateCheckResult.fromMap(Map<Object?, Object?> map) {
    return UpdateCheckResult(
      available: map['available'] as bool? ?? false,
      version: map['version'] as String?,
      releaseUrl: map['release_url'] as String?,
    );
  }
}

class ProfileCatalog {
  const ProfileCatalog({
    required this.profiles,
    required this.activeProfileId,
    this.identityStates = const <String, ProfileIdentityState>{},
  });

  final List<UsqueProfile> profiles;
  final String activeProfileId;
  final Map<String, ProfileIdentityState> identityStates;
}

class ProxySettings {
  const ProxySettings({
    this.socksIpv4 = '127.0.0.1',
    this.socksIpv6 = '::1',
    this.socksPort = 1080,
    this.httpIpv4 = '127.0.0.1',
    this.httpIpv6 = '::1',
    this.httpPort = 8080,
    this.dnsMode = ProxyDnsMode.remote,
    this.systemProxy = false,
  });

  final String socksIpv4;
  final String socksIpv6;
  final int socksPort;
  final String httpIpv4;
  final String httpIpv6;
  final int httpPort;
  final ProxyDnsMode dnsMode;
  final bool systemProxy;

  bool get remoteDns => dnsMode == ProxyDnsMode.remote;

  bool get exposesLan {
    final addresses = <String>[socksIpv4, socksIpv6, httpIpv4, httpIpv6];
    return addresses.any(
      (address) =>
          address != '127.0.0.1' &&
          address != '::1' &&
          address.toLowerCase() != 'localhost',
    );
  }

  ProxySettings copyWith({
    String? socksIpv4,
    String? socksIpv6,
    int? socksPort,
    String? httpIpv4,
    String? httpIpv6,
    int? httpPort,
    ProxyDnsMode? dnsMode,
    bool? systemProxy,
  }) {
    return ProxySettings(
      socksIpv4: socksIpv4 ?? this.socksIpv4,
      socksIpv6: socksIpv6 ?? this.socksIpv6,
      socksPort: socksPort ?? this.socksPort,
      httpIpv4: httpIpv4 ?? this.httpIpv4,
      httpIpv6: httpIpv6 ?? this.httpIpv6,
      httpPort: httpPort ?? this.httpPort,
      dnsMode: dnsMode ?? this.dnsMode,
      systemProxy: systemProxy ?? this.systemProxy,
    );
  }

  factory ProxySettings.fromMap(Map<String, Object?> map) {
    return ProxySettings(
      socksIpv4: _string(map, 'socks_ipv4'),
      socksIpv6: _string(map, 'socks_ipv6'),
      socksPort: _boundedInt(map, 'socks_port', 1, 65535),
      httpIpv4: _string(map, 'http_ipv4'),
      httpIpv6: _string(map, 'http_ipv6'),
      httpPort: _boundedInt(map, 'http_port', 1, 65535),
      dnsMode: _enumByName(ProxyDnsMode.values, _string(map, 'dns_mode')),
      systemProxy: _bool(map, 'system_proxy'),
    );
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'socks_ipv4': socksIpv4,
      'socks_ipv6': socksIpv6,
      'socks_port': socksPort,
      'http_ipv4': httpIpv4,
      'http_ipv6': httpIpv6,
      'http_port': httpPort,
      'dns_mode': dnsMode.name,
      'system_proxy': systemProxy,
    };
  }
}

class UsqueProfile {
  const UsqueProfile({
    required this.id,
    required this.name,
    this.mode = OperatingMode.vpn,
    this.transport = TransportPolicy.automatic,
    this.ipPolicy = IpPolicy.automatic,
    this.endpointIpv4 = defaultEndpointIpv4,
    this.endpointIpv6 = defaultEndpointIpv6,
    this.endpointPort = defaultEndpointPort,
    this.sni = defaultSni,
    this.mtu = defaultMtu,
    this.dnsIpv4 = defaultDnsIpv4,
    this.dnsIpv6 = defaultDnsIpv6,
    this.dnsMode = DnsMode.tunnel,
    this.killSwitch = true,
    this.allowLan = false,
    this.autoConnect = false,
    this.bypassCidrs = const <String>[],
    this.proxy = const ProxySettings(),
  });

  static const defaultEndpointIpv4 = '162.159.198.2';
  static const defaultEndpointIpv6 = '2606:4700:103::2';
  static const defaultEndpointPort = 443;
  static const defaultSni = 'www.visa.cn';
  static const defaultMtu = 1280;
  static const defaultDnsIpv4 = '1.1.1.1';
  static const defaultDnsIpv6 = '2606:4700:4700::1111';
  static const defaultProfileId = '8c30b771-9ebd-457a-b67b-bbc74a1ddba6';

  final String id;
  final String name;
  final OperatingMode mode;
  final TransportPolicy transport;
  final IpPolicy ipPolicy;
  final String endpointIpv4;
  final String endpointIpv6;
  final int endpointPort;
  final String sni;
  final int mtu;
  final String dnsIpv4;
  final String dnsIpv6;
  final DnsMode dnsMode;
  final bool killSwitch;
  final bool allowLan;
  final bool autoConnect;
  final List<String> bypassCidrs;
  final ProxySettings proxy;

  factory UsqueProfile.defaultProfile() {
    return const UsqueProfile(id: defaultProfileId, name: 'Default');
  }

  UsqueProfile resetAdvancedDefaults() {
    return copyWith(
      transport: TransportPolicy.automatic,
      ipPolicy: IpPolicy.automatic,
      endpointIpv4: defaultEndpointIpv4,
      endpointIpv6: defaultEndpointIpv6,
      endpointPort: defaultEndpointPort,
      sni: defaultSni,
      mtu: defaultMtu,
      dnsIpv4: defaultDnsIpv4,
      dnsIpv6: defaultDnsIpv6,
      dnsMode: DnsMode.tunnel,
      allowLan: false,
      bypassCidrs: const <String>[],
      proxy: const ProxySettings(),
    );
  }

  UsqueProfile copyWith({
    String? id,
    String? name,
    OperatingMode? mode,
    TransportPolicy? transport,
    IpPolicy? ipPolicy,
    String? endpointIpv4,
    String? endpointIpv6,
    int? endpointPort,
    String? sni,
    int? mtu,
    String? dnsIpv4,
    String? dnsIpv6,
    DnsMode? dnsMode,
    bool? killSwitch,
    bool? allowLan,
    bool? autoConnect,
    List<String>? bypassCidrs,
    ProxySettings? proxy,
  }) {
    return UsqueProfile(
      id: id ?? this.id,
      name: name ?? this.name,
      mode: mode ?? this.mode,
      transport: transport ?? this.transport,
      ipPolicy: ipPolicy ?? this.ipPolicy,
      endpointIpv4: endpointIpv4 ?? this.endpointIpv4,
      endpointIpv6: endpointIpv6 ?? this.endpointIpv6,
      endpointPort: endpointPort ?? this.endpointPort,
      sni: sni ?? this.sni,
      mtu: mtu ?? this.mtu,
      dnsIpv4: dnsIpv4 ?? this.dnsIpv4,
      dnsIpv6: dnsIpv6 ?? this.dnsIpv6,
      dnsMode: dnsMode ?? this.dnsMode,
      killSwitch: killSwitch ?? this.killSwitch,
      allowLan: allowLan ?? this.allowLan,
      autoConnect: autoConnect ?? this.autoConnect,
      bypassCidrs: bypassCidrs ?? this.bypassCidrs,
      proxy: proxy ?? this.proxy,
    );
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'id': id,
      'name': name,
      'mode': mode.name,
      'transport': transport.name,
      'ip_policy': ipPolicy.name,
      'endpoint_v4': endpointIpv4,
      'endpoint_v6': endpointIpv6,
      'endpoint_port': endpointPort,
      'sni': sni,
      'mtu': mtu,
      'dns_v4': dnsIpv4,
      'dns_v6': dnsIpv6,
      'dns_mode': dnsMode.name,
      'kill_switch': killSwitch,
      'allow_lan': allowLan,
      'auto_connect': autoConnect,
      'bypass_cidrs': bypassCidrs,
      'proxy': proxy.toMap(),
    };
  }

  factory UsqueProfile.fromMap(Map<String, Object?> map) {
    final id = _string(map, 'id').trim();
    final name = _string(map, 'name').trim();
    if (!RegExp(
      r'^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-'
      r'[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$',
    ).hasMatch(id)) {
      throw const FormatException('Invalid profile ID');
    }
    if (name.isEmpty || name.runes.length > 64) {
      throw const FormatException('Invalid profile name');
    }
    final bypass = _stringList(map, 'bypass_cidrs');
    if (bypass.length > 256) {
      throw const FormatException('Too many bypass routes');
    }
    final proxy = map['proxy'];
    if (proxy is! Map) {
      throw const FormatException('Missing proxy settings');
    }

    return UsqueProfile(
      id: id,
      name: name,
      mode: _enumByName(OperatingMode.values, _string(map, 'mode')),
      transport: _enumByName(TransportPolicy.values, _string(map, 'transport')),
      ipPolicy: _enumByName(IpPolicy.values, _string(map, 'ip_policy')),
      endpointIpv4: _string(map, 'endpoint_v4'),
      endpointIpv6: _string(map, 'endpoint_v6'),
      endpointPort: _boundedInt(map, 'endpoint_port', 1, 65535),
      sni: _string(map, 'sni'),
      mtu: _boundedInt(map, 'mtu', 1280, 9000),
      dnsIpv4: _string(map, 'dns_v4'),
      dnsIpv6: _string(map, 'dns_v6'),
      dnsMode: _enumByName(DnsMode.values, _string(map, 'dns_mode')),
      killSwitch: _bool(map, 'kill_switch'),
      allowLan: _bool(map, 'allow_lan'),
      autoConnect: _bool(map, 'auto_connect'),
      bypassCidrs: List<String>.unmodifiable(bypass),
      proxy: ProxySettings.fromMap(Map<String, Object?>.from(proxy)),
    );
  }
}

String _string(Map<String, Object?> map, String key) {
  final value = map[key];
  if (value is! String || value.length > 4096) {
    throw FormatException('Invalid $key');
  }
  return value;
}

bool _bool(Map<String, Object?> map, String key) {
  final value = map[key];
  if (value is! bool) {
    throw FormatException('Invalid $key');
  }
  return value;
}

int _boundedInt(
  Map<String, Object?> map,
  String key,
  int minimum,
  int maximum,
) {
  final value = map[key];
  if (value is! int || value < minimum || value > maximum) {
    throw FormatException('Invalid $key');
  }
  return value;
}

List<String> _stringList(Map<String, Object?> map, String key) {
  final value = map[key];
  if (value is! List) {
    throw FormatException('Invalid $key');
  }
  return value
      .map((item) {
        if (item is! String || item.length > 128) {
          throw FormatException('Invalid $key entry');
        }
        return item;
      })
      .toList(growable: false);
}

T _enumByName<T extends Enum>(List<T> values, String name) {
  for (final value in values) {
    if (value.name == name) {
      return value;
    }
  }
  throw FormatException('Unknown enum value: $name');
}

class ExitInfo {
  const ExitInfo({
    this.city,
    this.country,
    this.countryCode,
    this.flagSvg,
    this.ipv4,
    this.ipv6,
  });

  final String? city;
  final String? country;
  final String? countryCode;

  /// SVG bytes fetched through the tunnel and returned from the native cache.
  final String? flagSvg;
  final String? ipv4;
  final String? ipv6;

  bool get hasLocation => city != null || country != null;

  String get location {
    return <String?>[
      city,
      country,
    ].whereType<String>().where((value) => value.isNotEmpty).join(', ');
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is ExitInfo &&
            city == other.city &&
            country == other.country &&
            countryCode == other.countryCode &&
            flagSvg == other.flagSvg &&
            ipv4 == other.ipv4 &&
            ipv6 == other.ipv6;
  }

  @override
  int get hashCode =>
      Object.hash(city, country, countryCode, flagSvg, ipv4, ipv6);
}

class EngineSnapshot {
  const EngineSnapshot({
    this.phase = ConnectionPhase.disconnected,
    this.transport,
    this.addressFamily,
    this.connectedAt,
    this.downloadBytesPerSecond = 0,
    this.uploadBytesPerSecond = 0,
    this.downloadedBytes = 0,
    this.uploadedBytes = 0,
    this.reconnectCount = 0,
    this.activeListeners = const <String>[],
    this.killSwitchState,
    this.platformLockdown = false,
    this.alwaysOn = false,
    this.captivePauseRemainingSeconds = 0,
    this.exit = const ExitInfo(),
    this.warning,
    this.errorCode,
  });

  final ConnectionPhase phase;
  final String? transport;
  final String? addressFamily;
  final DateTime? connectedAt;
  final int downloadBytesPerSecond;
  final int uploadBytesPerSecond;
  final int downloadedBytes;
  final int uploadedBytes;
  final int reconnectCount;
  final List<String> activeListeners;
  final String? killSwitchState;
  final bool platformLockdown;
  final bool alwaysOn;
  final int captivePauseRemainingSeconds;
  final ExitInfo exit;
  final String? warning;
  final String? errorCode;

  bool get isConnected =>
      phase == ConnectionPhase.connected ||
      phase == ConnectionPhase.degraded ||
      phase == ConnectionPhase.captivePortalPaused;

  bool get isTransitional =>
      phase == ConnectionPhase.preparing ||
      phase == ConnectionPhase.connectingH3 ||
      phase == ConnectionPhase.connectingH2 ||
      phase == ConnectionPhase.reconnecting ||
      phase == ConnectionPhase.disconnecting;

  factory EngineSnapshot.fromMap(Map<Object?, Object?> map) {
    ConnectionPhase parsePhase(String? value) {
      return ConnectionPhase.values.firstWhere(
        (phase) => phase.name == value,
        orElse: () => ConnectionPhase.error,
      );
    }

    final connectedAt = map['connected_at'] as String?;
    return EngineSnapshot(
      phase: parsePhase(map['phase'] as String?),
      transport: map['transport'] as String?,
      addressFamily: map['address_family'] as String?,
      connectedAt: connectedAt == null ? null : DateTime.tryParse(connectedAt),
      downloadBytesPerSecond:
          (map['download_bytes_per_second'] as num?)?.toInt() ?? 0,
      uploadBytesPerSecond:
          (map['upload_bytes_per_second'] as num?)?.toInt() ?? 0,
      downloadedBytes: (map['downloaded_bytes'] as num?)?.toInt() ?? 0,
      uploadedBytes: (map['uploaded_bytes'] as num?)?.toInt() ?? 0,
      reconnectCount: (map['reconnect_count'] as num?)?.toInt() ?? 0,
      activeListeners:
          (map['active_listeners'] as List?)?.whereType<String>().toList(
            growable: false,
          ) ??
          const <String>[],
      killSwitchState: map['kill_switch_state'] as String?,
      platformLockdown: map['platform_lockdown'] as bool? ?? false,
      alwaysOn: map['always_on'] as bool? ?? false,
      captivePauseRemainingSeconds:
          (map['captive_pause_remaining_seconds'] as num?)?.toInt() ?? 0,
      exit: ExitInfo(
        city: map['exit_city'] as String?,
        country: map['exit_country'] as String?,
        countryCode: map['exit_country_code'] as String?,
        flagSvg: map['exit_flag_svg'] as String?,
        ipv4: map['exit_ipv4'] as String?,
        ipv6: map['exit_ipv6'] as String?,
      ),
      warning: map['warning'] as String?,
      errorCode: map['error_code'] as String?,
    );
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is EngineSnapshot &&
            phase == other.phase &&
            transport == other.transport &&
            addressFamily == other.addressFamily &&
            connectedAt == other.connectedAt &&
            downloadBytesPerSecond == other.downloadBytesPerSecond &&
            uploadBytesPerSecond == other.uploadBytesPerSecond &&
            downloadedBytes == other.downloadedBytes &&
            uploadedBytes == other.uploadedBytes &&
            reconnectCount == other.reconnectCount &&
            listEquals(activeListeners, other.activeListeners) &&
            killSwitchState == other.killSwitchState &&
            platformLockdown == other.platformLockdown &&
            alwaysOn == other.alwaysOn &&
            captivePauseRemainingSeconds ==
                other.captivePauseRemainingSeconds &&
            exit == other.exit &&
            warning == other.warning &&
            errorCode == other.errorCode;
  }

  @override
  int get hashCode => Object.hashAll(<Object?>[
    phase,
    transport,
    addressFamily,
    connectedAt,
    downloadBytesPerSecond,
    uploadBytesPerSecond,
    downloadedBytes,
    uploadedBytes,
    reconnectCount,
    Object.hashAll(activeListeners),
    killSwitchState,
    platformLockdown,
    alwaysOn,
    captivePauseRemainingSeconds,
    exit,
    warning,
    errorCode,
  ]);
}
