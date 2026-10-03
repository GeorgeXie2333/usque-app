import 'dart:async';

import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:shared_preferences_platform_interface/shared_preferences_platform_interface.dart';

import '../app.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../services/engine_client.dart';
import 'preview_engine.dart';
import 'preview_update_downloader.dart';

/// A separate development shell; production entry points do not import it.
class PreviewApp extends StatefulWidget {
  const PreviewApp({super.key});

  @override
  State<PreviewApp> createState() => _PreviewAppState();
}

class _PreviewAppState extends State<PreviewApp> {
  late PreviewEngine _engine;
  int _generation = 0;
  ConnectionPhase _phase = ConnectionPhase.disconnected;
  StreamSubscription<EngineSnapshotEvent>? _subscription;
  bool _ready = false;
  bool _resetting = false;
  bool _hasEngine = false;

  @override
  void initState() {
    super.initState();
    SharedPreferencesStorePlatform.instance =
        InMemorySharedPreferencesStore.empty();
    unawaited(_reset(onboarding: false));
  }

  Future<void> _reset({required bool onboarding}) async {
    if (_resetting) return;
    _resetting = true;
    setState(() => _ready = false);
    // Dispose the old controller before resetting the shared in-memory store.
    await WidgetsBinding.instance.endOfFrame;
    unawaited(_subscription?.cancel());
    if (_hasEngine) _engine.dispose();
    if (!mounted) return;
    // Preferences and account/network changes exist only in this process.
    // Never open the host's shared_preferences store or a real engine.
    final preferences = await SharedPreferences.getInstance();
    await preferences.reload();
    await preferences.clear();
    await preferences.setBool('onboarding_complete', !onboarding);
    await preferences.setBool('update_checks_enabled', false);
    if (!mounted) return;
    _engine = PreviewEngine(identityReady: !onboarding);
    _hasEngine = true;
    _generation++;
    _phase = ConnectionPhase.disconnected;
    _subscription = _engine.snapshotEvents.listen((event) {
      if (!mounted || event.snapshot == null) return;
      setState(() => _phase = event.snapshot!.phase);
    });
    setState(() {
      _ready = true;
      _resetting = false;
    });
  }

  @override
  void dispose() {
    unawaited(_subscription?.cancel());
    if (_hasEngine) _engine.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => MaterialApp(
    title: 'Usque UI preview',
    debugShowCheckedModeBanner: false,
    theme: UsqueTheme.light(),
    home: Scaffold(
      body: SafeArea(
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.all(12),
              child: Wrap(
                spacing: 16,
                runSpacing: 8,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  const Text('UI preview · simulated data · no VPN'),
                  SizedBox(
                    width: 280,
                    child: DropdownButton<ConnectionPhase>(
                      isExpanded: true,
                      value: _phase,
                      hint: const Text('Connection state'),
                      items: const [
                        DropdownMenuItem(
                          value: ConnectionPhase.disconnected,
                          child: Text('Disconnected'),
                        ),
                        DropdownMenuItem(
                          value: ConnectionPhase.connectingH3,
                          child: Text('Connecting'),
                        ),
                        DropdownMenuItem(
                          value: ConnectionPhase.connected,
                          child: Text('Connected'),
                        ),
                        DropdownMenuItem(
                          value: ConnectionPhase.reconnecting,
                          child: Text('Reconnecting'),
                        ),
                        DropdownMenuItem(
                          value: ConnectionPhase.error,
                          child: Text('Error'),
                        ),
                      ],
                      onChanged: !_ready
                          ? null
                          : (phase) {
                              if (phase == null) return;
                              setState(() => _phase = phase);
                              _engine.showPhase(phase);
                            },
                    ),
                  ),
                  TextButton(
                    onPressed: _resetting
                        ? null
                        : () => _reset(onboarding: false),
                    child: const Text('Reset preview'),
                  ),
                  TextButton(
                    onPressed: _resetting
                        ? null
                        : () => _reset(onboarding: true),
                    child: const Text('Restart onboarding'),
                  ),
                ],
              ),
            ),
            const Divider(height: 1),
            Expanded(
              child: !_ready
                  ? const Center(child: CircularProgressIndicator())
                  : UsqueBootstrap(
                      key: ValueKey(_generation),
                      engine: _engine,
                      updateDownloader: PreviewUpdateDownloader(_engine),
                    ),
            ),
          ],
        ),
      ),
    ),
  );
}
