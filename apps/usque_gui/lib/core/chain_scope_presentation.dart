import '../models/app_models.dart';
import 'app_strings.dart';
import 'chain_strings.dart';

/// Factual limits of the observed session, never a protection verdict.
/// Saved selections and editor drafts deliberately are not inputs.
class ChainScopePresentation {
  const ChainScopePresentation._({
    this.source,
    this.scopeKey,
    this.androidSettings = false,
  });

  static const empty = ChainScopePresentation._();
  final ChainSource? source;
  final String? scopeKey;
  final bool androidSettings;
  bool get isEmpty => scopeKey == null && !androidSettings;

  String message(AppStrings strings) => [
    if (scopeKey case final key?) strings.chain(key),
    if (androidSettings) strings.chain('scope_android_settings'),
  ].join(' ');

  static ChainScopePresentation of(
    EngineSnapshot snapshot,
    UsqueProfile? appliedProfile, {
    required bool isAndroid,
  }) {
    if (snapshot.phase == ConnectionPhase.disconnected ||
        snapshot.phase == ConnectionPhase.disconnecting) {
      return empty;
    }
    final current = snapshot.chainExit.currentProfile;
    final source =
        current?.source ??
        (appliedProfile != null &&
                appliedProfile.chainEnabled &&
                (snapshot.phase == ConnectionPhase.error ||
                    snapshot.chainExit.stage != 'disabled')
            ? appliedProfile.chainSource
            : null);
    if (source?.isProxy != true) return empty;
    final tunnel = snapshot.frontends
        .where((frontend) => frontend.kind == FrontendKind.tunnel)
        .firstOrNull;
    final liveTunnel = switch (tunnel?.phase) {
      FrontendPhase.preparing ||
      FrontendPhase.active ||
      FrontendPhase.degraded ||
      FrontendPhase.reconnecting => true,
      _ => false,
    };
    final liveProxy = snapshot.frontends.any(
      (frontend) =>
          (frontend.kind == FrontendKind.http ||
              frontend.kind == FrontendKind.socks5) &&
          (frontend.phase == FrontendPhase.active ||
              frontend.phase == FrontendPhase.degraded),
    );
    final failed = snapshot.phase == ConnectionPhase.error;
    // Android omits structured outputs once JNI stops. Its native contract
    // reports Active only with an owned blocking TUN and Inactive only in VPN
    // mode. Use those explicit terminal values with the current proxy source;
    // an absent/unsupported value never establishes VPN scope.
    final androidTerminalVpn =
        isAndroid &&
        failed &&
        current?.source.isProxy == true &&
        (snapshot.killSwitchState == 'active' ||
            snapshot.killSwitchState == 'inactive');
    // Desktop preserves a failed tunnel frontend after retiring its runtime
    // and clearing appliedProfile. Error identifies that prior VPN scope;
    // Disabled or missing metadata does not.
    final knownVpn =
        liveTunnel ||
        (failed &&
            (tunnel?.phase == FrontendPhase.error ||
                appliedProfile?.frontends.tunnel == true)) ||
        androidTerminalVpn;
    final proxyOnly =
        liveProxy &&
        (tunnel?.phase == FrontendPhase.disabled ||
            (tunnel == null && appliedProfile?.frontends.tunnel == false));
    final failedProxyFrontend = snapshot.frontends.any(
      (frontend) =>
          (frontend.kind == FrontendKind.http ||
              frontend.kind == FrontendKind.socks5) &&
          frontend.phase == FrontendPhase.error,
    );
    // Failure does not broaden a proxy-only session's coverage. Keep that
    // limitation when both its non-VPN scope and proxy identity are known.
    // Missing tunnel metadata alone is never evidence that VPN was disabled.
    final failedProxyOnly =
        failed &&
        !knownVpn &&
        (tunnel?.phase == FrontendPhase.disabled ||
            appliedProfile?.frontends.tunnel == false) &&
        (current?.source.isProxy == true || failedProxyFrontend);
    final scopeKey = failed
        ? knownVpn && snapshot.killSwitchState == 'inactive'
              ? 'scope_interrupted'
              : failedProxyOnly
              ? 'scope_proxy_only'
              : null
        : liveTunnel
        ? 'scope_bypass'
        : proxyOnly
        ? 'scope_proxy_only'
        : null;
    return ChainScopePresentation._(
      source: source,
      scopeKey: scopeKey,
      // False also covers an unreported Lockdown state. This is conditional
      // setup advice and does not assert that system blocking is disabled.
      androidSettings: isAndroid && knownVpn && !snapshot.platformLockdown,
    );
  }

  @override
  bool operator ==(Object other) =>
      other is ChainScopePresentation &&
      source == other.source &&
      scopeKey == other.scopeKey &&
      androidSettings == other.androidSettings;

  @override
  int get hashCode => Object.hash(source, scopeKey, androidSettings);
}
