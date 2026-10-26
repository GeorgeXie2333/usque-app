import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/diagnostics_presentation.dart';
import '../core/diagnostics_strings.dart';
import '../core/usque_theme.dart';
import '../models/diagnostics_models.dart';
import 'diagnostic_finding_card.dart';

class DiagnosticCheckTile extends StatelessWidget {
  const DiagnosticCheckTile({
    required this.finding,
    required this.strings,
    super.key,
  });

  final DiagnosticFinding finding;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    final tokens = UsqueTokens.of(context);
    final color = diagnosticCheckStatusColor(tokens, finding.status);
    final label = diagnosticCheckLabel(strings, finding.checkId);
    final status = diagnosticStatusLabel(strings, finding.status);
    return ExpansionTile(
      key: PageStorageKey<String>('diagnostic-${finding.checkId}'),
      internalAddSemanticForOnTap: true,
      tilePadding: const EdgeInsetsDirectional.fromSTEB(14, 2, 12, 2),
      childrenPadding: EdgeInsets.zero,
      minTileHeight: 52,
      leading: SizedBox(
        width: 28,
        height: 28,
        child: Center(
          child: finding.status == DiagnosticCheckStatus.running
              ? SizedBox(
                  width: 18,
                  height: 18,
                  child: CircularProgressIndicator(
                    strokeWidth: 2,
                    color: color,
                  ),
                )
              : Icon(
                  diagnosticCheckStatusIcon(finding.status),
                  size: 19,
                  color: color,
                ),
        ),
      ),
      title: Text(label, maxLines: 2, overflow: TextOverflow.ellipsis),
      subtitle: finding.failure == null
          ? null
          : Text(
              finding.failure!.code,
              style: Theme.of(context).textTheme.labelSmall?.copyWith(
                color: color,
                fontFamily: UsqueFonts.mono,
                fontFamilyFallback: UsqueFonts.monoFallback,
              ),
            ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: <Widget>[
          Text(
            status,
            style: Theme.of(
              context,
            ).textTheme.labelSmall?.copyWith(color: color),
          ),
          const SizedBox(width: 6),
          const Icon(LucideIcons.chevronDown, size: 17),
        ],
      ),
      children: <Widget>[
        DiagnosticFindingCard(finding: finding, strings: strings),
      ],
    );
  }
}
