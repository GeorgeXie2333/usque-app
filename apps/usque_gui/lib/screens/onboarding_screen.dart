import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_theme.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';

class OnboardingScreen extends StatefulWidget {
  const OnboardingScreen({required this.controller, super.key});

  final AppController controller;

  @override
  State<OnboardingScreen> createState() => _OnboardingScreenState();
}

class _OnboardingScreenState extends State<OnboardingScreen> {
  final TextEditingController _secretController = TextEditingController();
  int _step = 0;
  bool _termsAccepted = false;
  bool _manualSecret = false;
  bool _secretVisible = false;

  AppStrings get strings => widget.controller.strings;

  @override
  void dispose() {
    _secretController
      ..clear()
      ..dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
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
                                    width: 46,
                                    height: 46,
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
                              const SizedBox(height: 36),
                            ],
                            _StepIndicator(step: _step, strings: strings),
                            const SizedBox(height: 32),
                            AnimatedSwitcher(
                              duration: const Duration(milliseconds: 220),
                              child: KeyedSubtree(
                                key: ValueKey<int>(_step),
                                child: _buildStep(context),
                              ),
                            ),
                            if (widget.controller.lastError !=
                                null) ...<Widget>[
                              const SizedBox(height: 20),
                              WarningBanner(
                                title: strings.get('setup_failed'),
                                message: widget.controller.lastError!,
                                danger: true,
                                onDismiss: widget.controller.clearError,
                              ),
                            ],
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
        manualSecret: _manualSecret,
        secretVisible: _secretVisible,
        secretController: _secretController,
        onMethodChanged: (value) => setState(() => _manualSecret = value),
        onVisibilityChanged: () =>
            setState(() => _secretVisible = !_secretVisible),
      ),
    };
  }

  Widget _buildActions(BuildContext context) {
    final isLast = _step == 3;
    final canContinue = switch (_step) {
      2 => _termsAccepted,
      3 => !_manualSecret || _secretController.text.trim().isNotEmpty,
      _ => true,
    };
    return Row(
      children: <Widget>[
        if (_step > 0)
          OutlinedButton.icon(
            onPressed: widget.controller.busy
                ? null
                : () => setState(() => _step--),
            icon: const Icon(LucideIcons.arrowLeft),
            label: Text(strings.get('back')),
          ),
        const Spacer(),
        FilledButton.icon(
          onPressed: !canContinue || widget.controller.busy
              ? null
              : () async {
                  if (!isLast) {
                    setState(() => _step++);
                    return;
                  }
                  final value = _manualSecret
                      ? _secretController.text.trim()
                      : null;
                  await widget.controller.finishOnboarding(warpSecret: value);
                  _secretController.clear();
                },
          icon: widget.controller.busy
              ? const SizedBox(
                  width: 18,
                  height: 18,
                  child: CircularProgressIndicator(
                    strokeWidth: 2,
                    color: Colors.white,
                  ),
                )
              : Icon(isLast ? LucideIcons.shieldCheck : LucideIcons.arrowRight),
          label: Text(strings.get(isLast ? 'finish_setup' : 'continue')),
        ),
      ],
    );
  }
}

class _BrandPane extends StatelessWidget {
  const _BrandPane({required this.strings});

  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    return ColoredBox(
      color: Theme.of(context).brightness == Brightness.dark
          ? const Color(0xFF23160E)
          : const Color(0xFFFFF0E3),
      child: Stack(
        fit: StackFit.expand,
        children: <Widget>[
          Positioned(
            top: -140,
            left: -110,
            child: Container(
              width: 420,
              height: 420,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                color: UsqueColors.orange.withValues(alpha: 0.13),
              ),
            ),
          ),
          Positioned(
            right: -100,
            bottom: -180,
            child: Container(
              width: 460,
              height: 460,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                color: UsqueColors.orange.withValues(alpha: 0.09),
              ),
            ),
          ),
          Padding(
            padding: const EdgeInsets.all(56),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Image.asset(
                  'assets/branding/usque-ui-icon.png',
                  width: 84,
                  height: 84,
                ),
                const Spacer(),
                Text(
                  'Usque',
                  style: Theme.of(context).textTheme.displaySmall?.copyWith(
                    fontSize: 48,
                    color: Theme.of(context).colorScheme.onSurface,
                  ),
                ),
                const SizedBox(height: 12),
                Text(
                  strings.get('onboarding_hero'),
                  style: Theme.of(context).textTheme.titleLarge?.copyWith(
                    height: 1.4,
                    color: Theme.of(context).colorScheme.onSurfaceVariant,
                  ),
                ),
                const SizedBox(height: 32),
                _Feature(
                  icon: LucideIcons.gauge,
                  label: strings.get('native_ui'),
                ),
                const SizedBox(height: 15),
                _Feature(
                  icon: LucideIcons.shieldCheck,
                  label: strings.get('leak_resistant'),
                ),
                const SizedBox(height: 15),
                _Feature(
                  icon: LucideIcons.database,
                  label: strings.get('no_telemetry'),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _Feature extends StatelessWidget {
  const _Feature({required this.icon, required this.label});

  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: <Widget>[
        Icon(icon, size: 20, color: UsqueColors.orange),
        const SizedBox(width: 11),
        Text(label, style: Theme.of(context).textTheme.titleMedium),
      ],
    );
  }
}

class _StepIndicator extends StatelessWidget {
  const _StepIndicator({required this.step, required this.strings});

  final int step;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      label: strings
          .get('setup_progress')
          .replaceAll('{current}', '${step + 1}')
          .replaceAll('{total}', '4'),
      child: Row(
        children: List<Widget>.generate(4, (index) {
          final active = index <= step;
          return Expanded(
            child: AnimatedContainer(
              duration: const Duration(milliseconds: 180),
              height: 4,
              margin: EdgeInsets.only(right: index == 3 ? 0 : 8),
              decoration: BoxDecoration(
                color: active
                    ? Theme.of(context).colorScheme.secondary
                    : Theme.of(context).colorScheme.surfaceContainerHighest,
                borderRadius: BorderRadius.circular(4),
              ),
            ),
          );
        }),
      ),
    );
  }
}

class _StepHeading extends StatelessWidget {
  const _StepHeading({
    required this.icon,
    required this.title,
    required this.body,
  });

  final IconData icon;
  final String title;
  final String body;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        DecoratedBox(
          decoration: BoxDecoration(
            color: Theme.of(context).colorScheme.secondaryContainer,
            borderRadius: BorderRadius.circular(18),
          ),
          child: Padding(
            padding: const EdgeInsets.all(15),
            child: Icon(
              icon,
              color: Theme.of(context).colorScheme.onSecondaryContainer,
              size: 28,
            ),
          ),
        ),
        const SizedBox(height: 24),
        Text(title, style: Theme.of(context).textTheme.headlineMedium),
        const SizedBox(height: 12),
        Text(
          body,
          style: Theme.of(context).textTheme.bodyLarge?.copyWith(
            color: Theme.of(context).colorScheme.onSurfaceVariant,
          ),
        ),
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
      body: strings.get('welcome_body'),
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
          title: strings.get('permissions_title'),
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
        const SizedBox(height: 24),
        CheckboxListTile(
          contentPadding: EdgeInsets.zero,
          controlAffinity: ListTileControlAffinity.leading,
          value: accepted,
          onChanged: (value) => onChanged(value ?? false),
          title: Text(strings.get('terms_accept')),
        ),
      ],
    );
  }
}

class _IdentityStep extends StatelessWidget {
  const _IdentityStep({
    required this.strings,
    required this.manualSecret,
    required this.secretVisible,
    required this.secretController,
    required this.onMethodChanged,
    required this.onVisibilityChanged,
  });

  final AppStrings strings;
  final bool manualSecret;
  final bool secretVisible;
  final TextEditingController secretController;
  final ValueChanged<bool> onMethodChanged;
  final VoidCallback onVisibilityChanged;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        _StepHeading(
          icon: LucideIcons.keyRound,
          title: strings.get('identity_title'),
          body: strings.get('identity_body'),
        ),
        const SizedBox(height: 24),
        SegmentedButton<bool>(
          segments: <ButtonSegment<bool>>[
            ButtonSegment<bool>(
              value: false,
              icon: const Icon(LucideIcons.userRoundPlus),
              label: Text(strings.get('register_new')),
            ),
            ButtonSegment<bool>(
              value: true,
              icon: const Icon(LucideIcons.keyRound),
              label: Text(strings.get('manual_secret')),
            ),
          ],
          selected: <bool>{manualSecret},
          onSelectionChanged: (selection) => onMethodChanged(selection.first),
          showSelectedIcon: false,
        ),
        if (manualSecret) ...<Widget>[
          const SizedBox(height: 20),
          TextField(
            controller: secretController,
            obscureText: !secretVisible,
            enableSuggestions: false,
            autocorrect: false,
            onChanged: (_) => (context as Element).markNeedsBuild(),
            decoration: InputDecoration(
              labelText: strings.get('warp_secret'),
              helperText: strings.get('secret_help'),
              helperMaxLines: 3,
              suffixIcon: IconButton(
                tooltip: strings.get(
                  secretVisible ? 'hide_secret' : 'show_secret',
                ),
                onPressed: onVisibilityChanged,
                icon: Icon(
                  secretVisible ? LucideIcons.eyeOff : LucideIcons.eye,
                ),
              ),
            ),
          ),
        ],
      ],
    );
  }
}
