import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_motion.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../models/onboarding_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/controller_selector.dart';
import '../widgets/external_link.dart';
import '../widgets/zero_trust_enrollment_editor.dart';

class OnboardingScreen extends StatefulWidget {
  const OnboardingScreen({
    required this.controller,
    this.externalLinkLauncher,
    super.key,
  });

  final AppController controller;
  final Future<bool> Function(Uri)? externalLinkLauncher;

  @override
  State<OnboardingScreen> createState() => _OnboardingScreenState();
}

typedef _OnboardingView = ({
  bool busy,
  int step,
  bool terms,
  OnboardingPhase phase,
  bool operationPending,
  bool permissionBusy,
  OnboardingPermissionState? permissions,
  InitialIdentityState? identity,
  String? error,
  LocalePreference locale,
});

class _OnboardingScreenState extends State<OnboardingScreen>
    with WidgetsBindingObserver {
  static const int _stepCount = 4;

  final TextEditingController _licenseController = TextEditingController();
  final _zeroTrustKey = GlobalKey<ZeroTrustEnrollmentEditorState>();
  int _step = 0;

  /// Direction of the last step change, so a step slides in from the side the
  /// user came from.
  bool _forward = true;
  bool _savingTerms = false;
  IdentityProvisioningMethod _method = IdentityProvisioningMethod.register;
  bool _licenseVisible = false;
  bool _zeroTrustValid = false;

  AppStrings get strings => widget.controller.strings;

  @override
  void initState() {
    super.initState();
    _step = widget.controller.onboardingStep;
    if (_step > 1 &&
        widget.controller.requiresOnboardingPermissions &&
        widget.controller.onboardingPermissions?.vpnGranted != true) {
      _step = 1;
      unawaited(widget.controller.setOnboardingStep(_step));
    } else if (_step == 3 && !widget.controller.onboardingTermsAccepted) {
      _step = 2;
      unawaited(widget.controller.setOnboardingStep(_step));
    }
    WidgetsBinding.instance.addObserver(this);
    if (widget.controller.requiresOnboardingPermissions) {
      unawaited(widget.controller.refreshOnboardingPermissions());
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed &&
        widget.controller.requiresOnboardingPermissions) {
      unawaited(widget.controller.refreshOnboardingPermissions());
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _licenseController
      ..clear()
      ..dispose();
    super.dispose();
  }

  void _goTo(int step) {
    if (_step == _stepCount - 1 &&
        step != _stepCount - 1 &&
        _method == IdentityProvisioningMethod.zeroTrust) {
      final editor = _zeroTrustKey.currentState;
      if (editor != null) {
        unawaited(editor.clearSensitive());
      }
    }
    setState(() {
      _forward = step > _step;
      _step = step;
      if (step != _stepCount - 1) _zeroTrustValid = false;
    });
    unawaited(widget.controller.setOnboardingStep(step));
  }

  void _changeMethod(IdentityProvisioningMethod value) {
    if (value == _method ||
        widget.controller.busy ||
        widget.controller.onboardingOperationPending) {
      return;
    }
    _licenseController.clear();
    if (_method == IdentityProvisioningMethod.zeroTrust) {
      final editor = _zeroTrustKey.currentState;
      if (editor != null) {
        unawaited(editor.clearSensitive());
      }
    }
    setState(() {
      _method = value;
      _licenseVisible = false;
      _zeroTrustValid = false;
    });
  }

  Future<void> _finishSetup({bool retryConfirmed = false}) async {
    if (widget.controller.busy ||
        widget.controller.onboardingOperationPending ||
        !widget.controller.onboardingTermsAccepted) {
      return;
    }
    if (widget.controller.onboardingPhase == OnboardingPhase.interrupted &&
        !retryConfirmed) {
      return;
    }
    String? licenseKey;
    ZeroTrustEnrollmentDraft? enrollment;
    final ready = widget.controller.onboardingPhase == OnboardingPhase.ready;
    if (!ready && _method == IdentityProvisioningMethod.registerWithLicense) {
      licenseKey = _licenseController.text.trim();
      if (licenseKey.isEmpty) return;
    } else if (!ready && _method == IdentityProvisioningMethod.zeroTrust) {
      enrollment = _zeroTrustKey.currentState?.validateAndRead();
      if (enrollment == null) {
        if (mounted) setState(() => _zeroTrustValid = false);
        return;
      }
    }

    final success = await widget.controller.finishOnboarding(
      method: _method,
      licenseKey: licenseKey,
      teamName: enrollment?.teamName,
      callbackUri: enrollment?.callbackUri,
    );
    licenseKey = null;
    enrollment = null;
    if (!mounted) return;
    _licenseController.clear();
    final editor = _zeroTrustKey.currentState;
    if (editor != null) {
      await editor.clearSensitive();
    }
    if (mounted && !success) {
      setState(() => _zeroTrustValid = false);
    }
  }

  Future<void> _setTermsAccepted(bool accepted) async {
    if (_savingTerms) return;
    setState(() => _savingTerms = true);
    try {
      await widget.controller.setOnboardingTermsAccepted(accepted);
    } finally {
      if (mounted) setState(() => _savingTerms = false);
    }
  }

  bool get _needsPermissions {
    if (!widget.controller.requiresOnboardingPermissions) return false;
    final permissions = widget.controller.onboardingPermissions;
    return permissions == null ||
        !permissions.vpnGranted ||
        permissions.notification ==
            OnboardingNotificationPermission.notRequested;
  }

  Future<void> _continue() async {
    if (_step == 1 && _needsPermissions) {
      if (!await widget.controller.prepareOnboardingPermissions() || !mounted) {
        return;
      }
    }
    if (!mounted) return;
    if (_step < _stepCount - 1) {
      _goTo(_step + 1);
    } else {
      await _finishSetup();
    }
  }

  Future<void> _retryRegistration() async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(strings.get('onboarding_retry_registration')),
        content: Text(strings.get('onboarding_result_unknown')),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(strings.get('onboarding_retry_registration')),
          ),
        ],
      ),
    );
    if (confirmed == true && mounted) {
      await _finishSetup(retryConfirmed: true);
    }
  }

  @override
  Widget build(BuildContext context) {
    return ControllerSelector<_OnboardingView>(
      controller: widget.controller,
      selector: (controller) => (
        busy: controller.busy,
        step: controller.onboardingStep,
        terms: controller.onboardingTermsAccepted,
        phase: controller.onboardingPhase,
        operationPending: controller.onboardingOperationPending,
        permissionBusy: controller.onboardingPermissionsBusy,
        permissions: controller.onboardingPermissions,
        identity: controller.initialIdentityState,
        error: controller.lastError,
        locale: controller.localePreference,
      ),
      builder: (context, view) {
        var step = view.step;
        if (step > 1 &&
            widget.controller.requiresOnboardingPermissions &&
            view.permissions?.vpnGranted != true) {
          step = 1;
        } else if (step == 3 && !view.terms) {
          step = 2;
        }
        if (step != _step) {
          _forward = step > _step;
          _step = step;
          _zeroTrustValid = false;
        }
        if (step != view.step) {
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (mounted && widget.controller.onboardingStep == view.step) {
              unawaited(widget.controller.setOnboardingStep(step));
            }
          });
        }
        return _buildScreen(context);
      },
    );
  }

  Widget _buildScreen(BuildContext context) {
    return Scaffold(
      backgroundColor: UsqueTokens.of(context).canvas,
      body: SafeArea(
        child: LayoutBuilder(
          builder: (context, constraints) {
            final wide =
                constraints.maxWidth >= 820 &&
                constraints.maxHeight >= 600 &&
                MediaQuery.textScalerOf(context).scale(16) <= 24;
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
                            _OnboardingAnimatedSize(
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
      1 => _PermissionsStep(strings: strings, controller: widget.controller),
      2 => _TermsStep(
        strings: strings,
        accepted: widget.controller.onboardingTermsAccepted,
        enabled: !_savingTerms,
        onChanged: (value) => unawaited(_setTermsAccepted(value)),
        externalLinkLauncher: widget.externalLinkLauncher,
      ),
      _ => _IdentityStep(
        strings: strings,
        controller: widget.controller,
        enabled:
            !widget.controller.busy &&
            !widget.controller.onboardingOperationPending,
        method: _method,
        licenseVisible: _licenseVisible,
        licenseController: _licenseController,
        zeroTrustKey: _zeroTrustKey,
        onMethodChanged: _changeMethod,
        onVisibilityChanged: () =>
            setState(() => _licenseVisible = !_licenseVisible),
        onLicenseChanged: (_) => setState(() {}),
        onZeroTrustValidityChanged: (valid) {
          if (mounted && valid != _zeroTrustValid) {
            setState(() => _zeroTrustValid = valid);
          }
        },
        onZeroTrustSubmitted: _finishSetup,
      ),
    };
  }

  Widget _buildActions(BuildContext context) {
    final isLast = _step == _stepCount - 1;
    final phase = widget.controller.onboardingPhase;
    final checking =
        isLast &&
        (phase == OnboardingPhase.interrupted ||
            phase == OnboardingPhase.unavailable ||
            phase == OnboardingPhase.reconciling ||
            (phase == OnboardingPhase.failed &&
                widget
                        .controller
                        .initialIdentityState
                        ?.operationId
                        .isNotEmpty ==
                    true));
    final waiting =
        widget.controller.busy ||
        widget.controller.onboardingPermissionsBusy ||
        _savingTerms;
    final blocked = waiting || widget.controller.onboardingOperationPending;
    final primaryBlocked = checking ? waiting : blocked;
    final draftValid = switch (_method) {
      IdentityProvisioningMethod.register => true,
      IdentityProvisioningMethod.registerWithLicense =>
        _licenseController.text.trim().isNotEmpty,
      IdentityProvisioningMethod.zeroTrust => _zeroTrustValid,
    };
    final canContinue = switch (_step) {
      2 => widget.controller.onboardingTermsAccepted,
      3 =>
        widget.controller.onboardingTermsAccepted &&
            (checking || phase == OnboardingPhase.ready || draftValid),
      _ => true,
    };
    final label = isLast
        ? checking
              ? 'onboarding_check_result'
              : phase == OnboardingPhase.ready
              ? 'onboarding_continue_existing'
              : 'finish_setup'
        : _step == 1 && _needsPermissions
        ? 'onboarding_grant_continue'
        : 'continue';
    return OverflowBar(
      alignment: MainAxisAlignment.spaceBetween,
      overflowAlignment: OverflowBarAlignment.end,
      spacing: 12,
      overflowSpacing: 12,
      children: <Widget>[
        if (_step > 0)
          OutlinedButton.icon(
            onPressed: blocked ? null : () => _goTo(_step - 1),
            icon: const Icon(LucideIcons.arrowLeft),
            label: Text(strings.get('back')),
          ),
        FilledButton.icon(
          onPressed: !canContinue || primaryBlocked
              ? null
              : checking
              ? () => widget.controller.resumeInitialIdentityState()
              : _continue,
          icon: primaryBlocked
              ? SizedBox(
                  width: 18,
                  height: 18,
                  child: CircularProgressIndicator(
                    strokeWidth: 2,
                    color: Theme.of(context).colorScheme.onPrimary,
                  ),
                )
              : Icon(
                  checking
                      ? LucideIcons.refreshCw
                      : isLast
                      ? LucideIcons.shieldCheck
                      : LucideIcons.arrowRight,
                ),
          label: Text(strings.get(label)),
        ),
        if (isLast &&
            (phase == OnboardingPhase.interrupted ||
                (phase == OnboardingPhase.failed &&
                    widget
                            .controller
                            .initialIdentityState
                            ?.operationId
                            .isNotEmpty ==
                        true)))
          TextButton(
            onPressed:
                blocked ||
                    !draftValid ||
                    !widget.controller.onboardingTermsAccepted
                ? null
                : _retryRegistration,
            child: Text(strings.get('onboarding_retry_registration')),
          ),
      ],
    );
  }
}

class _OnboardingAnimatedSize extends StatelessWidget {
  const _OnboardingAnimatedSize({required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) => UsqueMotion.reduced(context)
      ? child
      : AnimatedSize(
          duration: UsqueMotion.gentle,
          curve: UsqueMotion.emphasized,
          alignment: Alignment.topCenter,
          child: child,
        );
}

/// Animates the incoming step while keeping one live form in the tree.
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
    return TweenAnimationBuilder<double>(
      key: key,
      tween: Tween<double>(begin: 0, end: 1),
      duration: UsqueMotion.of(context, UsqueMotion.gentle),
      curve: UsqueMotion.emphasized,
      builder: (context, value, child) => Opacity(
        opacity: value,
        child: FractionalTranslation(
          translation: Offset(travel * (1 - value), 0),
          child: child,
        ),
      ),
      child: child,
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
          LayoutBuilder(
            builder: (context, constraints) => SingleChildScrollView(
              child: ConstrainedBox(
                constraints: BoxConstraints(minHeight: constraints.maxHeight),
                child: IntrinsicHeight(
                  child: Padding(
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
                ),
              ),
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
                  margin: EdgeInsetsDirectional.only(
                    end: index == total - 1 ? 0 : 6,
                  ),
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
            textDirection: TextDirection.ltr,
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
        Icon(icon, color: theme.colorScheme.primary, size: 28),
        const SizedBox(height: 24),
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
  const _PermissionsStep({required this.strings, required this.controller});

  final AppStrings strings;
  final AppController controller;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        _StepHeading(
          icon: LucideIcons.shield,
          title: strings.get('permissions_title'),
          body: strings.get(
            controller.requiresOnboardingPermissions
                ? 'onboarding_android_permissions_body'
                : 'permissions_body',
          ),
        ),
        const SizedBox(height: 22),
        if (controller.requiresOnboardingPermissions) ...[
          InlineStatus(
            label: strings.get(
              controller.onboardingPermissions?.vpnGranted == true
                  ? 'onboarding_vpn_granted'
                  : 'onboarding_vpn_required',
            ),
            tone: controller.onboardingPermissions?.vpnGranted == true
                ? StatusTone.success
                : StatusTone.warning,
          ),
          const SizedBox(height: 12),
          InlineStatus(
            label: strings.get(
              switch (controller.onboardingPermissions?.notification) {
                OnboardingNotificationPermission.granted =>
                  'onboarding_notifications_granted',
                OnboardingNotificationPermission.notGranted =>
                  'onboarding_notifications_denied',
                _ => 'onboarding_notifications_optional',
              },
            ),
            tone: StatusTone.neutral,
            showIndicator: false,
          ),
        ] else
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
    required this.enabled,
    required this.onChanged,
    this.externalLinkLauncher,
  });

  final AppStrings strings;
  final bool accepted;
  final bool enabled;
  final ValueChanged<bool> onChanged;
  final Future<bool> Function(Uri)? externalLinkLauncher;

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
        Wrap(
          spacing: 8,
          runSpacing: 4,
          children: [
            for (final link in const [
              (
                'onboarding_application_terms',
                'https://www.cloudflare.com/application/terms/',
              ),
              (
                'onboarding_personal_privacy',
                'https://www.cloudflare.com/application/privacypolicy/',
              ),
              (
                'onboarding_zero_trust_privacy',
                'https://www.cloudflare.com/privacypolicy/',
              ),
            ])
              TextButton.icon(
                onPressed: () => openExternalLink(
                  context,
                  strings,
                  link.$2,
                  launcher: externalLinkLauncher,
                ),
                icon: const Icon(LucideIcons.externalLink),
                label: Text(strings.get(link.$1)),
              ),
          ],
        ),
        const SizedBox(height: 12),
        ContentSection(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
          child: CheckboxListTile(
            contentPadding: const EdgeInsets.symmetric(horizontal: 8),
            controlAffinity: ListTileControlAffinity.leading,
            value: accepted,
            onChanged: enabled ? (value) => onChanged(value ?? false) : null,
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
    required this.controller,
    required this.enabled,
    required this.method,
    required this.licenseVisible,
    required this.licenseController,
    required this.zeroTrustKey,
    required this.onMethodChanged,
    required this.onVisibilityChanged,
    required this.onLicenseChanged,
    required this.onZeroTrustValidityChanged,
    required this.onZeroTrustSubmitted,
  });

  final AppStrings strings;
  final AppController controller;
  final bool enabled;
  final IdentityProvisioningMethod method;
  final bool licenseVisible;
  final TextEditingController licenseController;
  final GlobalKey<ZeroTrustEnrollmentEditorState> zeroTrustKey;
  final ValueChanged<IdentityProvisioningMethod> onMethodChanged;
  final VoidCallback onVisibilityChanged;
  final ValueChanged<String> onLicenseChanged;
  final ValueChanged<bool> onZeroTrustValidityChanged;
  final VoidCallback onZeroTrustSubmitted;

  @override
  Widget build(BuildContext context) {
    final phase = controller.onboardingPhase;
    final statusKey = switch (phase) {
      OnboardingPhase.idle => null,
      OnboardingPhase.submitting => 'onboarding_submitting',
      OnboardingPhase.reconciling => 'onboarding_reconciling',
      OnboardingPhase.ready => 'onboarding_ready',
      OnboardingPhase.interrupted => 'onboarding_interrupted',
      OnboardingPhase.failed => 'onboarding_failed',
      OnboardingPhase.unavailable => 'onboarding_unavailable',
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        _StepHeading(
          icon: LucideIcons.keyRound,
          title: strings.get('configure_identity'),
        ),
        const SizedBox(height: 22),
        if (statusKey != null) ...[
          Semantics(
            liveRegion: true,
            child: InlineStatus(
              label: strings.get(statusKey),
              tone: phase == OnboardingPhase.ready
                  ? StatusTone.success
                  : phase == OnboardingPhase.failed ||
                        phase == OnboardingPhase.interrupted
                  ? StatusTone.warning
                  : StatusTone.neutral,
            ),
          ),
          const SizedBox(height: 18),
        ],
        if (phase != OnboardingPhase.ready) ...[
          IdentityProvisioningMethodSelector(
            strings: strings,
            value: method,
            enabled: enabled,
            onChanged: onMethodChanged,
          ),
          _OnboardingAnimatedSize(
            child: switch (method) {
              IdentityProvisioningMethod.register => const SizedBox(
                width: double.infinity,
              ),
              IdentityProvisioningMethod.registerWithLicense => Padding(
                padding: const EdgeInsets.only(top: 20),
                child: TextField(
                  controller: licenseController,
                  enabled: enabled,
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
                      onPressed: enabled ? onVisibilityChanged : null,
                      icon: Icon(
                        licenseVisible ? LucideIcons.eyeOff : LucideIcons.eye,
                      ),
                    ),
                  ),
                ),
              ),
              IdentityProvisioningMethod.zeroTrust => Padding(
                padding: const EdgeInsets.only(top: 12),
                child: ZeroTrustEnrollmentEditor(
                  key: zeroTrustKey,
                  controller: controller,
                  enabled: enabled,
                  onValidityChanged: onZeroTrustValidityChanged,
                  onSubmitted: onZeroTrustSubmitted,
                ),
              ),
            },
          ),
        ],
      ],
    );
  }
}
