import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/diagnostics_strings.dart';
import '../core/usque_theme.dart';
import '../models/diagnostics_models.dart';

class DiagnosticFindingCard extends StatelessWidget {
  const DiagnosticFindingCard({
    required this.finding,
    required this.strings,
    super.key,
  });

  final DiagnosticFinding finding;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    final failure = finding.failure;
    final theme = Theme.of(context);
    final tokens = UsqueTokens.of(context);
    final color = _statusColor(tokens, finding.status);
    final remediation = failure?.remediationKey.isNotEmpty == true
        ? failure!.remediationKey
        : finding.remediationKey;
    return Container(
      margin: const EdgeInsetsDirectional.fromSTEB(46, 0, 12, 12),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: color.withValues(alpha: tokens.tint * 0.55),
        border: Border.all(color: color.withValues(alpha: 0.32)),
        borderRadius: BorderRadius.circular(UsqueRadii.control),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          if (failure != null) ...<Widget>[
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Expanded(
                  child: Text(
                    diagnosticFailureTitle(strings, failure.code),
                    style: theme.textTheme.titleSmall,
                  ),
                ),
                const SizedBox(width: 12),
                Flexible(
                  child: Text(
                    failure.code,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    textAlign: TextAlign.end,
                    style: theme.textTheme.labelSmall?.copyWith(
                      fontFamily: UsqueFonts.mono,
                      fontFamilyFallback: UsqueFonts.monoFallback,
                      color: color,
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 10),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: <Widget>[
                _Fact(
                  label: strings.languageCode == 'zh' ? '阶段' : 'Stage',
                  value: failure.stage,
                ),
                if (failure.transport != null)
                  _Fact(
                    label: strings.languageCode == 'zh' ? '传输' : 'Transport',
                    value: failure.transport!,
                  ),
                if (failure.addressFamily != null)
                  _Fact(
                    label: strings.languageCode == 'zh' ? '地址族' : 'Family',
                    value: failure.addressFamily!,
                  ),
                _Fact(
                  label: strings.languageCode == 'zh' ? '可重试' : 'Retryable',
                  value: _yesNo(strings, failure.retryable),
                ),
                _Fact(
                  label: strings.languageCode == 'zh' ? '允许回退' : 'Fallback',
                  value: _yesNo(strings, failure.fallbackAllowed),
                ),
              ],
            ),
            const SizedBox(height: 12),
            Text(
              diagnosticRemediation(strings, remediation),
              style: theme.textTheme.bodyMedium,
            ),
            const SizedBox(height: 12),
            Align(
              alignment: AlignmentDirectional.centerEnd,
              child: OutlinedButton.icon(
                onPressed: () => _copySupportInfo(context, failure),
                icon: const Icon(LucideIcons.copy, size: 17),
                label: Text(
                  strings.languageCode == 'zh' ? '复制支持信息' : 'Copy support info',
                ),
              ),
            ),
          ] else ...<Widget>[
            Text(
              _summaryText(strings, finding),
              style: theme.textTheme.bodyMedium,
            ),
            if (finding.dependencyReason?.isNotEmpty == true) ...<Widget>[
              const SizedBox(height: 8),
              Text(
                finding.dependencyReason!,
                style: theme.textTheme.bodySmall?.copyWith(
                  color: theme.colorScheme.onSurfaceVariant,
                  fontFamily: UsqueFonts.mono,
                  fontFamilyFallback: UsqueFonts.monoFallback,
                ),
              ),
            ],
          ],
          if (finding.sanitizedEvidence.isNotEmpty) ...<Widget>[
            const SizedBox(height: 10),
            Wrap(
              spacing: 6,
              runSpacing: 6,
              children: finding.sanitizedEvidence
                  .map(
                    (value) => Chip(
                      visualDensity: VisualDensity.compact,
                      label: Text(value),
                    ),
                  )
                  .toList(growable: false),
            ),
          ],
        ],
      ),
    );
  }

  Future<void> _copySupportInfo(
    BuildContext context,
    TransportFailureInfo failure,
  ) async {
    final parts = <String>[
      'code=${failure.code}',
      'stage=${failure.stage}',
      if (failure.transport != null) 'transport=${failure.transport}',
      if (failure.addressFamily != null) 'family=${failure.addressFamily}',
      'retryable=${failure.retryable}',
      'fallback_allowed=${failure.fallbackAllowed}',
    ];
    await Clipboard.setData(ClipboardData(text: parts.join('\n')));
    if (context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            strings.languageCode == 'zh'
                ? '支持信息已复制。'
                : 'Support information copied.',
          ),
        ),
      );
    }
  }
}

class _Fact extends StatelessWidget {
  const _Fact({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 6),
      decoration: BoxDecoration(
        border: Border.all(color: UsqueTokens.of(context).hairline),
        borderRadius: BorderRadius.circular(UsqueRadii.chip),
      ),
      child: Text(
        '$label · $value',
        style: Theme.of(context).textTheme.labelSmall,
      ),
    );
  }
}

String _yesNo(AppStrings strings, bool value) {
  if (strings.languageCode == 'zh') {
    return value ? '是' : '否';
  }
  return value ? 'Yes' : 'No';
}

String _summaryText(AppStrings strings, DiagnosticFinding finding) {
  final zh = strings.languageCode == 'zh';
  return switch (finding.status) {
    DiagnosticCheckStatus.passed => zh ? '检查通过。' : 'This check passed.',
    DiagnosticCheckStatus.warning =>
      zh ? '检查需要注意。' : 'This check needs attention.',
    DiagnosticCheckStatus.failed => zh ? '检查失败。' : 'This check failed.',
    DiagnosticCheckStatus.skipped =>
      zh
          ? '由于当前状态或依赖项，此检查未运行。'
          : 'This check did not run in the current state.',
    DiagnosticCheckStatus.cancelled =>
      zh ? '检查已取消。' : 'This check was cancelled.',
    DiagnosticCheckStatus.running => zh ? '检查正在运行。' : 'This check is running.',
    DiagnosticCheckStatus.pending =>
      zh ? '检查尚未开始。' : 'This check has not started.',
  };
}

Color _statusColor(UsqueTokens tokens, DiagnosticCheckStatus status) {
  return switch (status) {
    DiagnosticCheckStatus.passed => tokens.success,
    DiagnosticCheckStatus.warning => tokens.caution,
    DiagnosticCheckStatus.failed => tokens.danger,
    DiagnosticCheckStatus.running => tokens.brand,
    DiagnosticCheckStatus.pending ||
    DiagnosticCheckStatus.skipped ||
    DiagnosticCheckStatus.cancelled => tokens.hairlineStrong,
  };
}
