import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_theme.dart';

/// A page-local acknowledgement, never a persisted security preference.
class ZeroTrustEndpointWarning extends StatefulWidget {
  const ZeroTrustEndpointWarning({required this.strings, super.key});

  final AppStrings strings;

  @override
  State<ZeroTrustEndpointWarning> createState() =>
      _ZeroTrustEndpointWarningState();
}

class _ZeroTrustEndpointWarningState extends State<ZeroTrustEndpointWarning> {
  bool _acknowledged = false;
  final _ackFocus = FocusNode(debugLabel: 'ZT endpoint acknowledgement');

  @override
  void dispose() {
    _ackFocus.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final strings = widget.strings;
    final theme = Theme.of(context);
    return Dialog.fullscreen(
      backgroundColor: UsqueColors.danger,
      child: Shortcuts(
        shortcuts: const {
          SingleActivator(LogicalKeyboardKey.escape): DismissIntent(),
        },
        child: Actions(
          actions: {
            DismissIntent: CallbackAction<DismissIntent>(
              onInvoke: (_) {
                Navigator.pop(context, false);
                return null;
              },
            ),
          },
          child: Theme(
            data: theme.copyWith(
              textTheme: theme.textTheme.apply(
                bodyColor: Colors.white,
                displayColor: Colors.white,
              ),
              colorScheme: theme.colorScheme.copyWith(
                surface: UsqueColors.danger,
                onSurface: Colors.white,
                primary: Colors.white,
                onPrimary: UsqueColors.danger,
                outline: Colors.white,
                onSurfaceVariant: Colors.white,
              ),
              focusColor: Colors.white24,
              checkboxTheme: CheckboxThemeData(
                side: const BorderSide(color: Colors.white, width: 2),
                fillColor: WidgetStateProperty.resolveWith(
                  (states) => states.contains(WidgetState.selected)
                      ? Colors.white
                      : Colors.transparent,
                ),
                checkColor: const WidgetStatePropertyAll(UsqueColors.danger),
              ),
            ),
            child: DefaultTextStyle.merge(
              style: const TextStyle(color: Colors.white),
              child: SafeArea(
                child: Center(
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 808),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Expanded(
                          child: SingleChildScrollView(
                            padding: const EdgeInsets.all(24),
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.stretch,
                              children: [
                                const Align(
                                  alignment: AlignmentDirectional.centerStart,
                                  child: Icon(
                                    LucideIcons.triangleAlert,
                                    color: Colors.white,
                                    size: 40,
                                  ),
                                ),
                                const SizedBox(height: 20),
                                Semantics(
                                  header: true,
                                  namesRoute: true,
                                  child: Text(
                                    strings.get(
                                      'zero_trust_endpoint_risk_title',
                                    ),
                                    style: theme.textTheme.headlineSmall
                                        ?.copyWith(color: Colors.white),
                                  ),
                                ),
                                const SizedBox(height: 20),
                                Text(
                                  strings.get('zero_trust_endpoint_risk_body'),
                                ),
                                const SizedBox(height: 24),
                                CheckboxListTile(
                                  key: const ValueKey('zt-endpoint-risk-ack'),
                                  contentPadding: EdgeInsets.zero,
                                  focusNode: _ackFocus,
                                  controlAffinity:
                                      ListTileControlAffinity.leading,
                                  value: _acknowledged,
                                  onChanged: (value) => setState(
                                    () => _acknowledged = value ?? false,
                                  ),
                                  title: Text(
                                    strings.get('zero_trust_endpoint_risk_ack'),
                                    style: const TextStyle(color: Colors.white),
                                  ),
                                ),
                              ],
                            ),
                          ),
                        ),
                        Padding(
                          padding: const EdgeInsets.fromLTRB(24, 12, 24, 24),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              OutlinedButton(
                                autofocus: true,
                                style: OutlinedButton.styleFrom(
                                  foregroundColor: Colors.white,
                                  side: const BorderSide(color: Colors.white),
                                  minimumSize: const Size(48, 48),
                                ),
                                onPressed: () => Navigator.pop(context, false),
                                child: Text(strings.get('cancel')),
                              ),
                              const SizedBox(height: 12),
                              FilledButton(
                                key: const ValueKey(
                                  'zt-endpoint-risk-continue',
                                ),
                                style: FilledButton.styleFrom(
                                  backgroundColor: Colors.white,
                                  foregroundColor: UsqueColors.danger,
                                  disabledBackgroundColor: Colors.white24,
                                  disabledForegroundColor: Colors.white70,
                                  minimumSize: const Size(48, 48),
                                ),
                                onPressed: _acknowledged
                                    ? () => Navigator.pop(context, true)
                                    : null,
                                child: Text(
                                  strings.get(
                                    'zero_trust_endpoint_risk_continue',
                                  ),
                                ),
                              ),
                            ],
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
