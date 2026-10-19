import 'dart:async';

import 'package:flutter/material.dart';

import '../core/chain_exit_presentation.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import 'controller_selector.dart';
import 'country_flag.dart';

typedef _SummaryInputs = ({
  bool active,
  bool supported,
  int libraryRevision,
  String catalogId,
  ChainExitPresentation view,
});

/// Builds a chain proxy summary and, while [section] is visible, resolves the
/// saved choice once per selection or library change: the VPN Gate node from
/// the local catalogue, or the configuration name from the local library.
/// Neither read refreshes, probes or touches the network.
class ChainExitSummaryBuilder extends StatelessWidget {
  const ChainExitSummaryBuilder({
    required this.controller,
    required this.section,
    required this.builder,
    super.key,
  });

  final AppController controller;
  final AppSection section;
  final Widget Function(BuildContext context, ChainExitPresentation view)
  builder;

  @override
  Widget build(BuildContext context) => ControllerSelector<_SummaryInputs>(
    controller: controller,
    // Throughput counters are not selected, so statistics never rebuild this.
    selector: (app) {
      final profile = app.activeProfile;
      final capabilities = app.engineCapabilities;
      return (
        active: app.section == section,
        supported: profile.chainSource == ChainSource.vpnGate
            ? capabilities?.vpnGateTcp ?? false
            : capabilities?.chainProfileImport ?? false,
        libraryRevision: profile.chainSource == ChainSource.vpnGate
            ? 0
            : app.chainLibraryRevision,
        catalogId: app.strings.catalogId,
        view: ChainExitPresentation.of(
          profile: profile,
          snapshot: app.snapshot,
        ),
      );
    },
    builder: (context, inputs) => _SavedSelection(
      controller: controller,
      inputs: inputs,
      builder: builder,
    ),
  );
}

class _SavedSelection extends StatefulWidget {
  const _SavedSelection({
    required this.controller,
    required this.inputs,
    required this.builder,
  });

  final AppController controller;
  final _SummaryInputs inputs;
  final Widget Function(BuildContext context, ChainExitPresentation view)
  builder;

  @override
  State<_SavedSelection> createState() => _SavedSelectionState();
}

class _SavedSelectionState extends State<_SavedSelection> {
  String? _resolvedKey;
  ChainSelection? _resolved;
  String? _attemptedKey;
  String? _invalidatedLiveKey;
  int _generation = 0;

  @override
  void initState() {
    super.initState();
    _maybeLoad();
  }

  @override
  void didUpdateWidget(covariant _SavedSelection oldWidget) {
    super.didUpdateWidget(oldWidget);
    final controllerChanged = oldWidget.controller != widget.controller;
    final libraryChanged =
        oldWidget.inputs.libraryRevision != widget.inputs.libraryRevision;
    if (controllerChanged || libraryChanged) {
      // Reject a read started before the mutation, even if its reply arrives last.
      _generation++;
      _resolvedKey = null;
      _resolved = null;
      _attemptedKey = null;
      _invalidatedLiveKey = controllerChanged
          ? null
          : ChainExitPresentation.savedSelectionKey(
              widget.controller.activeProfile,
            );
    }
    // Returning to the page retries a lookup that found nothing before.
    if (!widget.inputs.active) _attemptedKey = null;
    _maybeLoad();
  }

  void _maybeLoad() {
    final inputs = widget.inputs;
    if (inputs.view.live) {
      _rememberLive(inputs.view);
      return;
    }
    final key = inputs.view.savedKey;
    if (!inputs.active ||
        !inputs.supported ||
        key == null ||
        key == _resolvedKey ||
        key == _attemptedKey) {
      return;
    }
    _attemptedKey = key;
    unawaited(_load(++_generation, key));
  }

  /// A live exit that is also the saved one stays named after disconnect
  /// without reading the catalogue or library.
  void _rememberLive(ChainExitPresentation view) {
    final profile = widget.controller.activeProfile;
    final key = ChainExitPresentation.savedSelectionKey(profile);
    final selection = view.selection;
    if (key == null || selection == null) return;
    // Session metadata can still contain the name from before a library edit.
    // Keep displaying it live, but resolve the saved name after disconnect.
    if (selection.server == null && key == _invalidatedLiveKey) return;
    final saved = switch (selection.server) {
      final server? => server.matches(profile.vpnGate),
      null =>
        selection.id != null && selection.id == profile.chainExit?.profileId,
    };
    if (saved && view.source == profile.chainSource) {
      _resolvedKey = key;
      _resolved = selection;
    }
  }

  Future<void> _load(int generation, String key) async {
    final app = widget.controller;
    final profile = app.activeProfile;
    if (ChainExitPresentation.savedSelectionKey(profile) != key) return;
    ChainSelection? found;
    try {
      if (profile.chainSource == ChainSource.vpnGate) {
        // statusOnly omits saved_server, so request the smallest page.
        final directory = await app.listVpnGate(limit: 1);
        final server = directory.savedServer;
        if (server != null && server.matches(profile.vpnGate)) {
          found = ChainSelection(server: server);
        }
      } else {
        final id = profile.chainExit?.profileId;
        final library = await app.chainProfile({'action': 'list'});
        for (final candidate in library.profiles) {
          if (candidate.id == id) {
            found = ChainSelection(name: candidate.name);
            break;
          }
        }
      }
    } catch (_) {
      // A missing catalogue or library keeps the summary readable without a
      // name and must not imply that the live connection failed.
    }
    if (!mounted || generation != _generation || found == null) return;
    setState(() {
      _resolvedKey = key;
      _resolved = found;
      if (_invalidatedLiveKey == key) _invalidatedLiveKey = null;
    });
  }

  @override
  Widget build(BuildContext context) {
    final view = widget.inputs.view;
    final saved = view.savedKey != null && view.savedKey == _resolvedKey
        ? _resolved
        : null;
    return widget.builder(
      context,
      saved == null ? view : view.withSaved(saved),
    );
  }
}

/// "Source · choice" on one run of text. A VPN Gate node carries its flag
/// inline so the line wraps like ordinary text in a narrow column.
class ChainSelectionText extends StatelessWidget {
  const ChainSelectionText(
    this.view, {
    this.showSource = true,
    this.style,
    this.maxLines = 2,
    super.key,
  });

  final ChainExitPresentation view;
  final bool showSource;
  final TextStyle? style;
  final int maxLines;

  @override
  Widget build(BuildContext context) {
    final selection = view.selection;
    final spans = <InlineSpan>[
      if (showSource) TextSpan(text: view.source.label),
    ];
    if (selection != null) {
      if (spans.isNotEmpty) spans.add(const TextSpan(text: ' · '));
      if (selection.server case final server?) {
        spans
          ..add(
            WidgetSpan(
              alignment: PlaceholderAlignment.middle,
              child: Padding(
                padding: const EdgeInsetsDirectional.only(end: 6),
                // Sized to the text line so the flag never changes its height.
                child: CountryFlag(
                  countryCode: server.countryCode,
                  size: const Size(20, 15),
                ),
              ),
            ),
          )
          ..add(TextSpan(text: '${server.countryCode ?? '—'} · ${server.ip}'));
      } else {
        spans.add(TextSpan(text: selection.name ?? selection.id ?? ''));
      }
    }
    if (spans.isEmpty) return const SizedBox.shrink();
    final base = style ?? DefaultTextStyle.of(context).style;
    return Text.rich(
      TextSpan(children: spans),
      maxLines: maxLines,
      overflow: TextOverflow.ellipsis,
      style: base.copyWith(fontFeatures: UsqueTheme.tabularFigures),
    );
  }
}
