import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_motion.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';

class OnboardingScreen extends StatefulWidget {
  const OnboardingScreen({required this.controller, super.key});

  final AppController controller;

  @override
  State<OnboardingScreen> createState() => _OnboardingScreenState();
}

class _OnboardingScreenState extends State<OnboardingScreen> {
  static const int _stepCount = 4;

  final TextEditingController _licenseController = TextEditingController();
  int _step = 0;

  /// Direction of the last step change, so a step slides in from the side the
  /// user came from.
  bool _forward = true;
  bool _termsAccepted = false;
  bool _useLicense = false;
  bool _licenseVisible = false;

  AppStrings get strings => widget.controller.strings;

  @override
  void dispose() {
    _licenseController
      ..clear()
      ..dispose();
    super.dispose();
  }

  void _goTo(int step) {
    setState(() {
      _forward = step > _step;
      _step = step;
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: UsqueTokens.of(context).canvas,
      body: SafeArea(
        child: LayoutBuilder(
          builder: (context, constraints) {
            final wide = constraints.maxWidth >= 820;
            return Row(
              children: <Widget>[
                if (wide)
                  Expanded(flex: 4, child: _BrandPane(strings: strings)),
                Expanded(
                  flex: 6,
                  child: Center(
                    child: SingleChildScrollView(
                      padding: EdgeInsets.symmetric(
                        horizontal: wide ? 64 : 24,
                        vertical: 32,
                      ),
                      child: ConstrainedBox(
                        constraints: const BoxConstraints(maxWidth: 620),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: <Widget>[
                            if (!wide) ...<Widget>[
                              Row(
                                children: <Widget>[
                                  Image.asset(
                                    'assets/branding/usque-ui-icon.png',
                                    width: 42,
                                    height: 42,
                                    filterQuality: FilterQuality.medium,
                                  ),
                                  const SizedBox(width: 12),
                                  Text(
                                    'Usque',
                                    style: Theme.of(
                                      context,
                                    ).textTheme.titleLarge,
                                  ),
                                ],
                              ),
                              const SizedBox(height: 34),
                            ],
                            _StepIndicator(
                              step: _step,
                              total: _stepCount,
                              strings: strings,
                            ),
                            const SizedBox(height: 30),
                            AnimatedSize(
                              duration: UsqueMotion.of(
                                context,
                                UsqueMotion.gentle,
                              ),
                              curve: UsqueMotion.emphasized,
                              alignment: Alignment.topCenter,
                              child: _StepTransition(
                                step: _step,
                                forward: _forward,
                                child: _buildStep(context),
                              ),
                            ),
                            const SizedBox(height: 20),
                            BannerSlot(
                              spacing: 0,
                              child: widget.controller.lastError == null
                                  ? null
                                  : WarningBanner(
                                      title: strings.get('setup_failed'),
                                      message: widget.controller.lastError!,
                                      danger: true,
                                      onDismiss: widget.controller.clearError,
                                    ),
                            ),
                            const SizedBox(height: 28),
                            _buildActions(context),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
              ],
            );
          },
        ),
      ),
    );
  }

  Widget _buildStep(BuildContext context) {
    return switch (_step) {
      0 => _IntroStep(strings: strings),
      1 => _PermissionsStep(strings: strings),
      2 => _TermsStep(
        strings: strings,
        accepted: _termsAccepted,
        onChanged: (value) => setState(() => _termsAccepted = value),
      ),
      _ => _IdentityStep(
        strings: strings,
        useLicense: _useLicense,
        licenseVisible: _licenseVisible,
        licenseController: _licenseController,
        onMethodChanged: (value) => setState(() => _useLicense = value),
        onVisibilityChanged: () =>
            setState(() => _licenseVisible = !_licenseVisible),
        onLicenseChanged: (_) => setState(() {}),
      ),
    };
  }

  Widget _buildActions(BuildContext context) {
    final isLast = _step == _stepCount - 1;
    final canContinue = switch (_step) {
      2 => _termsAccepted,
      3 => !_useLicense || _licenseController.text.trim().isNotEmpty,
      _ => true,
    };
    return Row(
      children: <Widget>[
        if (_step > 0)
          OutlinedButton.icon(
            onPressed: widget.controller.busy ? null : () => _goTo(_step - 1),
            icon: const Icon(LucideIcons.arrowLeft),
            label: Text(strings.get('back')),
          ),
        const Spacer(),
        FilledButton.icon(
          onPressed: !canContinue || widget.controller.busy
              ? null
              : () async {
                  if (!isLast) {
                    _goTo(_step + 1);
                    return;
                  }
                  final value = _useLicense
                      ? _licenseController.text.trim()
                      : null;
                  await widget.controller.finishOnboarding(
                    method: _useLicense
                        ? IdentityProvisioningMethod.registerWithLicense
                        : IdentityProvisioningMethod.register,
                    licenseKey: value,
                  );
                  _licenseController.clear();
                },
          icon: widget.controller.busy
              ? SizedBox(
                  width: 18,
                  height: 18,
                  child: CircularProgressIndicator(
                    strokeWidth: 2,
                    color: Theme.of(context).colorScheme.onPrimary,
                  ),
                )
              : Icon(isLast ? LucideIcons.shieldCheck : LucideIcons.arrowRight),
          label: Text(strings.get(isLast ? 'finish_setup' : 'continue')),
        ),
      ],
    );
  }
}

/// Slides one step out and the next one in, in the direction of travel.
class _StepTransition extends StatelessWidget {
  const _StepTransition({
    required this.step,
    required this.forward,
    required this.child,
  });

  final int step;
  final bool forward;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final Key key = ValueKey<int>(step);
    final double travel = forward ? 0.05 : -0.05;
    return AnimatedSwitcher(
      duration: UsqueMotion.of(context, UsqueMotion.gentle),
      switchInCurve: UsqueMotion.emphasized,
      switchOutCurve: UsqueMotion.exit,
      layoutBuilder: (currentChild, previousChildren) => Stack(
        alignment: Alignment.topLeft,
        children: <Widget>[...previousChildren, ?currentChild],
      ),
      transitionBuilder: (child, animation) {
        final bool incoming = child.key == key;
        return FadeTransition(
          opacity: animation,
          child: SlideTransition(
            position: Tween<Offset>(
              begin: Offset(incoming ? travel : -travel, 0),
              end: Offset.zero,
            ).animate(animation),
            child: child,
          ),
        );
      },
      child: KeyedSubtree(key: key, child: child),
    );
  }
}

/// Left half of the setup window: the mark, a quiet instrument motif, and the
/// one disclaimer every user should read before they connect.
class _BrandPane extends StatelessWidget {
  const _BrandPane({required this.strings});

  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final UsqueTokens tokens = UsqueTokens.of(context);
    final bool dark = theme.brightness == Brightness.dark;

    return DecoratedBox(
      decoration: BoxDecoration(
        gradient: LinearGradient(
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
          colors: dark
              ? const <Color>[Color(0xFF1C120A), Color(0xFF121214)]
              : const <Color>[Color(0xFFFFF3E8), Color(0xFFF7F5F0)],
        ),
        border: Border(right: BorderSide(color: tokens.hairline)),
      ),
      child: Stack(
        fit: StackFit.expand,
        children: <Widget>[
          Positioned(
            right: -170,
            top: 90,
            width: 460,
            height: 460,
            child: RepaintBoundary(
              child: CustomPaint(
                painter: _BrandMotifPainter(
                  accent: tokens.brand,
                  track: tokens.hairlineStrong,
                ),
              ),
            ),
          ),
          Padding(
            padding: const EdgeInsets.all(48),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Image.asset(
                  'assets/branding/usque-ui-icon.png',
                  width: 72,
                  height: 72,
                  filterQuality: FilterQuality.medium,
                ),
                const Spacer(),
                Text(
                  'Usque',
                  style: theme.textTheme.displayMedium?.copyWith(
                    color: theme.colorScheme.onSurface,
                  ),
                ),
                const SizedBox(height: 14),
                ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 320),
                  child: Text(
                    strings.get('unofficial'),
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: theme.colorScheme.onSurfaceVariant,
                      height: 1.5,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// Concentric bezels, echoing the connection ring the user is about to meet.
class _BrandMotifPainter extends CustomPainter {
  const _BrandMotifPainter({required this.accent, required this.track});

  final Color accent;
  final Color track;

  @override
  void paint(Canvas canvas, Size size) {
    final Offset center = size.center(Offset.zero);
    final double outer = size.shortestSide / 2;

    canvas.drawCircle(
      center,
      outer,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1
        ..color = track.withValues(alpha: 0.5),
    );
    canvas.drawCircle(
      center,
      outer * 0.72,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.4
        ..color = accent.withValues(alpha: 0.28),
    );
    canvas.drawArc(
      Rect.fromCircle(center: center, radius: outer * 0.54),
      -math.pi / 2,
      math.pi * 0.75,
      false,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2.4
        ..strokeCap = StrokeCap.round
        ..color = accent.withValues(alpha: 0.5),
    );

    final Paint tick = Paint()
      ..strokeWidth = 1.2
      ..strokeCap = StrokeCap.round
      ..color = track.withValues(alpha: 0.65);
    for (int i = 0; i < 48; i += 1) {
      final double angle = -math.pi / 2 + (i / 48) * 2 * math.pi;
      final Offset direction = Offset(math.cos(angle), math.sin(angle));
      final double length = i % 4 == 0 ? 11 : 6;
      canvas.drawLine(
        center + direction * (outer * 0.88 - length),
        center + direction * (outer * 0.88),
        tick,
      );
    }
  }

  @override
  bool shouldRepaint(covariant _BrandMotifPainter oldDelegate) =>
      oldDelegate.accent != accent || oldDelegate.track != track;
}

class _StepIndicator extends StatelessWidget {
  const _StepIndicator({
    required this.step,
    required this.total,
    required this.strings,
  });

  final int step;
  final int total;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final UsqueTokens tokens = UsqueTokens.of(context);
    return Semantics(
      label: strings
          .get('setup_progress')
          .replaceAll('{current}', '${step + 1}')
          .replaceAll('{total}', '$total'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Row(
            children: List<Widget>.generate(total, (index) {
              final bool reached = index <= step;
              return Expanded(
                child: AnimatedContainer(
                  duration: UsqueMotion.of(context, UsqueMotion.base),
                  curve: UsqueMotion.emphasized,
                  height: 3,
                  margin: EdgeInsets.only(right: index == total - 1 ? 0 : 6),
                  decoration: BoxDecoration(
                    color: reached ? tokens.brand : tokens.hairlineStrong,
                    borderRadius: BorderRadius.circular(3),
                  ),
                ),
              );
            }),
          ),
          const SizedBox(height: 12),
          Text(
            '${step + 1} / $total',
            style: UsqueTheme.mono(
              context,
              size: 12,
              color: theme.colorScheme.onSurfaceVariant,
            ),
          ),
        ],
      ),
    );
  }
}

class _StepHeading extends StatelessWidget {
  const _StepHeading({required this.icon, required this.title, this.body});

  final IconData icon;
  final String title;
  final String? body;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Container(
          width: 46,
          height: 46,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: theme.colorScheme.primary.withValues(
              alpha: UsqueTokens.of(context).tint,
            ),
            borderRadius: BorderRadius.circular(UsqueRadii.control),
          ),
          child: Icon(icon, color: theme.colorScheme.primary, size: 22),
        ),
        const SizedBox(height: 22),
        Text(title, style: theme.textTheme.headlineMedium),
        if (body case final body?) ...<Widget>[
          const SizedBox(height: 12),
          Text(
            body,
            style: theme.textTheme.bodyLarge?.copyWith(
              color: theme.colorScheme.onSurfaceVariant,
            ),
          ),
        ],
      ],
    );
  }
}

class _IntroStep extends StatelessWidget {
  const _IntroStep({required this.strings});

  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    return _StepHeading(
      icon: LucideIcons.sparkles,
      title: strings.get('welcome_title'),
      body: strings.get('unofficial'),
    );
  }
}

class _PermissionsStep extends StatelessWidget {
  const _PermissionsStep({required this.strings});

  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        _StepHeading(
          icon: LucideIcons.shield,
          title: strings.get('permissions_title'),
          body: strings.get('permissions_body'),
        ),
        const SizedBox(height: 22),
        WarningBanner(
          title: strings.get('heads_up'),
          message: strings.get('permission_note'),
        ),
      ],
    );
  }
}

class _TermsStep extends StatelessWidget {
  const _TermsStep({
    required this.strings,
    required this.accepted,
    required this.onChanged,
  });

  final AppStrings strings;
  final bool accepted;
  final ValueChanged<bool> onChanged;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        _StepHeading(
          icon: LucideIcons.fileText,
          title: strings.get('terms_title'),
          body: strings.get('terms_body'),
        ),
        const SizedBox(height: 18),
        Panel(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
          child: CheckboxListTile(
            contentPadding: const EdgeInsets.symmetric(horizontal: 8),
            controlAffinity: ListTileControlAffinity.leading,
            value: accepted,
            onChanged: (value) => onChanged(value ?? false),
            title: Text(strings.get('terms_accept')),
          ),
        ),
      ],
    );
  }
}

class _IdentityStep extends StatelessWidget {
  const _IdentityStep({
    required this.strings,
    required this.useLicense,
    required this.licenseVisible,
    required this.licenseController,
    required this.onMethodChanged,
    required this.onVisibilityChanged,
    required this.onLicenseChanged,
  });

  final AppStrings strings;
  final bool useLicense;
  final bool licenseVisible;
  final TextEditingController licenseController;
  final ValueChanged<bool> onMethodChanged;
  final VoidCallback onVisibilityChanged;
  final ValueChanged<String> onLicenseChanged;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        _StepHeading(
          icon: LucideIcons.keyRound,
          title: strings.get('identity_title'),
        ),
        const SizedBox(height: 22),
        SegmentedButton<bool>(
          segments: <ButtonSegment<bool>>[
            ButtonSegment<bool>(
              value: false,
              icon: const Icon(LucideIcons.userRoundPlus),
              label: Text(strings.get('register_new')),
            ),
            ButtonSegment<bool>(
              value: true,
              icon: const Icon(LucideIcons.badgePlus),
              label: Text(strings.get('use_license_key')),
            ),
          ],
          selected: <bool>{useLicense},
          onSelectionChanged: (selection) => onMethodChanged(selection.first),
          showSelectedIcon: false,
        ),
        AnimatedSize(
          duration: UsqueMotion.of(context, UsqueMotion.gentle),
          curve: UsqueMotion.emphasized,
          alignment: Alignment.topCenter,
          child: !useLicense
              ? const SizedBox(width: double.infinity)
              : Padding(
                  padding: const EdgeInsets.only(top: 20),
                  child: TextField(
                    controller: licenseController,
                    obscureText: !licenseVisible,
                    enableSuggestions: false,
                    autocorrect: false,
                    onChanged: onLicenseChanged,
                    decoration: InputDecoration(
                      labelText: strings.get('warp_license_key'),
                      suffixIcon: IconButton(
                        tooltip: strings.get(
                          licenseVisible ? 'hide_license' : 'show_license',
                        ),
                        onPressed: onVisibilityChanged,
                        icon: Icon(
                          licenseVisible ? LucideIcons.eyeOff : LucideIcons.eye,
                        ),
                      ),
                    ),
                  ),
                ),
        ),
      ],
    );
  }
}
