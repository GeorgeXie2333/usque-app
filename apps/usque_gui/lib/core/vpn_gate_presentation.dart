import '../models/app_models.dart';

/// Presents the live chain separately from the editable, device-wide settings.
class VpnGatePresentation {
  VpnGatePresentation(
    EngineSnapshot snapshot, {
    required bool configuredEnabled,
  }) {
    final gate = snapshot.vpnGate;
    connected = snapshot.isConnected && gate.connected;
    busy = snapshot.isTransitional;
    failed =
        snapshot.phase != ConnectionPhase.disconnecting &&
        (gate.stage == 'error' ||
            snapshot.phase == ConnectionPhase.error && configuredEnabled);
    statusKey = switch (snapshot.phase) {
      ConnectionPhase.disconnecting => 'disconnecting',
      _ when failed => 'gate_failed',
      _ when connected => 'connected',
      _ when busy && (configuredEnabled || gate.stage != 'disabled') =>
        switch (gate.stage) {
          'connecting_server' => 'gate_connecting_server',
          'negotiating' => 'gate_negotiating',
          'configuring_network' || 'connected' => 'gate_configuring_network',
          'reconnecting' => 'reconnecting',
          _ => 'gate_connecting_warp',
        },
      _ when snapshot.isConnected => 'gate_warp_only',
      _ when !configuredEnabled => 'gate_disabled',
      _ => 'gate_no_connection',
    };
    server = connected || busy || failed ? gate.server : null;
    serverLabelKey = connected ? 'gate_current' : 'gate_target';
    warpKey = switch (gate.warpStage) {
      'connected' => 'connected',
      'connecting' => 'connecting',
      'reconnecting' => 'reconnecting',
      'disconnected' => 'disconnected',
      'error' => 'error',
      _ => null,
    };
    if (!connected && !busy && !failed) warpKey = null;
  }

  late final bool connected, busy, failed;
  late final String statusKey, serverLabelKey;
  late final VpnGateServer? server;
  String? warpKey;
}
