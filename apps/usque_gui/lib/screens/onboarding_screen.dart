import 'dart:async';
import 'dart:math' as math;
import 'dart:ui' show lerpDouble;

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/legal_links.dart';
import '../core/usque_motion.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../models/onboarding_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/controller_selector.dart';
import '../widgets/external_link.dart';
import '../widgets/linux_service_section.dart';
import '../widgets/setup_handoff.dart';
import '../widgets/usque_logo.dart';
import '../widgets/zero_trust_enrollment_editor.dart';
import 'shell_screen.dart';

class OnboardingScreen extends StatefulWidget {
  const OnboardingScreen({
    required this.controller,
    this.externalLinkLauncher,
    this.exit,
    super.key,
  });

  final AppController controller;
  final Future<bool> Function(Uri)? externalLinkLauncher;

  /// Runs from 0 to 1 while completed setup hands over to the shell; see
  /// [SetupHandoff]. Setup holds its last frame meanwhile, so the account's
  /// new state never flashes on the way out.
  final Animation<double>? exit;

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

  late final ProxyAnimation _exit = ProxyAnimation(
    widget.exit ?? kAlwaysDismissedAnimation,
  );

  /// The last laid-out body and the layout it was built for, replayed while
  /// [OnboardingScreen.exit] runs.
  (_OnboardingLayout, Widget)? _body;

  AppStrings get strings => widget.controller.strings;

  @override
  void initState() {
    super.initState();
    _step = widget.controller.onboardingStep;
    if (_step == 3 && !widget.controller.onboardingTermsAccepted) {
      _step = 2;
      unawaited(widget.controller.setOnboardingStep(_step));
    }
    WidgetsBinding.instance.addObserver(this);
    if (widget.controller.requiresOnboardingPermissions) {
      unawaited(widget.controller.refreshOnboardingPermissions());
    }
  }

  @override
  void didUpdateWidget(OnboardingScreen oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.exit != oldWidget.exit) {
      _exit.parent = widget.exit ?? kAlwaysDismissedAnimation;
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
        if (step == 3 && !view.terms) {
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
      // The canvas is its own layer so it can clear during the hand-off.
      backgroundColor: Colors.transparent,
      body: Stack(
        fit: StackFit.expand,
        children: <Widget>[
          _ExitFade(
            animation: _exit,
            interval: SetupHandoffTiming.canvas,
            child: ColoredBox(color: UsqueTokens.of(context).canvas),
          ),
          SafeArea(child: LayoutBuilder(builder: _layoutBody)),
        ],
      ),
    );
  }

  Widget _layoutBody(BuildContext context, BoxConstraints constraints) {
    final comfortableText = MediaQuery.textScalerOf(context).scale(16) <= 24;
    final _OnboardingLayout layout = (
      size: constraints.biggest,
      wide:
          constraints.maxWidth >= 820 &&
          constraints.maxHeight >= 600 &&
          comfortableText,
      // Short windows and large text scroll the actions with the step.
      pinned: constraints.maxHeight >= 560 && comfortableText,
    );
    if (widget.exit != null && _body?.$1 == layout) return _body!.$2;
    final form = _buildForm(context, layout);
    final Widget body;
    if (layout.wide) {
      final paneWidth = constraints.maxWidth * 0.4;
      body = Stack(
        children: <Widget>[
          PositionedDirectional(
            start: 0,
            top: 0,
            bottom: 0,
            child: _BrandPane(
              strings: strings,
              width: paneWidth,
              railWidth: ShellScreen.railExtent(constraints.maxWidth),
              progress: (_step + 1) / (_stepCount + 1),
              exit: _exit,
            ),
          ),
          PositionedDirectional(
            start: paneWidth,
            top: 0,
            end: 0,
            bottom: 0,
            child: form,
          ),
        ],
      );
    } else {
      body = form;
    }
    if (widget.exit == null) _body = (layout, body);
    return body;
  }

  Widget _buildForm(BuildContext context, _OnboardingLayout layout) {
    final wide = layout.wide;
    final double gutter = wide ? 64 : 24;
    final EdgeInsets padding = EdgeInsets.fromLTRB(
      gutter,
      wide ? (layout.size.height * 0.1).clamp(32, 96) : 24,
      gutter,
      // The wide footer ends on the brand pane's baseline.
      wide ? 48 : 24,
    );
    final theme = Theme.of(context);
    Widget column(Widget child) => Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 620),
        child: child,
      ),
    );
    final footer = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        _buildActions(context),
        const SizedBox(height: 24),
        Text(
          strings.get('trademark_attribution'),
          style: theme.textTheme.bodySmall?.copyWith(
            color: theme.colorScheme.onSurfaceVariant,
            height: 1.5,
          ),
        ),
      ],
    );
    final content = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        _ExitFade(
          animation: _exit,
          interval: SetupHandoffTiming.form,
          lift: 12,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: <Widget>[
              if (!wide) ...<Widget>[
                Row(
                  children: <Widget>[
                    const UsqueLogo(size: 40),
                    const SizedBox(width: 12),
                    Text('Usque', style: theme.textTheme.titleLarge),
                  ],
                ),
                const SizedBox(height: 32),
              ],
              _StepIndicator(
                step: _step,
                total: _stepCount,
                next: switch (_step) {
                  0 => strings.get('permissions_title'),
                  1 => strings.get('terms_title'),
                  2 => strings.get('configure_identity'),
                  _ => null,
                },
                strings: strings,
              ),
              const SizedBox(height: 30),
              _OnboardingAnimatedSize(
                child: _StepTransition(
                  step: _step,
                  forward: _forward,
                  child: _buildStep(context, wide: wide),
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
              if (!layout.pinned) ...<Widget>[
                const SizedBox(height: 28),
                footer,
              ],
            ],
          ),
        ),
      ],
    );
    if (!layout.pinned) {
      return SingleChildScrollView(padding: padding, child: column(content));
    }
    return _PinnedFooterLayout(
      padding: padding,
      body: column(content),
      footer: _ExitFade(
        animation: _exit,
        interval: SetupHandoffTiming.form,
        child: column(footer),
      ),
    );
  }

  Widget _buildStep(BuildContext context, {required bool wide}) {
    return switch (_step) {
      // The wide layout's brand pane already carries the non-affiliation
      // notice on every step; narrow layouts show it here instead.
      0 => _IntroStep(strings: strings, showNotice: !wide),
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
      // The primary action keeps the end edge on every step, including the
      // first one, which has nothing to go back to.
      alignment: _step > 0
          ? MainAxisAlignment.spaceBetween
          : MainAxisAlignment.end,
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

/// Setup's layout decisions for one set of constraints.
typedef _OnboardingLayout = ({Size size, bool wide, bool pinned});

/// Clears [child] away over [interval] of the setup hand-off, optionally
/// lifting it by [lift] pixels. At rest it draws [child] untouched.
class _ExitFade extends StatelessWidget {
  const _ExitFade({
    required this.animation,
    required this.interval,
    required this.child,
    this.lift = 0,
  });

  final Animation<double> animation;
  final Curve interval;
  final double lift;
  final Widget child;

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: animation,
    builder: (context, child) {
      final t = interval.transform(animation.value);
      return Opacity(
        opacity: 1 - t,
        child: Transform.translate(offset: Offset(0, -lift * t), child: child),
      );
    },
    child: child,
  );
}

/// Scrolls the step above actions that stay at the bottom edge, so the
/// primary action never moves between steps. A hairline marks the edge only
/// while the step continues below it.
class _PinnedFooterLayout extends StatefulWidget {
  const _PinnedFooterLayout({
    required this.padding,
    required this.body,
    required this.footer,
  });

  final EdgeInsets padding;
  final Widget body;
  final Widget footer;

  @override
  State<_PinnedFooterLayout> createState() => _PinnedFooterLayoutState();
}

class _PinnedFooterLayoutState extends State<_PinnedFooterLayout> {
  bool _continues = false;

  bool _onScroll(Notification notification) {
    final ScrollMetrics? metrics = switch (notification) {
      ScrollMetricsNotification(:final metrics, depth: 0) => metrics,
      ScrollNotification(:final metrics, depth: 0) => metrics,
      _ => null,
    };
    if (metrics != null) {
      final continues = metrics.extentAfter > 0.5;
      if (continues != _continues) setState(() => _continues = continues);
    }
    return false;
  }

  @override
  Widget build(BuildContext context) {
    final padding = widget.padding;
    return Column(
      children: <Widget>[
        Expanded(
          child: NotificationListener<Notification>(
            onNotification: _onScroll,
            child: SingleChildScrollView(
              padding: padding.copyWith(bottom: UsqueSpace.l),
              child: widget.body,
            ),
          ),
        ),
        Padding(
          padding: EdgeInsets.symmetric(horizontal: padding.left),
          child: Align(
            alignment: Alignment.topCenter,
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 620),
              child: Opacity(
                opacity: _continues ? 1 : 0,
                child: Divider(
                  height: 1,
                  color: UsqueTokens.of(context).hairline,
                ),
              ),
            ),
          ),
        ),
        Padding(
          padding: padding.copyWith(top: UsqueSpace.m),
          child: widget.footer,
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

/// Left half of the setup window: the mark, a ring that closes as setup
/// advances, and the one disclaimer every user should read before they
/// connect. On completion the pane folds into the shell's rail and its mark
/// lands on the rail's.
class _BrandPane extends StatelessWidget {
  const _BrandPane({
    required this.strings,
    required this.width,
    required this.railWidth,
    required this.progress,
    required this.exit,
  });

  final AppStrings strings;

  /// Width at rest; the content keeps this width while the pane folds.
  final double width;

  /// The shell rail this pane folds into.
  final double railWidth;

  /// Share of setup reached, from 0 to 1.
  final double progress;
  final Animation<double> exit;

  static const double _inset = 48;
  static const double _logoSize = 72;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final UsqueTokens tokens = UsqueTokens.of(context);
    final bool dark = theme.brightness == Brightness.dark;
    final bool rtl = Directionality.of(context) == TextDirection.rtl;
    const Rect target = ShellScreen.railBrandLogo;
    // Only the extended rail has a mark for this one to land on.
    final bool landsOnRail = railWidth >= target.right;

    final Widget surface = Stack(
      fit: StackFit.expand,
      children: <Widget>[
        DecoratedBox(
          decoration: BoxDecoration(
            gradient: LinearGradient(
              begin: Alignment.topLeft,
              end: Alignment.bottomRight,
              colors: dark
                  ? const <Color>[Color(0xFF1C120A), Color(0xFF121214)]
                  : const <Color>[Color(0xFFFFF3E8), Color(0xFFF7F5F0)],
            ),
          ),
        ),
        Positioned(
          right: -170,
          top: 90,
          width: 460,
          height: 460,
          child: _ExitFade(
            animation: exit,
            interval: SetupHandoffTiming.motif,
            child: RepaintBoundary(
              child: TweenAnimationBuilder<double>(
                tween: Tween<double>(end: progress),
                duration: UsqueMotion.of(context, UsqueMotion.gentle),
                curve: UsqueMotion.emphasized,
                builder: (context, reached, _) => AnimatedBuilder(
                  animation: exit,
                  builder: (context, _) => CustomPaint(
                    painter: _BrandMotifPainter(
                      progress: lerpDouble(
                        reached,
                        1,
                        SetupHandoffTiming.ringClose.transform(exit.value),
                      )!,
                      accent: tokens.brand,
                      track: tokens.hairlineStrong,
                    ),
                  ),
                ),
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
                  padding: const EdgeInsets.all(_inset),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: <Widget>[
                      // The mark is drawn above the pane, so it can outlast it.
                      const SizedBox(height: _logoSize),
                      const Spacer(),
                      _ExitFade(
                        animation: exit,
                        interval: SetupHandoffTiming.motif,
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: <Widget>[
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
                ),
              ),
            ),
          ),
        ),
      ],
    );

    return AnimatedBuilder(
      animation: exit,
      builder: (context, child) => SizedBox(
        width: lerpDouble(
          width,
          railWidth,
          SetupHandoffTiming.collapse.transform(exit.value),
        ),
        child: child,
      ),
      child: Stack(
        fit: StackFit.expand,
        children: <Widget>[
          _ExitFade(
            animation: exit,
            interval: SetupHandoffTiming.pane,
            child: DecoratedBox(
              position: DecorationPosition.foreground,
              decoration: BoxDecoration(
                border: BorderDirectional(
                  end: BorderSide(color: tokens.hairline),
                ),
              ),
              // The content keeps its width and is clipped as the pane
              // folds, rather than reflowing.
              child: ClipRect(
                child: OverflowBox(
                  alignment: AlignmentDirectional.topStart,
                  minWidth: width,
                  maxWidth: width,
                  child: surface,
                ),
              ),
            ),
          ),
          PositionedDirectional(
            start: _inset,
            top: _inset,
            child: AnimatedBuilder(
              animation: exit,
              builder: (context, child) {
                final double travel = landsOnRail
                    ? SetupHandoffTiming.collapse.transform(exit.value)
                    : 0;
                final double fade =
                    (landsOnRail
                            ? SetupHandoffTiming.mark
                            : SetupHandoffTiming.pane)
                        .transform(exit.value);
                return Opacity(
                  opacity: 1 - fade,
                  child: Transform.translate(
                    offset: Offset(
                      (rtl ? -1 : 1) * (target.left - _inset) * travel,
                      (target.top - _inset) * travel,
                    ),
                    child: Transform.scale(
                      scale: lerpDouble(1, target.width / _logoSize, travel),
                      alignment: AlignmentDirectional.topStart,
                      child: child,
                    ),
                  ),
                );
              },
              child: const UsqueLogo(size: _logoSize),
            ),
          ),
        ],
      ),
    );
  }
}

/// Concentric bezels, echoing the connection ring the user is about to meet.
/// The inner arc grows with [progress] and closes as setup completes.
class _BrandMotifPainter extends CustomPainter {
  const _BrandMotifPainter({
    required this.progress,
    required this.accent,
    required this.track,
  });

  final double progress;
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
    // Counter-clockwise from the top, so the arc grows across the side of
    // the ring that stays inside the pane.
    canvas.drawArc(
      Rect.fromCircle(center: center, radius: outer * 0.54),
      -math.pi / 2,
      -2 * math.pi * progress.clamp(0, 1),
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
      oldDelegate.progress != progress ||
      oldDelegate.accent != accent ||
      oldDelegate.track != track;
}

class _StepIndicator extends StatelessWidget {
  const _StepIndicator({
    required this.step,
    required this.total,
    required this.next,
    required this.strings,
  });

  final int step;
  final int total;

  /// Title of the step that follows, if any.
  final String? next;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final UsqueTokens tokens = UsqueTokens.of(context);
    final Color muted = theme.colorScheme.onSurfaceVariant;
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
          Row(
            crossAxisAlignment: CrossAxisAlignment.baseline,
            textBaseline: TextBaseline.alphabetic,
            children: <Widget>[
              Text(
                '${step + 1} / $total',
                textDirection: TextDirection.ltr,
                style: UsqueTheme.address(context, size: 12, color: muted),
              ),
              if (next case final next?) ...<Widget>[
                const SizedBox(width: 12),
                Flexible(
                  child: Text(
                    strings
                        .get('onboarding_up_next')
                        .replaceAll('{step}', next),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.labelMedium?.copyWith(color: muted),
                  ),
                ),
              ],
            ],
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
        // The tinted tile Home and Settings use for a lit feature.
        ExcludeSemantics(
          child: Container(
            width: 44,
            height: 44,
            decoration: BoxDecoration(
              color: theme.colorScheme.primary.withValues(
                alpha: UsqueTokens.of(context).tint,
              ),
              borderRadius: BorderRadius.circular(UsqueRadii.control),
            ),
            child: Icon(icon, color: theme.colorScheme.primary, size: 22),
          ),
        ),
        const SizedBox(height: 20),
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

/// Icons shared by each step's heading and the welcome outline.
const IconData _permissionsIcon = LucideIcons.shield;
const IconData _termsIcon = LucideIcons.fileText;
const IconData _identityIcon = LucideIcons.keyRound;

class _IntroStep extends StatelessWidget {
  const _IntroStep({required this.strings, required this.showNotice});

  final AppStrings strings;
  final bool showNotice;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final Color muted = theme.colorScheme.onSurfaceVariant;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        _StepHeading(
          icon: LucideIcons.sparkles,
          title: strings.get('welcome_title'),
          body: strings.get(showNotice ? 'unofficial' : 'welcome_body'),
        ),
        const SizedBox(height: 22),
        // An outline of what follows, in the order the steps run.
        ContentList(
          children: <Widget>[
            for (final (index, (icon, key)) in const <(IconData, String)>[
              (_permissionsIcon, 'permissions_title'),
              (_termsIcon, 'terms_title'),
              (_identityIcon, 'configure_identity'),
            ].indexed)
              Padding(
                padding: RowInsets.row,
                child: Row(
                  children: <Widget>[
                    ExcludeSemantics(child: Icon(icon, size: 20, color: muted)),
                    const SizedBox(width: 12),
                    Expanded(
                      child: Text(
                        strings.get(key),
                        style: theme.textTheme.titleMedium,
                      ),
                    ),
                    const SizedBox(width: 12),
                    ExcludeSemantics(
                      child: Text(
                        '${index + 2}',
                        textDirection: TextDirection.ltr,
                        style: UsqueTheme.address(
                          context,
                          size: 12,
                          color: muted,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
          ],
        ),
      ],
    );
  }
}

class _PermissionsStep extends StatelessWidget {
  const _PermissionsStep({required this.strings, required this.controller});

  final AppStrings strings;
  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final notification = controller.onboardingPermissions?.notification;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        _StepHeading(
          icon: _permissionsIcon,
          title: strings.get('permissions_title'),
          body: strings.get(
            controller.requiresOnboardingPermissions
                ? 'onboarding_android_permissions_body'
                : 'permissions_body',
          ),
        ),
        const SizedBox(height: 22),
        if (controller.supportsLinuxServiceMaintenance) ...<Widget>[
          LinuxServiceSection(controller: controller),
          const SizedBox(height: UsqueSpace.m),
        ],
        if (controller.requiresOnboardingPermissions) ...[
          Padding(
            padding: const EdgeInsets.symmetric(
              horizontal: RowInsets.horizontal,
            ),
            child: InlineStatus(
              label: strings.get(switch (notification) {
                OnboardingNotificationPermission.granted =>
                  'onboarding_notifications_granted',
                OnboardingNotificationPermission.notGranted =>
                  'onboarding_notifications_denied',
                _ => 'onboarding_notifications_optional',
              }),
              // Declining is a supported choice, not a warning.
              tone: notification == OnboardingNotificationPermission.granted
                  ? StatusTone.success
                  : StatusTone.neutral,
              icon: switch (notification) {
                OnboardingNotificationPermission.granted => null,
                OnboardingNotificationPermission.notGranted =>
                  LucideIcons.bellOff,
                _ => LucideIcons.bell,
              },
            ),
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
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        _StepHeading(
          icon: _termsIcon,
          title: strings.get('terms_title'),
          body: strings.get('terms_body'),
        ),
        const SizedBox(height: 18),
        // The same rows About uses for these documents.
        ContentList(
          children: <Widget>[
            for (final (index, (labelKey, url))
                in kCloudflareLegalLinks.indexed)
              LinkRow(
                key: ValueKey<String>('onboarding-$labelKey'),
                icon: const <IconData>[
                  LucideIcons.fileText,
                  LucideIcons.shieldCheck,
                  LucideIcons.building2,
                ][index],
                title: strings.get(labelKey),
                onTap: () => openExternalLink(
                  context,
                  strings,
                  url,
                  launcher: externalLinkLauncher,
                ),
              ),
          ],
        ),
        const SizedBox(height: 16),
        // The box and its label share the link rows' icon and text columns.
        ListTileTheme.merge(
          horizontalTitleGap: 0,
          child: CheckboxListTile(
            contentPadding: const EdgeInsetsDirectional.only(
              end: RowInsets.horizontal,
            ),
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
          icon: _identityIcon,
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
