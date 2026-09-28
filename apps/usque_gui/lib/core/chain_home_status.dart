import '../models/app_models.dart';
import '../widgets/common.dart';
import 'app_strings.dart';
import 'chain_strings.dart';
import 'connection_presentation.dart';

/// Home headline, ring, and chain row for an enabled chain.
///
/// Desktop publishes `connectingH3` before the handshake and Android stays on
/// `preparing` until that handshake returns. The chain stage is the status
/// both platforms already share, so the home surface reads it instead of the
/// platform connection phase.
class ChainHomeStatus {
  const ChainHomeStatus({
    required this.drivesHome,
    required this.showChainRow,
    required this.labelKey,
    required this.chainCatalog,
    required this.mode,
    required this.tone,
  });

  /// The headline and ring follow [labelKey]. Chain-off keeps the connection
  /// phase, as does an idle enabled chain or a connected or failed session
  /// whose chain payload is gone.
  final bool drivesHome;

  /// The chain row is omitted when the chain is off and idle, and when a live
  /// session has no chain payload (so it cannot say "disconnected").
  final bool showChainRow;
  final String labelKey;

  /// Chain-catalog keys collide with the main catalog, so they are not read
  /// through `AppStrings.get`.
  final bool chainCatalog;
  final RingMode mode;
  final StatusTone tone;

  String label(AppStrings strings) =>
      chainCatalog ? strings.chain(labelKey) : strings.get(labelKey);

  static const off = ChainHomeStatus(
    drivesHome: false,
    showChainRow: false,
    labelKey: 'disconnected',
    chainCatalog: false,
    mode: RingMode.idle,
    tone: StatusTone.neutral,
  );

  /// OpenVPN, WireGuard, and VPN Gate share one stage. Desktop clears the
  /// legacy gate stage while a profile is current, so a live chain stage wins.
  /// The gate stage fills in only while the chain stage is still `disabled`.
  static String stage({
    required String chainStage,
    required String gateStage,
  }) => chainStage != 'disabled' ? chainStage : gateStage;

  static ChainHomeStatus of({
    required ConnectionPhase phase,
    required bool chainEnabled,
    required String chainStage,
    required String gateStage,
    required bool hasCurrentProfile,
    required bool hasGateServer,
  }) {
    final resolved = stage(chainStage: chainStage, gateStage: gateStage);
    final hasSession =
        hasCurrentProfile || hasGateServer || resolved != 'disabled';
    // A dropped chain payload must not turn an up or failed tunnel into
    // "disconnected". The headline keeps the connection phase.
    final payloadMissing =
        chainEnabled &&
        resolved == 'disabled' &&
        (phase == ConnectionPhase.connected ||
            phase == ConnectionPhase.degraded ||
            phase == ConnectionPhase.error);
    if ((!chainEnabled && resolved == 'disabled') || payloadMissing) {
      return off;
    }
    final status = _status(
      phase: phase,
      chainEnabled: chainEnabled,
      stage: resolved,
      hasSession: hasSession,
    );
    return ChainHomeStatus(
      drivesHome:
          chainEnabled &&
          !(phase == ConnectionPhase.disconnected && resolved == 'disabled'),
      showChainRow: true,
      labelKey: status.labelKey,
      chainCatalog: status.chainCatalog,
      mode: status.mode,
      tone: status.tone,
    );
  }

  ConnectionPresentation presentation(ConnectionPresentation connection) =>
      ConnectionPresentation(
        mode: mode,
        tone: tone,
        labelKey: labelKey,
        actionKey: connection.actionKey,
        recoverable: connection.recoverable,
      );
}

class _ChainVisual {
  const _ChainVisual(
    this.labelKey,
    this.mode,
    this.tone, {
    this.chainCatalog = false,
  });
  final String labelKey;
  final RingMode mode;
  final StatusTone tone;
  final bool chainCatalog;
}

_ChainVisual _status({
  required ConnectionPhase phase,
  required bool chainEnabled,
  required String stage,
  required bool hasSession,
}) {
  if (phase == ConnectionPhase.disconnecting && (hasSession || chainEnabled)) {
    return const _ChainVisual(
      'disconnecting',
      RingMode.scan,
      StatusTone.neutral,
      chainCatalog: true,
    );
  }
  if (stage == 'error' || (phase == ConnectionPhase.error && chainEnabled)) {
    return const _ChainVisual(
      'error',
      RingMode.fault,
      StatusTone.danger,
      chainCatalog: true,
    );
  }
  // Every exit uses the same chain sentences. Stage names stay internal.
  const progressing = <String>{
    'connecting_warp',
    'connecting_server',
    'negotiating',
    'configuring_network',
    'reconnecting',
  };
  if (progressing.contains(stage)) {
    return const _ChainVisual(
      'connecting',
      RingMode.scan,
      StatusTone.brand,
      chainCatalog: true,
    );
  }
  if (stage == 'connected') {
    // A degraded tunnel keeps that wording. Any other phase, including one
    // still reporting "preparing", follows the chain's connected stage.
    if (phase == ConnectionPhase.degraded) {
      return const _ChainVisual(
        'degraded',
        RingMode.steady,
        StatusTone.warning,
      );
    }
    return const _ChainVisual(
      'connected',
      RingMode.steady,
      StatusTone.success,
      chainCatalog: true,
    );
  }
  if (chainEnabled &&
      stage == 'disabled' &&
      (phase == ConnectionPhase.preparing ||
          phase == ConnectionPhase.connectingH3 ||
          phase == ConnectionPhase.connectingH2 ||
          phase == ConnectionPhase.reconnecting)) {
    return const _ChainVisual(
      'connecting',
      RingMode.scan,
      StatusTone.brand,
      chainCatalog: true,
    );
  }
  if (chainEnabled && stage == 'disabled') {
    return const _ChainVisual(
      'enabled_idle',
      RingMode.idle,
      StatusTone.neutral,
      chainCatalog: true,
    );
  }
  return const _ChainVisual('disconnected', RingMode.idle, StatusTone.neutral);
}
