import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/usque_motion.dart';
import '../core/usque_theme.dart';

/// Scrolling frame shared by every section: an eyebrow-free title row, an
/// optional header slot, and a width-limited content column.
class PageFrame extends StatelessWidget {
  const PageFrame({
    required this.title,
    required this.child,
    this.subtitle,
    this.header,
    this.titleWidget,
    this.actions = const <Widget>[],
    super.key,
  });

  final String title;
  final Widget child;
  final String? subtitle;
  final Widget? header;
  final Widget? titleWidget;
  final List<Widget> actions;

  static const double maxContentWidth = 1120;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final double gutter = MediaQuery.sizeOf(context).width < 600 ? 16 : 26;
    return CustomScrollView(
      key: PageStorageKey<String>(title),
      slivers: <Widget>[
        SliverPadding(
          padding: EdgeInsets.fromLTRB(gutter, gutter, gutter, 18),
          sliver: SliverToBoxAdapter(
            child: Align(
              alignment: Alignment.topCenter,
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: maxContentWidth),
                child: LayoutBuilder(
                  builder: (context, constraints) {
                    // A bare Column would shrink-wrap and the Align above
                    // would centre the whole heading, so every branch below
                    // has to claim the full row.
                    final Widget heading = Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: <Widget>[
                        if (header != null) ...<Widget>[
                          header!,
                          const SizedBox(height: 18),
                        ],
                        titleWidget ??
                            Text(title, style: theme.textTheme.headlineMedium),
                        if (subtitle != null) ...<Widget>[
                          const SizedBox(height: 6),
                          Text(
                            subtitle!,
                            style: theme.textTheme.bodyMedium?.copyWith(
                              color: theme.colorScheme.onSurfaceVariant,
                            ),
                          ),
                        ],
                      ],
                    );
                    if (actions.isEmpty) {
                      return SizedBox(width: double.infinity, child: heading);
                    }
                    // Below this width the title and its actions stop being a
                    // row: the buttons drop under the heading instead of
                    // squeezing it.
                    if (constraints.maxWidth < 560) {
                      return Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: <Widget>[
                          heading,
                          const SizedBox(height: 16),
                          Wrap(spacing: 8, runSpacing: 8, children: actions),
                        ],
                      );
                    }
                    return Row(
                      crossAxisAlignment: CrossAxisAlignment.end,
                      children: <Widget>[
                        Expanded(child: heading),
                        const SizedBox(width: 16),
                        Wrap(spacing: 8, runSpacing: 8, children: actions),
                      ],
                    );
                  },
                ),
              ),
            ),
          ),
        ),
        SliverPadding(
          padding: EdgeInsets.fromLTRB(gutter, 0, gutter, 34),
          sliver: SliverToBoxAdapter(
            child: Align(
              alignment: Alignment.topCenter,
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: maxContentWidth),
                child: child,
              ),
            ),
          ),
        ),
      ],
    );
  }
}

/// A full-screen route pushed over the shell.
///
/// Repeats the shell's page grammar — back link, large title, optional
/// subtitle, trailing actions — instead of a Material app bar, so a pushed
/// screen reads as the same instrument rather than a different app.
class SubPage extends StatelessWidget {
  const SubPage({
    required this.title,
    required this.backLabel,
    required this.child,
    this.subtitle,
    this.actions = const <Widget>[],
    this.bottomBar,
    super.key,
  });

  final String title;
  final String backLabel;
  final Widget child;
  final String? subtitle;
  final List<Widget> actions;
  final Widget? bottomBar;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      bottomNavigationBar: bottomBar,
      body: SafeArea(
        child: PageFrame(
          title: title,
          subtitle: subtitle,
          actions: actions,
          header: Align(
            alignment: AlignmentDirectional.centerStart,
            child: TextButton.icon(
              onPressed: () => Navigator.of(context).maybePop(),
              icon: const Icon(LucideIcons.arrowLeftDir, size: 17),
              label: Text(backLabel),
              style: TextButton.styleFrom(
                alignment: AlignmentDirectional.centerStart,
                padding: const EdgeInsetsDirectional.fromSTEB(2, 8, 12, 8),
                foregroundColor: Theme.of(context).colorScheme.onSurfaceVariant,
              ),
            ),
          ),
          child: child,
        ),
      ),
    );
  }
}

/// A vertical run of panels under one gap value.
///
/// Screens list their sections and never hand-space them, which is the only
/// reliable way to keep a long settings page evenly ruled.
class PanelStack extends StatelessWidget {
  const PanelStack({required this.children, this.spacing = 16, super.key});

  final List<Widget> children;
  final double spacing;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        for (int index = 0; index < children.length; index++) ...<Widget>[
          if (index != 0) SizedBox(height: spacing),
          children[index],
        ],
      ],
    );
  }
}

/// A hairline instrument plate. The border warms slightly under the pointer so
/// panels feel physical without adding shadows to the page.
class Panel extends StatefulWidget {
  const Panel({
    required this.child,
    this.padding = const EdgeInsets.all(20),
    this.color,
    this.onTap,
    this.accent,
    super.key,
  });

  final Widget child;
  final EdgeInsetsGeometry padding;
  final Color? color;
  final VoidCallback? onTap;

  /// Draws a 3px marker down the leading edge; used to flag the active profile.
  final Color? accent;

  @override
  State<Panel> createState() => _PanelState();
}

class _PanelState extends State<Panel> {
  bool _hovered = false;
  bool _focused = false;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final UsqueTokens tokens = UsqueTokens.of(context);
    final bool interactive = widget.onTap != null;
    final bool focused = interactive && _focused;
    final bool lifted = interactive && (_hovered || focused);
    final BorderSide border = focused
        ? BorderSide(color: theme.colorScheme.primary, width: 2)
        : BorderSide(color: lifted ? tokens.hairlineStrong : tokens.hairline);

    Widget panelChild = Padding(padding: widget.padding, child: widget.child);
    if (interactive) {
      panelChild = InkWell(
        onTap: widget.onTap,
        onHover: (value) => setState(() => _hovered = value),
        onFocusChange: (value) => setState(() => _focused = value),
        borderRadius: BorderRadius.circular(UsqueRadii.card),
        child: panelChild,
      );
    }

    // The fill lives on a Material so ListTile ink still lands on a surface;
    // the container only carries the lift shadow. Interactive panels use an
    // InkWell so touch, pointer, keyboard, and D-pad input share one state
    // layer and focus model.
    Widget content = AnimatedContainer(
      duration: UsqueMotion.of(context, UsqueMotion.fast),
      curve: UsqueMotion.standard,
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(UsqueRadii.card),
        boxShadow: lifted
            ? <BoxShadow>[
                BoxShadow(
                  color: theme.colorScheme.shadow.withValues(alpha: 0.06),
                  blurRadius: 18,
                  offset: const Offset(0, 6),
                ),
              ]
            : const <BoxShadow>[],
      ),
      child: Material(
        color: widget.color ?? theme.colorScheme.surface,
        animationDuration: UsqueMotion.of(context, UsqueMotion.fast),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(UsqueRadii.card),
          side: border,
        ),
        clipBehavior: Clip.antiAlias,
        child: panelChild,
      ),
    );

    if (widget.accent != null) {
      content = Stack(
        children: <Widget>[
          content,
          PositionedDirectional(
            top: 18,
            bottom: 18,
            start: 0,
            child: IgnorePointer(
              child: Container(
                width: 3,
                decoration: BoxDecoration(
                  color: widget.accent,
                  borderRadius: const BorderRadius.horizontal(
                    right: Radius.circular(3),
                  ),
                ),
              ),
            ),
          ),
        ],
      );
    }

    if (!interactive) {
      return content;
    }
    return Semantics(button: true, child: content);
  }
}

/// Icon tile plus title and optional supporting line. Opens every panel.
class SectionTitle extends StatelessWidget {
  const SectionTitle({
    required this.icon,
    required this.title,
    this.subtitle,
    this.trailing,
    super.key,
  });

  final IconData icon;
  final String title;
  final String? subtitle;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Container(
          width: 34,
          height: 34,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: theme.colorScheme.primary.withValues(
              alpha: UsqueTokens.of(context).tint,
            ),
            borderRadius: BorderRadius.circular(UsqueRadii.chip),
          ),
          child: Icon(icon, size: 17, color: theme.colorScheme.primary),
        ),
        const SizedBox(width: 12),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Padding(
                padding: const EdgeInsets.only(top: 2),
                child: Text(title, style: theme.textTheme.titleMedium),
              ),
              if (subtitle != null) ...<Widget>[
                const SizedBox(height: 4),
                Text(
                  subtitle!,
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: theme.colorScheme.onSurfaceVariant,
                  ),
                ),
              ],
            ],
          ),
        ),
        if (trailing != null) ...<Widget>[const SizedBox(width: 12), trailing!],
      ],
    );
  }
}

/// The common panel shape: a [SectionTitle] header over a body column.
class SectionPanel extends StatelessWidget {
  const SectionPanel({
    required this.icon,
    required this.title,
    required this.children,
    this.subtitle,
    this.trailing,
    this.gap = 18,
    super.key,
  });

  final IconData icon;
  final String title;
  final String? subtitle;
  final Widget? trailing;
  final List<Widget> children;

  /// Space between the header and the body.
  final double gap;

  @override
  Widget build(BuildContext context) {
    return Panel(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          SectionTitle(
            icon: icon,
            title: title,
            subtitle: subtitle,
            trailing: trailing,
          ),
          if (children.isNotEmpty) ...<Widget>[
            SizedBox(height: gap),
            ...children,
          ],
        ],
      ),
    );
  }
}

enum StatusTone { success, warning, danger, brand, neutral }

/// Foreground colour for a tone, resolved against the current theme.
Color statusToneColor(BuildContext context, StatusTone tone) {
  final ThemeData theme = Theme.of(context);
  final UsqueTokens tokens = UsqueTokens.of(context);
  return switch (tone) {
    StatusTone.success => tokens.success,
    StatusTone.warning => tokens.caution,
    StatusTone.danger => tokens.danger,
    // The logo orange is deliberately vivid and misses the 3:1 graphical
    // contrast threshold on its own light tint. Use the accessible ember for
    // status indicators in light mode; dark surfaces can keep the brand hue.
    StatusTone.brand =>
      theme.brightness == Brightness.light
          ? theme.colorScheme.primary
          : tokens.brand,
    StatusTone.neutral => theme.colorScheme.onSurfaceVariant,
  };
}

/// Compact state marker. Reads as a lamp on an instrument: a lit dot, a short
/// label, and a wash of the same colour behind it.
class StatusPill extends StatelessWidget {
  const StatusPill({
    required this.label,
    required this.tone,
    this.icon,
    this.dim = false,
    this.showIndicator = true,
    super.key,
  });

  final String label;
  final StatusTone tone;

  /// Optional glyph. Without one the pill shows a status dot.
  final IconData? icon;

  /// Renders the pill without a fill, for dense rows of many pills.
  final bool dim;

  /// Drops the dot or glyph when a dense row does not need a status marker.
  final bool showIndicator;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final UsqueTokens tokens = UsqueTokens.of(context);
    final Color foreground = statusToneColor(context, tone);
    final Color background = tone == StatusTone.neutral
        ? theme.colorScheme.surfaceContainerHigh
        : foreground.withValues(alpha: tokens.tint);
    // Status hue belongs to the lamp/icon and wash. Small labels use a
    // semantic foreground that remains AA-readable over every tinted surface.
    final Color labelColor = tone == StatusTone.neutral
        ? theme.colorScheme.onSurfaceVariant
        : theme.colorScheme.onSurface;
    final Widget text = Text(
      label,
      style: theme.textTheme.labelMedium?.copyWith(
        color: labelColor,
        fontWeight: FontWeight.w700,
      ),
    );

    return AnimatedContainer(
      duration: UsqueMotion.of(context, UsqueMotion.base),
      curve: UsqueMotion.standard,
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
      decoration: BoxDecoration(
        color: dim ? Colors.transparent : background,
        borderRadius: BorderRadius.circular(UsqueRadii.pill),
        border: dim ? Border.all(color: tokens.hairline) : null,
      ),
      child: !showIndicator
          ? text
          : Row(
              mainAxisSize: MainAxisSize.min,
              children: <Widget>[
                if (icon != null)
                  Icon(icon, size: 14, color: foreground)
                else
                  _StatusDot(color: foreground),
                const SizedBox(width: 7),
                Flexible(child: text),
              ],
            ),
    );
  }
}

class _StatusDot extends StatelessWidget {
  const _StatusDot({required this.color});

  final Color color;

  @override
  Widget build(BuildContext context) {
    return AnimatedContainer(
      duration: UsqueMotion.of(context, UsqueMotion.base),
      width: 7,
      height: 7,
      decoration: BoxDecoration(color: color, shape: BoxShape.circle),
    );
  }
}

/// Inline advisory. Warnings explain the exposure; errors explain the failure.
class WarningBanner extends StatelessWidget {
  const WarningBanner({
    required this.title,
    required this.message,
    this.onDismiss,
    this.danger = false,
    super.key,
  });

  final String title;
  final String message;
  final VoidCallback? onDismiss;
  final bool danger;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final UsqueTokens tokens = UsqueTokens.of(context);
    final Color foreground = danger ? tokens.danger : tokens.caution;
    return Semantics(
      liveRegion: true,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: foreground.withValues(alpha: danger ? 0.09 : 0.10),
          border: Border.all(color: foreground.withValues(alpha: 0.30)),
          borderRadius: BorderRadius.circular(UsqueRadii.control),
        ),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(14, 13, 10, 13),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Padding(
                padding: const EdgeInsets.only(top: 1),
                child: Icon(
                  danger ? LucideIcons.circleX : LucideIcons.triangleAlert,
                  color: foreground,
                  size: 18,
                ),
              ),
              const SizedBox(width: 11),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: <Widget>[
                    Text(
                      title,
                      style: theme.textTheme.titleSmall?.copyWith(
                        color: foreground,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                    const SizedBox(height: 3),
                    Text(message, style: theme.textTheme.bodyMedium),
                  ],
                ),
              ),
              if (onDismiss != null) ...<Widget>[
                const SizedBox(width: 4),
                IconButton(
                  tooltip: MaterialLocalizations.of(context).closeButtonTooltip,
                  onPressed: onDismiss,
                  iconSize: 17,
                  visualDensity: VisualDensity.compact,
                  color: foreground,
                  icon: const Icon(LucideIcons.x),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

/// Reserves no space until [child] arrives, then grows and fades it in.
///
/// Keeps banner appearance from snapping the page layout.
class BannerSlot extends StatelessWidget {
  const BannerSlot({required this.child, this.spacing = 16, super.key});

  final Widget? child;
  final double spacing;

  @override
  Widget build(BuildContext context) {
    final content = child == null
        ? const SizedBox(width: double.infinity, height: 0)
        : Padding(
            key: const ValueKey<String>('banner'),
            padding: EdgeInsets.only(bottom: spacing),
            child: child,
          );
    // A zero-duration AnimatedSize can synchronously re-dirty itself when
    // large text changes the banner's measured height. Reduced motion should
    // bypass layout animation altogether, not animate with a zero duration.
    if (UsqueMotion.reduced(context)) return content;
    return AnimatedSize(
      duration: UsqueMotion.of(context, UsqueMotion.gentle),
      curve: UsqueMotion.emphasized,
      alignment: Alignment.topCenter,
      child: FadeThroughSwitcher(child: content),
    );
  }
}

/// Label on the left, value on the right. The workhorse of every readout.
class ReadoutRow extends StatelessWidget {
  const ReadoutRow({
    required this.label,
    required this.value,
    this.icon,
    this.leading,
    this.valueColor,
    this.stackWhenNarrow = false,
    super.key,
  });

  final String label;
  final Widget value;
  final IconData? icon;
  final Widget? leading;
  final Color? valueColor;
  final bool stackWhenNarrow;

  /// Builds a row whose value is plain text.
  static Widget text(
    BuildContext context, {
    required String label,
    required String value,
    IconData? icon,
    Widget? leading,
  }) {
    return ReadoutRow(
      label: label,
      icon: icon,
      leading: leading,
      value: Text(
        value,
        textAlign: TextAlign.end,
        style: Theme.of(context).textTheme.titleSmall,
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final leadingWidgets = <Widget>[
      if (leading != null)
        SizedBox(width: 22, child: Center(child: leading))
      else if (icon != null)
        SizedBox(
          width: 22,
          child: Icon(
            icon,
            size: 17,
            color: theme.colorScheme.onSurfaceVariant,
          ),
        ),
      if (leading != null || icon != null) const SizedBox(width: 11),
    ];
    final labelText = Text(
      label,
      style: theme.textTheme.bodyMedium?.copyWith(
        color: theme.colorScheme.onSurfaceVariant,
      ),
    );
    return LayoutBuilder(
      builder: (context, constraints) {
        if (stackWhenNarrow &&
            (constraints.maxWidth < 320 ||
                MediaQuery.textScalerOf(context).scale(14) > 21)) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  ...leadingWidgets,
                  Expanded(child: labelText),
                ],
              ),
              const SizedBox(height: 6),
              Align(alignment: AlignmentDirectional.centerEnd, child: value),
            ],
          );
        }
        return Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            ...leadingWidgets,
            Expanded(child: labelText),
            const SizedBox(width: 14),
            Flexible(
              child: Align(
                alignment: AlignmentDirectional.centerEnd,
                child: value,
              ),
            ),
          ],
        );
      },
    );
  }
}

/// Selectable machine value in the mono face.
class MonoValue extends StatefulWidget {
  const MonoValue({
    required this.value,
    this.muted = false,
    this.size,
    super.key,
  });

  final String value;
  final bool muted;
  final double? size;

  @override
  State<MonoValue> createState() => _MonoValueState();
}

class _MonoValueState extends State<MonoValue> {
  // SelectableText has an internal scrollable. Without a storage boundary it
  // inherits the page/ExpansionTile key, reads a bool as a scroll offset, and
  // can overwrite its parent's state. Machine values need no persisted offset.
  final _textStorage = PageStorageBucket();

  @override
  Widget build(BuildContext context) {
    return PageStorage(
      bucket: _textStorage,
      child: SelectableText(
        widget.value,
        textAlign: TextAlign.end,
        style: UsqueTheme.mono(
          context,
          size: widget.size,
          weight: FontWeight.w500,
          color: widget.muted
              ? Theme.of(context).colorScheme.onSurfaceVariant
              : Theme.of(context).colorScheme.onSurface,
        ),
      ),
    );
  }
}

class EmptyValue extends StatelessWidget {
  const EmptyValue({required this.label, super.key});

  final String label;

  @override
  Widget build(BuildContext context) {
    return Text(
      label,
      style: TextStyle(color: Theme.of(context).colorScheme.onSurfaceVariant),
    );
  }
}

String formatRate(int bytesPerSecond) {
  if (bytesPerSecond < 1000) {
    return '$bytesPerSecond B/s';
  }
  if (bytesPerSecond < 1000 * 1000) {
    return '${(bytesPerSecond / 1000).toStringAsFixed(1)} KB/s';
  }
  if (bytesPerSecond < 1000 * 1000 * 1000) {
    return '${(bytesPerSecond / (1000 * 1000)).toStringAsFixed(1)} MB/s';
  }
  return '${(bytesPerSecond / (1000 * 1000 * 1000)).toStringAsFixed(1)} GB/s';
}

String formatDuration(Duration duration) {
  final hours = duration.inHours.toString().padLeft(2, '0');
  final minutes = (duration.inMinutes % 60).toString().padLeft(2, '0');
  final seconds = (duration.inSeconds % 60).toString().padLeft(2, '0');
  return '$hours:$minutes:$seconds';
}
