import 'package:flutter/foundation.dart';

import 'diagnostics_models.dart';

enum AppSection { home, profiles, proxy, settings }

enum ConnectionPhase {
  disconnected,
  preparing,
  connectingH3,
  connectingH2,
  connected,
  degraded,
  reconnecting,
  disconnecting,
  error,
}

enum OperatingMode { vpn, socks5, httpProxy }

enum TransportPolicy { automatic, http3, http2 }

enum IpPolicy { automatic, preferIpv4, preferIpv6, ipv4Only, ipv6Only }

enum DnsMode { tunnel, localConfigured, system }

enum ProxyDnsMode { remote, localConfigured, system }

enum DirectDnsMode { unknown, physicalSystem, doh, dot }

class DirectDnsSettings {
  const DirectDnsSettings({
    this.mode = DirectDnsMode.physicalSystem,
    this.serverName = '',
    this.dohPath = '',
    this.bootstrapIps = const <String>[],
    this.port = 0,
  });

  final DirectDnsMode mode;
  final String serverName;
  final String dohPath;
  final List<String> bootstrapIps;
  final int port;

  DirectDnsSettings copyWith({
    DirectDnsMode? mode,
    String? serverName,
    String? dohPath,
    List<String>? bootstrapIps,
    int? port,
  }) {
    return DirectDnsSettings(
      mode: mode ?? this.mode,
      serverName: serverName ?? this.serverName,
      dohPath: dohPath ?? this.dohPath,
      bootstrapIps: bootstrapIps ?? this.bootstrapIps,
      port: port ?? this.port,
    );
  }

  factory DirectDnsSettings.fromMap(Map<String, Object?> map) {
    final modeName = map['mode'] as String? ?? 'physicalSystem';
    final mode = DirectDnsMode.values.firstWhere(
      (value) => value.name == modeName,
      orElse: () => DirectDnsMode.unknown,
    );
    return DirectDnsSettings(
      mode: mode,
      serverName: _stringOr(map, 'server_name', ''),
      dohPath: _stringOr(map, 'doh_path', ''),
      bootstrapIps: List<String>.unmodifiable(
        map.containsKey('bootstrap_ips')
            ? _stringList(map, 'bootstrap_ips')
            : const <String>[],
      ),
      port: (map['port'] as num?)?.toInt() ?? 0,
    );
  }

  Map<String, Object?> toMap() => <String, Object?>{
    'mode': mode.name,
    'server_name': serverName,
    'doh_path': dohPath,
    'bootstrap_ips': bootstrapIps,
    'port': port,
  };

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is DirectDnsSettings &&
          mode == other.mode &&
          serverName == other.serverName &&
          dohPath == other.dohPath &&
          listEquals(bootstrapIps, other.bootstrapIps) &&
          port == other.port;

  @override
  int get hashCode => Object.hash(
    mode,
    serverName,
    dohPath,
    Object.hashAll(bootstrapIps),
    port,
  );
}

enum IdentityProvisioningMethod { register, registerWithLicense, zeroTrust }

enum IdentityProvider { consumer, zeroTrust }

enum ProfileIdentityState { ready, missing, invalid }

enum LicenseState { free, warpPlus, unknown, cleanupPending, notApplicable }

enum FrontendKind { tunnel, socks5, http, systemProxy }

enum FrontendPhase {
  disabled,
  preparing,
  active,
  degraded,
  reconnecting,
  error,
}

enum ThemePreference { system, light, dark }

enum LocalePreference {
  system,
  english,
  simplifiedChinese,
  traditionalChineseHongKong,
  traditionalChineseTaiwan,
  japanese,
  korean,
  spanish,
  portuguese,
  french,
  dutch,
  turkish,
  russian,
  persian,
  arabic,
  german,
  indonesian,
  italian,
  polish,
  thai,
  ukrainian,
  vietnamese;

  /// Language picker order: System first, then English names A–Z with Chinese
  /// variants grouped under Chinese.
  static const List<LocalePreference> pickerOrder = <LocalePreference>[
    system,
    arabic,
    simplifiedChinese,
    traditionalChineseHongKong,
    traditionalChineseTaiwan,
    dutch,
    english,
    french,
    german,
    indonesian,
    italian,
    japanese,
    korean,
    persian,
    polish,
    portuguese,
    russian,
    spanish,
    thai,
    turkish,
    ukrainian,
    vietnamese,
  ];

  String get languageLabelKey => switch (this) {
    system => 'language_system',
    english => 'language_en',
    simplifiedChinese => 'language_zh',
    traditionalChineseHongKong => 'language_zh_hk',
    traditionalChineseTaiwan => 'language_zh_tw',
    japanese => 'language_ja',
    korean => 'language_ko',
    spanish => 'language_es',
    portuguese => 'language_pt',
    french => 'language_fr',
    dutch => 'language_nl',
    turkish => 'language_tr',
    russian => 'language_ru',
    persian => 'language_fa',
    arabic => 'language_ar',
    german => 'language_de',
    indonesian => 'language_id',
    italian => 'language_it',
    polish => 'language_pl',
    thai => 'language_th',
    ukrainian => 'language_uk',
    vietnamese => 'language_vi',
  };
}

class GeoRulesEntry {
  const GeoRulesEntry({
    required this.countryCode,
    this.hasGeoip = false,
    this.hasGeosite = false,
    this.lastUpdatedUnixMilliseconds = 0,
  });

  final String countryCode;
  final bool hasGeoip;
  final bool hasGeosite;
  final int lastUpdatedUnixMilliseconds;
}

class GeoRulesList {
  const GeoRulesList({
    this.entries = const <GeoRulesEntry>[],
    this.lastSuccessfulUpdateUnixMilliseconds = 0,
    this.hasGlobalGeosite = false,
    this.globalGeositeUpdatedUnixMilliseconds = 0,
  });

  final List<GeoRulesEntry> entries;
  final int lastSuccessfulUpdateUnixMilliseconds;
  final bool hasGlobalGeosite;
  final int globalGeositeUpdatedUnixMilliseconds;
}

enum GeoRulesUpdateStatus { upToDate, updated, failed }

class GeoRulesUpdateResult {
  const GeoRulesUpdateResult({
    required this.countryCode,
    required this.status,
    this.reason = '',
    this.artifactKind = '',
    this.artifactScope = '',
  });

  final String countryCode;
  final GeoRulesUpdateStatus status;
  final String reason;
  final String artifactKind;
  final String artifactScope;
}

class GeoRulesProgress {
  const GeoRulesProgress({
    this.currentFile = '',
    this.completed = 0,
    this.total = 0,
  });

  final String currentFile;
  final int completed;
  final int total;
}

class UpdateCheckResult {
  const UpdateCheckResult({
    required this.available,
    this.version,
    this.releaseUrl,
    this.package,
  });

  const UpdateCheckResult.current()
    : available = false,
      version = null,
      releaseUrl = null,
      package = null;

  final bool available;
  final String? version;
  final String? releaseUrl;
  final UpdatePackage? package;

  factory UpdateCheckResult.fromMap(Map<Object?, Object?> map) {
    final package = map['package'];
    return UpdateCheckResult(
      available: map['available'] as bool? ?? false,
      version: map['version'] as String?,
      releaseUrl: map['release_url'] as String?,
      package: package is Map<Object?, Object?>
          ? UpdatePackage.fromMap(package)
          : null,
    );
  }
}

class UpdatePackage {
  const UpdatePackage({
    required this.name,
    required this.downloadUrl,
    required this.size,
    required this.sha256,
    required this.platform,
    required this.variant,
  });

  final String name;
  final String downloadUrl;
  final int size;
  final String sha256;
  final String platform;
  final String variant;

  factory UpdatePackage.fromMap(Map<Object?, Object?> map) {
    return UpdatePackage(
      name: map['name'] as String? ?? '',
      downloadUrl: map['download_url'] as String? ?? '',
      size: map['size'] as int? ?? 0,
      sha256: map['sha256'] as String? ?? '',
      platform: map['platform'] as String? ?? '',
      variant: map['variant'] as String? ?? '',
    );
  }

  Map<String, Object> toMap() => <String, Object>{
    'name': name,
    'download_url': downloadUrl,
    'size': size,
    'sha256': sha256,
    'platform': platform,
    'variant': variant,
  };
}

enum UpdateOperationPhase {
  idle,
  checking,
  available,
  downloading,
  verifying,
  ready,
  installing,
  failed,
}

class PerAppProxySettings {
  const PerAppProxySettings({
    this.enabled = false,
    this.packageNames = const <String>[],
  });

  static const int maxPackages = 1024;
  static const int maxPackageLength = 256;
  static final RegExp packageNamePattern = RegExp(
    r'^[A-Za-z][A-Za-z0-9_]*(?:\.[A-Za-z][A-Za-z0-9_]*)*$',
  );

  final bool enabled;
  final List<String> packageNames;

  PerAppProxySettings copyWith({bool? enabled, List<String>? packageNames}) {
    return PerAppProxySettings(
      enabled: enabled ?? this.enabled,
      packageNames: packageNames ?? this.packageNames,
    );
  }

  static List<String> sanitizePackages(
    Iterable<String> names, {
    String? selfPackage,
  }) {
    final sanitized = names
        .map((name) => name.trim())
        .where(
          (name) =>
              name.isNotEmpty &&
              name != selfPackage &&
              name.length <= maxPackageLength &&
              packageNamePattern.hasMatch(name),
        )
        .toSet()
        .toList();
    sanitized.sort();
    return List<String>.unmodifiable(sanitized);
  }

  String? validationError({String? selfPackage}) {
    if (packageNames.length > maxPackages) {
      return 'INVALID_ARGUMENT';
    }
    final invalid = packageNames.any((raw) {
      final name = raw.trim();
      return name.isNotEmpty &&
          name != selfPackage &&
          (name.length > maxPackageLength ||
              !packageNamePattern.hasMatch(name));
    });
    if (invalid) {
      return 'INVALID_ARGUMENT';
    }
    final sanitized = sanitizePackages(packageNames, selfPackage: selfPackage);
    if (enabled && sanitized.isEmpty) {
      return 'ANDROID_PER_APP_EMPTY';
    }
    return null;
  }

  factory PerAppProxySettings.fromMap(Map<Object?, Object?> map) {
    final names = (map['package_names'] as List?)?.whereType<String>().toList(
      growable: false,
    );
    return PerAppProxySettings(
      enabled: map['enabled'] as bool? ?? false,
      packageNames: List<String>.unmodifiable(names ?? const <String>[]),
    );
  }

  Map<String, Object?> toMap() => <String, Object?>{
    'enabled': enabled,
    'package_names': packageNames,
  };
}

class InstalledAppInfo {
  const InstalledAppInfo({
    required this.packageName,
    required this.label,
    required this.isSystem,
    required this.hasInternet,
  });

  final String packageName;
  final String label;
  final bool isSystem;
  final bool hasInternet;

  factory InstalledAppInfo.fromMap(Map<Object?, Object?> map) {
    return InstalledAppInfo(
      packageName: map['package_name'] as String? ?? '',
      label: map['label'] as String? ?? '',
      isSystem: map['is_system'] as bool? ?? false,
      hasInternet: map['has_internet'] as bool? ?? false,
    );
  }
}

class PlatformPreferences {
  const PlatformPreferences({
    this.startOnBoot = false,
    this.closeToTray = true,
    this.warpProtocolAssociation = false,
  });

  final bool startOnBoot;
  final bool closeToTray;
  final bool warpProtocolAssociation;

  factory PlatformPreferences.fromMap(Map<Object?, Object?> map) {
    return PlatformPreferences(
      startOnBoot: map['start_on_boot'] as bool? ?? false,
      closeToTray: map['close_to_tray'] as bool? ?? true,
      warpProtocolAssociation:
          map['warp_protocol_association'] as bool? ?? false,
    );
  }
}

class ProfileCatalog {
  const ProfileCatalog({
    required this.profiles,
    required this.activeProfileId,
    this.identityStates = const <String, ProfileIdentityState>{},
    this.identityStatuses = const <String, ProfileIdentityStatus>{},
  });

  final List<UsqueProfile> profiles;
  final String activeProfileId;
  final Map<String, ProfileIdentityState> identityStates;
  final Map<String, ProfileIdentityStatus> identityStatuses;
}

class ProfileIdentityStatus {
  const ProfileIdentityStatus({
    required this.state,
    this.licenseState = LicenseState.unknown,
    this.accountType = '',
    this.cleanupPending = false,
    this.provider = IdentityProvider.consumer,
    this.organization = '',
  });

  final ProfileIdentityState state;
  final LicenseState licenseState;
  final String accountType;
  final bool cleanupPending;
  final IdentityProvider provider;
  final String organization;
}

class FrontendSettings {
  const FrontendSettings({
    required this.tunnel,
    required this.socks5,
    required this.http,
  });

  const FrontendSettings.windowsDefault()
    : tunnel = true,
      socks5 = true,
      http = true;

  const FrontendSettings.androidDefault()
    : tunnel = true,
      socks5 = true,
      http = true;

  final bool tunnel;
  final bool socks5;
  final bool http;

  bool get any => tunnel || socks5 || http;

  FrontendSettings copyWith({bool? tunnel, bool? socks5, bool? http}) {
    return FrontendSettings(
      tunnel: tunnel ?? this.tunnel,
      socks5: socks5 ?? this.socks5,
      http: http ?? this.http,
    );
  }

  factory FrontendSettings.fromMap(Map<String, Object?> map) {
    return FrontendSettings(
      tunnel: _bool(map, 'tunnel'),
      socks5: _bool(map, 'socks5'),
      http: _bool(map, 'http'),
    );
  }

  Map<String, Object?> toMap() => <String, Object?>{
    'tunnel': tunnel,
    'socks5': socks5,
    'http': http,
  };
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
    this.dnsIpv4 = '1.1.1.1',
    this.dnsIpv6 = '2606:4700:4700::1111',
    this.systemProxy = false,
    this.authUsername = '',
  });

  final String socksIpv4;
  final String socksIpv6;
  final int socksPort;
  final String httpIpv4;
  final String httpIpv6;
  final int httpPort;
  final ProxyDnsMode dnsMode;
  final String dnsIpv4;
  final String dnsIpv6;
  final bool systemProxy;
  final String authUsername;

  bool get remoteDns => dnsMode == ProxyDnsMode.remote;

  bool get hasAuth => authUsername.isNotEmpty;

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
    String? dnsIpv4,
    String? dnsIpv6,
    bool? systemProxy,
    String? authUsername,
  }) {
    return ProxySettings(
      socksIpv4: socksIpv4 ?? this.socksIpv4,
      socksIpv6: socksIpv6 ?? this.socksIpv6,
      socksPort: socksPort ?? this.socksPort,
      httpIpv4: httpIpv4 ?? this.httpIpv4,
      httpIpv6: httpIpv6 ?? this.httpIpv6,
      httpPort: httpPort ?? this.httpPort,
      dnsMode: dnsMode ?? this.dnsMode,
      dnsIpv4: dnsIpv4 ?? this.dnsIpv4,
      dnsIpv6: dnsIpv6 ?? this.dnsIpv6,
      systemProxy: systemProxy ?? this.systemProxy,
      authUsername: authUsername ?? this.authUsername,
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
      dnsIpv4: _stringOr(map, 'dns_v4', '1.1.1.1'),
      dnsIpv6: _stringOr(map, 'dns_v6', '2606:4700:4700::1111'),
      systemProxy: _bool(map, 'system_proxy'),
      authUsername: _stringOr(map, 'auth_username', ''),
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
      'dns_v4': dnsIpv4,
      'dns_v6': dnsIpv6,
      'system_proxy': systemProxy,
      if (authUsername.isNotEmpty) 'auth_username': authUsername,
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
    this.geoDirectCountries = const <String>[],
    this.proxy = const ProxySettings(),
    this.frontends = const FrontendSettings.windowsDefault(),
    this.directDns = const DirectDnsSettings(),
  });

  static const defaultEndpointIpv4 = '162.159.198.2';
  static const defaultEndpointIpv6 = '2606:4700:103::2';
  static const defaultEndpointPort = 443;
  static const defaultSni = 'speed.cloudflare.com';
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
  final List<String> geoDirectCountries;
  final ProxySettings proxy;
  final FrontendSettings frontends;
  final DirectDnsSettings directDns;

  factory UsqueProfile.defaultProfile() {
    final android = defaultTargetPlatform == TargetPlatform.android;
    return UsqueProfile(
      id: defaultProfileId,
      name: 'Default',
      frontends: android
          ? const FrontendSettings.androidDefault()
          : const FrontendSettings.windowsDefault(),
      proxy: const ProxySettings(),
      directDns: const DirectDnsSettings(),
    );
  }

  static OperatingMode modeFromFrontends(FrontendSettings frontends) {
    if (frontends.tunnel) {
      return OperatingMode.vpn;
    }
    if (frontends.http && !frontends.socks5) {
      return OperatingMode.httpProxy;
    }
    return OperatingMode.socks5;
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
      directDns: const DirectDnsSettings(),
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
    List<String>? geoDirectCountries,
    ProxySettings? proxy,
    FrontendSettings? frontends,
    DirectDnsSettings? directDns,
  }) {
    final nextFrontends = frontends ?? this.frontends;
    final nextMode = frontends != null
        ? modeFromFrontends(nextFrontends)
        : (mode ?? this.mode);
    return UsqueProfile(
      id: id ?? this.id,
      name: name ?? this.name,
      mode: nextMode,
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
      geoDirectCountries: geoDirectCountries ?? this.geoDirectCountries,
      proxy: proxy ?? this.proxy,
      frontends: nextFrontends,
      directDns: directDns ?? this.directDns,
    );
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'id': id,
      'name': name,
      'mode': modeFromFrontends(frontends).name,
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
      'geo_direct_countries': geoDirectCountries,
      'proxy': proxy.toMap(),
      'frontends': frontends.toMap(),
      'direct_dns': directDns.toMap(),
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
    final geoDirect = map.containsKey('geo_direct_countries')
        ? _stringList(map, 'geo_direct_countries')
        : const <String>[];
    if (geoDirect.length > 32) {
      throw const FormatException('Too many direct countries');
    }
    final proxy = map['proxy'];
    if (proxy is! Map) {
      throw const FormatException('Missing proxy settings');
    }

    final frontends = map['frontends'];
    final directDns = map['direct_dns'];
    final legacyMode = _enumByName(OperatingMode.values, _string(map, 'mode'));
    final migratedFrontends = frontends is Map
        ? FrontendSettings.fromMap(Map<String, Object?>.from(frontends))
        : FrontendSettings(
            tunnel: legacyMode == OperatingMode.vpn,
            socks5: legacyMode == OperatingMode.socks5,
            http: legacyMode == OperatingMode.httpProxy,
          );

    return UsqueProfile(
      id: id,
      name: name,
      mode: modeFromFrontends(migratedFrontends),
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
      geoDirectCountries: List<String>.unmodifiable(geoDirect),
      proxy: ProxySettings.fromMap(Map<String, Object?>.from(proxy)),
      frontends: migratedFrontends,
      directDns: directDns is Map
          ? DirectDnsSettings.fromMap(Map<String, Object?>.from(directDns))
          : const DirectDnsSettings(),
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

String _stringOr(Map<String, Object?> map, String key, String fallback) {
  if (!map.containsKey(key)) {
    return fallback;
  }
  return _string(map, key);
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

Map<Object?, Object?> _objectMap(Object? value) => value is Map
    ? Map<Object?, Object?>.from(value)
    : const <Object?, Object?>{};

int _mapInt(Map<Object?, Object?> map, String key) =>
    (map[key] as num?)?.toInt() ?? 0;

int? _mapNullableInt(Map<Object?, Object?> map, String key) =>
    (map[key] as num?)?.toInt();

String _mapString(Map<Object?, Object?> map, String key) =>
    map[key] as String? ?? '';

T _enumNameOr<T extends Enum>(List<T> values, Object? raw, T fallback) {
  final name = raw as String?;
  if (name == null) return fallback;
  return values.firstWhere(
    (value) => value.name == name,
    orElse: () => fallback,
  );
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

class FrontendRuntimeStatus {
  const FrontendRuntimeStatus({
    required this.kind,
    required this.phase,
    this.listeners = const <String>[],
    this.errorCode,
  });

  final FrontendKind kind;
  final FrontendPhase phase;
  final List<String> listeners;
  final String? errorCode;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is FrontendRuntimeStatus &&
            kind == other.kind &&
            phase == other.phase &&
            listEquals(listeners, other.listeners) &&
            errorCode == other.errorCode;
  }

  @override
  int get hashCode =>
      Object.hash(kind, phase, Object.hashAll(listeners), errorCode);
}

enum MetricAvailability { unknown, available, unsupported, notReady, stale }

enum NetworkQualityLevel {
  unknown,
  good,
  fair,
  poor,
  limitedData,
  disconnected,
}

enum NetworkQueueKind {
  unknown,
  tunToTransport,
  proxyToTransport,
  transportOutgoing,
  h3DatagramSend,
  h3WireSend,
  transportToTun,
  transportToProxy,
  directDns,
}

class NetworkConnectionMetrics {
  const NetworkConnectionMetrics({
    this.smoothedRttMilliseconds,
    this.minimumRttMilliseconds,
    this.rttVarianceMilliseconds,
    this.intervalLossBasisPoints,
    this.congestionWindowBytes,
    this.bytesInFlight,
    this.sendRateBitsPerSecond,
    this.packetsLost = 0,
    this.bytesLost = 0,
    this.tunSinkDropCount = 0,
    this.quicDatagramDropCount = 0,
    this.queueOldestAgeMilliseconds,
    this.currentPmtuBytes,
    this.migrationAttemptCount = 0,
    this.migrationSuccessCount = 0,
    this.migrationFailureCount = 0,
    this.lastMigrationDurationMilliseconds,
    this.udpSendSyscallCount = 0,
    this.udpRecvSyscallCount = 0,
    this.udpDatagramSentCount = 0,
    this.udpDatagramReceivedCount = 0,
    this.packetBufferPoolHitCount = 0,
    this.packetBufferPoolMissCount = 0,
    this.h2FlowControlStallCount = 0,
    this.h2FlowControlStallTotalMilliseconds = 0,
    this.h2FlowControlStallMaxMilliseconds = 0,
    this.h2StreamReceiveWindowBytes = 0,
    this.h2ConnectionReceiveWindowBytes = 0,
    this.directDnsSuccessCount = 0,
    this.directDnsFailureCount = 0,
    this.directDnsTimeoutCount = 0,
    this.directDnsLastRttMilliseconds,
    this.pmtuChangeCount = 0,
    this.pmtuRevalidationFailureCount = 0,
    this.smoothedRttAvailability = MetricAvailability.unknown,
    this.minimumRttAvailability = MetricAvailability.unknown,
    this.rttVarianceAvailability = MetricAvailability.unknown,
    this.intervalLossAvailability = MetricAvailability.unknown,
    this.congestionWindowAvailability = MetricAvailability.unknown,
    this.bytesInFlightAvailability = MetricAvailability.unknown,
    this.sendRateAvailability = MetricAvailability.unknown,
  });

  final int? smoothedRttMilliseconds;
  final int? minimumRttMilliseconds;
  final int? rttVarianceMilliseconds;
  final int? intervalLossBasisPoints;
  final int? congestionWindowBytes;
  final int? bytesInFlight;
  final int? sendRateBitsPerSecond;
  final int packetsLost;
  final int bytesLost;
  final int tunSinkDropCount;
  final int quicDatagramDropCount;
  final int? queueOldestAgeMilliseconds;
  final int? currentPmtuBytes;
  final int migrationAttemptCount;
  final int migrationSuccessCount;
  final int migrationFailureCount;
  final int? lastMigrationDurationMilliseconds;
  final int udpSendSyscallCount;
  final int udpRecvSyscallCount;
  final int udpDatagramSentCount;
  final int udpDatagramReceivedCount;
  final int packetBufferPoolHitCount;
  final int packetBufferPoolMissCount;
  final int h2FlowControlStallCount;
  final int h2FlowControlStallTotalMilliseconds;
  final int h2FlowControlStallMaxMilliseconds;
  final int h2StreamReceiveWindowBytes;
  final int h2ConnectionReceiveWindowBytes;
  final int directDnsSuccessCount;
  final int directDnsFailureCount;
  final int directDnsTimeoutCount;
  final int? directDnsLastRttMilliseconds;
  final int pmtuChangeCount;
  final int pmtuRevalidationFailureCount;
  final MetricAvailability smoothedRttAvailability;
  final MetricAvailability minimumRttAvailability;
  final MetricAvailability rttVarianceAvailability;
  final MetricAvailability intervalLossAvailability;
  final MetricAvailability congestionWindowAvailability;
  final MetricAvailability bytesInFlightAvailability;
  final MetricAvailability sendRateAvailability;

  List<Object?> get _values => <Object?>[
    smoothedRttMilliseconds,
    minimumRttMilliseconds,
    rttVarianceMilliseconds,
    intervalLossBasisPoints,
    congestionWindowBytes,
    bytesInFlight,
    sendRateBitsPerSecond,
    packetsLost,
    bytesLost,
    tunSinkDropCount,
    quicDatagramDropCount,
    queueOldestAgeMilliseconds,
    currentPmtuBytes,
    migrationAttemptCount,
    migrationSuccessCount,
    migrationFailureCount,
    lastMigrationDurationMilliseconds,
    udpSendSyscallCount,
    udpRecvSyscallCount,
    udpDatagramSentCount,
    udpDatagramReceivedCount,
    packetBufferPoolHitCount,
    packetBufferPoolMissCount,
    h2FlowControlStallCount,
    h2FlowControlStallTotalMilliseconds,
    h2FlowControlStallMaxMilliseconds,
    h2StreamReceiveWindowBytes,
    h2ConnectionReceiveWindowBytes,
    directDnsSuccessCount,
    directDnsFailureCount,
    directDnsTimeoutCount,
    directDnsLastRttMilliseconds,
    pmtuChangeCount,
    pmtuRevalidationFailureCount,
    smoothedRttAvailability,
    minimumRttAvailability,
    rttVarianceAvailability,
    intervalLossAvailability,
    congestionWindowAvailability,
    bytesInFlightAvailability,
    sendRateAvailability,
  ];

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is NetworkConnectionMetrics && listEquals(_values, other._values);

  @override
  int get hashCode => Object.hashAll(_values);
}

class NetworkQueueQuality {
  const NetworkQueueQuality({
    this.kind = NetworkQueueKind.unknown,
    this.availability = MetricAvailability.unknown,
    this.currentItems = 0,
    this.capacityItems = 0,
    this.currentBytes = 0,
    this.capacityBytes = 0,
    this.highWaterItems = 0,
    this.highWaterBytes = 0,
    this.dropItems = 0,
    this.dropBytes = 0,
    this.oldestAgeMilliseconds,
    this.enqueueCount = 0,
    this.dequeueCount = 0,
    this.closed = false,
    this.cancelled = false,
  });

  final NetworkQueueKind kind;
  final MetricAvailability availability;
  final int currentItems;
  final int capacityItems;
  final int currentBytes;
  final int capacityBytes;
  final int highWaterItems;
  final int highWaterBytes;
  final int dropItems;
  final int dropBytes;
  final int? oldestAgeMilliseconds;
  final int enqueueCount;
  final int dequeueCount;
  final bool closed;
  final bool cancelled;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is NetworkQueueQuality &&
          kind == other.kind &&
          availability == other.availability &&
          currentItems == other.currentItems &&
          capacityItems == other.capacityItems &&
          currentBytes == other.currentBytes &&
          capacityBytes == other.capacityBytes &&
          highWaterItems == other.highWaterItems &&
          highWaterBytes == other.highWaterBytes &&
          dropItems == other.dropItems &&
          dropBytes == other.dropBytes &&
          oldestAgeMilliseconds == other.oldestAgeMilliseconds &&
          enqueueCount == other.enqueueCount &&
          dequeueCount == other.dequeueCount &&
          closed == other.closed &&
          cancelled == other.cancelled;

  @override
  int get hashCode => Object.hashAll(<Object?>[
    kind,
    availability,
    currentItems,
    capacityItems,
    currentBytes,
    capacityBytes,
    highWaterItems,
    highWaterBytes,
    dropItems,
    dropBytes,
    oldestAgeMilliseconds,
    enqueueCount,
    dequeueCount,
    closed,
    cancelled,
  ]);
}

class PmtuQualityInfo {
  const PmtuQualityInfo({
    this.availability = MetricAvailability.unknown,
    this.outerPmtuBytes,
    this.effectiveConnectIpPayloadBytes,
    this.effectivePayloadAvailability = MetricAvailability.unknown,
    this.phaseCode = '',
    this.changeCount = 0,
    this.revalidationFailureCount = 0,
  });

  final MetricAvailability availability;
  final int? outerPmtuBytes;
  final int? effectiveConnectIpPayloadBytes;
  final MetricAvailability effectivePayloadAvailability;
  final String phaseCode;
  final int changeCount;
  final int revalidationFailureCount;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is PmtuQualityInfo &&
          availability == other.availability &&
          outerPmtuBytes == other.outerPmtuBytes &&
          effectiveConnectIpPayloadBytes ==
              other.effectiveConnectIpPayloadBytes &&
          effectivePayloadAvailability == other.effectivePayloadAvailability &&
          phaseCode == other.phaseCode &&
          changeCount == other.changeCount &&
          revalidationFailureCount == other.revalidationFailureCount;

  @override
  int get hashCode => Object.hash(
    availability,
    outerPmtuBytes,
    effectiveConnectIpPayloadBytes,
    effectivePayloadAvailability,
    phaseCode,
    changeCount,
    revalidationFailureCount,
  );
}

class MigrationQualityInfo {
  const MigrationQualityInfo({
    this.phaseCode = '',
    this.attemptCount = 0,
    this.successCount = 0,
    this.failureCount = 0,
    this.lastDurationMilliseconds,
    this.lastReasonCode = '',
  });

  final String phaseCode;
  final int attemptCount;
  final int successCount;
  final int failureCount;
  final int? lastDurationMilliseconds;
  final String lastReasonCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is MigrationQualityInfo &&
          phaseCode == other.phaseCode &&
          attemptCount == other.attemptCount &&
          successCount == other.successCount &&
          failureCount == other.failureCount &&
          lastDurationMilliseconds == other.lastDurationMilliseconds &&
          lastReasonCode == other.lastReasonCode;

  @override
  int get hashCode => Object.hash(
    phaseCode,
    attemptCount,
    successCount,
    failureCount,
    lastDurationMilliseconds,
    lastReasonCode,
  );
}

class DirectDnsQualityInfo {
  const DirectDnsQualityInfo({
    this.mode = DirectDnsMode.unknown,
    this.phaseCode = '',
    this.successCount = 0,
    this.failureCount = 0,
    this.timeoutCount = 0,
    this.lastRttMilliseconds,
    this.lastReasonCode = '',
  });

  final DirectDnsMode mode;
  final String phaseCode;
  final int successCount;
  final int failureCount;
  final int timeoutCount;
  final int? lastRttMilliseconds;
  final String lastReasonCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is DirectDnsQualityInfo &&
          mode == other.mode &&
          phaseCode == other.phaseCode &&
          successCount == other.successCount &&
          failureCount == other.failureCount &&
          timeoutCount == other.timeoutCount &&
          lastRttMilliseconds == other.lastRttMilliseconds &&
          lastReasonCode == other.lastReasonCode;

  @override
  int get hashCode => Object.hash(
    mode,
    phaseCode,
    successCount,
    failureCount,
    timeoutCount,
    lastRttMilliseconds,
    lastReasonCode,
  );
}

class NetworkQualitySnapshot {
  const NetworkQualitySnapshot({
    this.sampledAt,
    this.connectionInstanceId,
    this.level = NetworkQualityLevel.unknown,
    this.metrics = const NetworkConnectionMetrics(),
    this.queues = const <NetworkQueueQuality>[],
    this.pmtu = const PmtuQualityInfo(),
    this.migration = const MigrationQualityInfo(),
    this.directDns = const DirectDnsQualityInfo(),
  });

  final DateTime? sampledAt;
  final String? connectionInstanceId;
  final NetworkQualityLevel level;
  final NetworkConnectionMetrics metrics;
  final List<NetworkQueueQuality> queues;
  final PmtuQualityInfo pmtu;
  final MigrationQualityInfo migration;
  final DirectDnsQualityInfo directDns;

  factory NetworkQualitySnapshot.fromMap(Map<Object?, Object?> map) {
    final metricsMap = _objectMap(map['metrics']);
    final pmtuMap = _objectMap(map['pmtu']);
    final migrationMap = _objectMap(map['migration']);
    final directDnsMap = _objectMap(map['direct_dns']);
    final sampledAtMilliseconds = _mapInt(map, 'sampled_at_unix_ms');
    final connectionId = _mapString(map, 'connection_instance_id');
    return NetworkQualitySnapshot(
      sampledAt: sampledAtMilliseconds == 0
          ? null
          : DateTime.fromMillisecondsSinceEpoch(
              sampledAtMilliseconds,
              isUtc: true,
            ),
      connectionInstanceId: connectionId.isEmpty ? null : connectionId,
      level: _enumNameOr(
        NetworkQualityLevel.values,
        map['level'],
        NetworkQualityLevel.unknown,
      ),
      metrics: NetworkConnectionMetrics(
        smoothedRttMilliseconds: _mapNullableInt(
          metricsMap,
          'smoothed_rtt_milliseconds',
        ),
        minimumRttMilliseconds: _mapNullableInt(
          metricsMap,
          'minimum_rtt_milliseconds',
        ),
        rttVarianceMilliseconds: _mapNullableInt(
          metricsMap,
          'rtt_variance_milliseconds',
        ),
        intervalLossBasisPoints: _mapNullableInt(
          metricsMap,
          'interval_loss_basis_points',
        ),
        congestionWindowBytes: _mapNullableInt(
          metricsMap,
          'congestion_window_bytes',
        ),
        bytesInFlight: _mapNullableInt(metricsMap, 'bytes_in_flight'),
        sendRateBitsPerSecond: _mapNullableInt(
          metricsMap,
          'send_rate_bits_per_second',
        ),
        packetsLost: _mapInt(metricsMap, 'packets_lost'),
        bytesLost: _mapInt(metricsMap, 'bytes_lost'),
        tunSinkDropCount: _mapInt(metricsMap, 'tun_sink_drop_count'),
        quicDatagramDropCount: _mapInt(metricsMap, 'quic_datagram_drop_count'),
        queueOldestAgeMilliseconds: _mapNullableInt(
          metricsMap,
          'queue_oldest_age_milliseconds',
        ),
        currentPmtuBytes: _mapNullableInt(metricsMap, 'current_pmtu_bytes'),
        migrationAttemptCount: _mapInt(metricsMap, 'migration_attempt_count'),
        migrationSuccessCount: _mapInt(metricsMap, 'migration_success_count'),
        migrationFailureCount: _mapInt(metricsMap, 'migration_failure_count'),
        lastMigrationDurationMilliseconds: _mapNullableInt(
          metricsMap,
          'last_migration_duration_milliseconds',
        ),
        udpSendSyscallCount: _mapInt(metricsMap, 'udp_send_syscall_count'),
        udpRecvSyscallCount: _mapInt(metricsMap, 'udp_recv_syscall_count'),
        udpDatagramSentCount: _mapInt(metricsMap, 'udp_datagram_sent_count'),
        udpDatagramReceivedCount: _mapInt(
          metricsMap,
          'udp_datagram_received_count',
        ),
        packetBufferPoolHitCount: _mapInt(
          metricsMap,
          'packet_buffer_pool_hit_count',
        ),
        packetBufferPoolMissCount: _mapInt(
          metricsMap,
          'packet_buffer_pool_miss_count',
        ),
        h2FlowControlStallCount: _mapInt(
          metricsMap,
          'h2_flow_control_stall_count',
        ),
        h2FlowControlStallTotalMilliseconds: _mapInt(
          metricsMap,
          'h2_flow_control_stall_total_milliseconds',
        ),
        h2FlowControlStallMaxMilliseconds: _mapInt(
          metricsMap,
          'h2_flow_control_stall_max_milliseconds',
        ),
        h2StreamReceiveWindowBytes: _mapInt(
          metricsMap,
          'h2_stream_receive_window_bytes',
        ),
        h2ConnectionReceiveWindowBytes: _mapInt(
          metricsMap,
          'h2_connection_receive_window_bytes',
        ),
        directDnsSuccessCount: _mapInt(metricsMap, 'direct_dns_success_count'),
        directDnsFailureCount: _mapInt(metricsMap, 'direct_dns_failure_count'),
        directDnsTimeoutCount: _mapInt(metricsMap, 'direct_dns_timeout_count'),
        directDnsLastRttMilliseconds: _mapNullableInt(
          metricsMap,
          'direct_dns_last_rtt_milliseconds',
        ),
        pmtuChangeCount: _mapInt(metricsMap, 'pmtu_change_count'),
        pmtuRevalidationFailureCount: _mapInt(
          metricsMap,
          'pmtu_revalidation_failure_count',
        ),
        smoothedRttAvailability: _enumNameOr(
          MetricAvailability.values,
          metricsMap['smoothed_rtt_availability'],
          MetricAvailability.unknown,
        ),
        minimumRttAvailability: _enumNameOr(
          MetricAvailability.values,
          metricsMap['minimum_rtt_availability'],
          MetricAvailability.unknown,
        ),
        rttVarianceAvailability: _enumNameOr(
          MetricAvailability.values,
          metricsMap['rtt_variance_availability'],
          MetricAvailability.unknown,
        ),
        intervalLossAvailability: _enumNameOr(
          MetricAvailability.values,
          metricsMap['interval_loss_availability'],
          MetricAvailability.unknown,
        ),
        congestionWindowAvailability: _enumNameOr(
          MetricAvailability.values,
          metricsMap['congestion_window_availability'],
          MetricAvailability.unknown,
        ),
        bytesInFlightAvailability: _enumNameOr(
          MetricAvailability.values,
          metricsMap['bytes_in_flight_availability'],
          MetricAvailability.unknown,
        ),
        sendRateAvailability: _enumNameOr(
          MetricAvailability.values,
          metricsMap['send_rate_availability'],
          MetricAvailability.unknown,
        ),
      ),
      queues: List<NetworkQueueQuality>.unmodifiable(
        (map['queues'] as List? ?? const <Object?>[])
            .whereType<Map<Object?, Object?>>()
            .map((raw) {
              final queue = Map<Object?, Object?>.from(raw);
              return NetworkQueueQuality(
                kind: _enumNameOr(
                  NetworkQueueKind.values,
                  queue['kind'],
                  NetworkQueueKind.unknown,
                ),
                availability: _enumNameOr(
                  MetricAvailability.values,
                  queue['availability'],
                  MetricAvailability.unknown,
                ),
                currentItems: _mapInt(queue, 'current_items'),
                capacityItems: _mapInt(queue, 'capacity_items'),
                currentBytes: _mapInt(queue, 'current_bytes'),
                capacityBytes: _mapInt(queue, 'capacity_bytes'),
                highWaterItems: _mapInt(queue, 'high_water_items'),
                highWaterBytes: _mapInt(queue, 'high_water_bytes'),
                dropItems: _mapInt(queue, 'drop_items'),
                dropBytes: _mapInt(queue, 'drop_bytes'),
                oldestAgeMilliseconds: _mapNullableInt(
                  queue,
                  'oldest_age_milliseconds',
                ),
                enqueueCount: _mapInt(queue, 'enqueue_count'),
                dequeueCount: _mapInt(queue, 'dequeue_count'),
                closed: queue['closed'] as bool? ?? false,
                cancelled: queue['cancelled'] as bool? ?? false,
              );
            }),
      ),
      pmtu: PmtuQualityInfo(
        availability: _enumNameOr(
          MetricAvailability.values,
          pmtuMap['availability'],
          MetricAvailability.unknown,
        ),
        outerPmtuBytes: _mapNullableInt(pmtuMap, 'outer_pmtu_bytes'),
        effectiveConnectIpPayloadBytes: _mapNullableInt(
          pmtuMap,
          'effective_connect_ip_payload_bytes',
        ),
        effectivePayloadAvailability: _enumNameOr(
          MetricAvailability.values,
          pmtuMap['effective_payload_availability'],
          MetricAvailability.unknown,
        ),
        phaseCode: _mapString(pmtuMap, 'phase_code'),
        changeCount: _mapInt(pmtuMap, 'change_count'),
        revalidationFailureCount: _mapInt(
          pmtuMap,
          'revalidation_failure_count',
        ),
      ),
      migration: MigrationQualityInfo(
        phaseCode: _mapString(migrationMap, 'phase_code'),
        attemptCount: _mapInt(migrationMap, 'attempt_count'),
        successCount: _mapInt(migrationMap, 'success_count'),
        failureCount: _mapInt(migrationMap, 'failure_count'),
        lastDurationMilliseconds: _mapNullableInt(
          migrationMap,
          'last_duration_milliseconds',
        ),
        lastReasonCode: _mapString(migrationMap, 'last_reason_code'),
      ),
      directDns: DirectDnsQualityInfo(
        mode: _enumNameOr(
          DirectDnsMode.values,
          directDnsMap['mode'],
          DirectDnsMode.unknown,
        ),
        phaseCode: _mapString(directDnsMap, 'phase_code'),
        successCount: _mapInt(directDnsMap, 'success_count'),
        failureCount: _mapInt(directDnsMap, 'failure_count'),
        timeoutCount: _mapInt(directDnsMap, 'timeout_count'),
        lastRttMilliseconds: _mapNullableInt(
          directDnsMap,
          'last_rtt_milliseconds',
        ),
        lastReasonCode: _mapString(directDnsMap, 'last_reason_code'),
      ),
    );
  }

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is NetworkQualitySnapshot &&
          connectionInstanceId == other.connectionInstanceId &&
          level == other.level &&
          metrics == other.metrics &&
          listEquals(queues, other.queues) &&
          pmtu == other.pmtu &&
          migration == other.migration &&
          directDns == other.directDns;

  @override
  int get hashCode => Object.hash(
    connectionInstanceId,
    level,
    metrics,
    Object.hashAll(queues),
    pmtu,
    migration,
    directDns,
  );
}

class EngineCapabilities {
  const EngineCapabilities({
    this.networkQuality = false,
    this.encryptedDirectDns = false,
    this.quicMigration = false,
    this.automaticPmtu = false,
  });

  final bool networkQuality;
  final bool encryptedDirectDns;
  final bool quicMigration;
  final bool automaticPmtu;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is EngineCapabilities &&
          networkQuality == other.networkQuality &&
          encryptedDirectDns == other.encryptedDirectDns &&
          quicMigration == other.quicMigration &&
          automaticPmtu == other.automaticPmtu;

  @override
  int get hashCode => Object.hash(
    networkQuality,
    encryptedDirectDns,
    quicMigration,
    automaticPmtu,
  );
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
    this.exit = const ExitInfo(),
    this.warning,
    this.errorCode,
    this.failure,
    this.frontends = const <FrontendRuntimeStatus>[],
    this.networkQuality,
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
  final ExitInfo exit;
  final String? warning;
  final String? errorCode;
  final TransportFailureInfo? failure;
  final List<FrontendRuntimeStatus> frontends;
  final NetworkQualitySnapshot? networkQuality;

  bool get isConnected =>
      phase == ConnectionPhase.connected || phase == ConnectionPhase.degraded;

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
      failure: map['failure'] is Map
          ? TransportFailureInfo.fromMap(
              Map<Object?, Object?>.from(map['failure'] as Map),
            )
          : null,
      frontends:
          (map['frontends'] as List?)
              ?.whereType<Map<Object?, Object?>>()
              .map((value) {
                final kind = FrontendKind.values.firstWhere(
                  (item) => item.name == value['kind'],
                  orElse: () => FrontendKind.tunnel,
                );
                final phase = FrontendPhase.values.firstWhere(
                  (item) => item.name == value['phase'],
                  orElse: () => FrontendPhase.error,
                );
                return FrontendRuntimeStatus(
                  kind: kind,
                  phase: phase,
                  listeners:
                      (value['listeners'] as List?)?.whereType<String>().toList(
                        growable: false,
                      ) ??
                      const <String>[],
                  errorCode: value['error_code'] as String?,
                );
              })
              .toList(growable: false) ??
          const <FrontendRuntimeStatus>[],
      networkQuality: map['network_quality'] is Map
          ? NetworkQualitySnapshot.fromMap(
              Map<Object?, Object?>.from(map['network_quality'] as Map),
            )
          : null,
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
            exit == other.exit &&
            warning == other.warning &&
            errorCode == other.errorCode &&
            failure == other.failure &&
            listEquals(frontends, other.frontends) &&
            networkQuality == other.networkQuality;
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
    exit,
    warning,
    errorCode,
    failure,
    Object.hashAll(frontends),
    networkQuality,
  ]);
}
