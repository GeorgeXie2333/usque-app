import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/usque_motion.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/animated_index_stack.dart';
import '../widgets/controller_selector.dart';
import 'diagnostics_screen.dart';
import 'home_screen.dart';
import 'profiles_screen.dart';
import 'proxy_screen.dart';
import 'settings_screen.dart';

const double _railMinWidth = 78;
const double _railMinExtendedWidth = 232;

/// Matches [NavigationRailDestination.padding] so the brand sits on the
/// same vertical axis as the destination icons.
const EdgeInsets _destinationPadding = EdgeInsets.symmetric(
  vertical: 3,
  horizontal: 8,
);

class ShellScreen extends StatelessWidget {
  const ShellScreen({required this.controller, super.key});

  final AppController controller;

  static const List<IconData> _icons = <IconData>[
    LucideIcons.house,
    LucideIcons.layers3,
    LucideIcons.waypoints,
    LucideIcons.settings,
    LucideIcons.activity,
  ];

  /// Width at which the bottom bar gives way to the side rail.
  static const double _railBreakpoint = 760;

  /// Width at which the rail can afford to show labels beside the icons.
  static const double _extendedBreakpoint = 1050;

  KeyEventResult _handleNavigationKey(
    KeyEvent event, {
    required bool vertical,
  }) {
    if (event is! KeyDownEvent && event is! KeyRepeatEvent) {
      return KeyEventResult.ignored;
    }
    final previousKey = vertical
        ? LogicalKeyboardKey.arrowUp
        : LogicalKeyboardKey.arrowLeft;
    final nextKey = vertical
        ? LogicalKeyboardKey.arrowDown
        : LogicalKeyboardKey.arrowRight;
    final delta = switch (event.logicalKey) {
      final key when key == previousKey => -1,
      final key when key == nextKey => 1,
      _ => 0,
    };
    if (delta == 0) {
      return KeyEventResult.ignored;
    }
    final sections = AppSection.values;
    final next =
        (controller.section.index + delta + sections.length) % sections.length;
    controller.selectSection(sections[next]);
    return KeyEventResult.handled;
  }

  @override
  Widget build(BuildContext context) {
    return ControllerSelector<AppSection>(
      controller: controller,
      selector: (controller) => controller.section,
      builder: (context, section) {
        final strings = controller.strings;
        final labels = <String>[
          strings.get('home'),
          strings.get('profiles'),
          strings.get('proxy'),
          strings.get('settings'),
          strings.get('diagnostics'),
        ];
        final pages = <Widget>[
          // Home subscribes per block, so it takes the controller directly.
          HomeScreen(
            key: const ValueKey<String>('home-page'),
            controller: controller,
          ),
          ControllerSelector<
            ({
              List<UsqueProfile> profiles,
              String activeProfileId,
              Map<String, ProfileIdentityState> identityStates,
              Map<String, ProfileIdentityStatus> identityStatuses,
            })
          >(
            key: const ValueKey<String>('profiles-controller-selector'),
            controller: controller,
            active: (controller) => controller.section == AppSection.profiles,
            selector: (controller) => (
              profiles: controller.profiles,
              activeProfileId: controller.activeProfileId,
              identityStates: controller.profileIdentityStates,
              identityStatuses: controller.profileIdentityStatuses,
            ),
            builder: (context, _) => ProfilesScreen(controller: controller),
          ),
          ControllerSelector<UsqueProfile>(
            key: const ValueKey<String>('proxy-controller-selector'),
            controller: controller,
            active: (controller) => controller.section == AppSection.proxy,
            selector: (controller) => controller.activeProfile,
            builder: (context, _) => ProxyScreen(controller: controller),
          ),
          ControllerSelector<
            ({
              ThemePreference theme,
              LocalePreference locale,
              bool updateChecksEnabled,
              UpdateCheckResult? updateResult,
              bool busy,
              String? error,
              String? notice,
              UsqueProfile profile,
            })
          >(
            key: const ValueKey<String>('settings-controller-selector'),
            controller: controller,
            active: (controller) => controller.section == AppSection.settings,
            selector: (controller) => (
              theme: controller.themePreference,
              locale: controller.localePreference,
              updateChecksEnabled: controller.updateChecksEnabled,
              updateResult: controller.updateResult,
              busy: controller.busy,
              error: controller.lastError,
              notice: controller.lastNotice,
              profile: controller.activeProfile,
            ),
            builder: (context, _) => SettingsScreen(controller: controller),
          ),
          ControllerSelector<
            ({
              EngineSnapshot snapshot,
              ConnectionPhase phase,
              bool streamDegraded,
              bool busy,
              String? error,
              String? notice,
            })
          >(
            key: const ValueKey<String>('diagnostics-controller-selector'),
            controller: controller,
            active: (controller) =>
                controller.section == AppSection.diagnostics,
            selector: (controller) => (
              snapshot: controller.snapshot,
              phase: controller.snapshot.phase,
              streamDegraded: controller.snapshotStreamDegraded,
              busy: controller.busy,
              error: controller.lastError,
              notice: controller.lastNotice,
            ),
            builder: (context, _) => DiagnosticsScreen(controller: controller),
          ),
        ];
        final selected = section.index;

        return LayoutBuilder(
          builder: (context, constraints) {
            final useRail = constraints.maxWidth >= _railBreakpoint;
            final extended = constraints.maxWidth >= _extendedBreakpoint;
            return Scaffold(
              body: SafeArea(
                bottom: false,
                child: Row(
                  children: <Widget>[
                    if (useRail) ...<Widget>[
                      Focus(
                        canRequestFocus: false,
                        onKeyEvent: (_, event) =>
                            _handleNavigationKey(event, vertical: true),
                        child: NavigationRail(
                          extended: extended,
                          minWidth: _railMinWidth,
                          minExtendedWidth: _railMinExtendedWidth,
                          selectedIndex: selected,
                          onDestinationSelected: (index) => controller
                              .selectSection(AppSection.values[index]),
                          labelType: extended
                              ? NavigationRailLabelType.none
                              : NavigationRailLabelType.all,
                          leading: _RailLeading(
                            controller: controller,
                            extended: extended,
                          ),
                          destinations:
                              List<NavigationRailDestination>.generate(
                                labels.length,
                                (index) => NavigationRailDestination(
                                  icon: Icon(_icons[index]),
                                  selectedIcon: Icon(_icons[index]),
                                  label: Text(labels[index]),
                                  padding: _destinationPadding,
                                ),
                              ),
                        ),
                      ),
                      VerticalDivider(
                        width: 1,
                        thickness: 1,
                        color: UsqueTokens.of(context).hairline,
                      ),
                    ],
                    Expanded(
                      child: AnimatedIndexStack(
                        index: selected,
                        children: pages,
                      ),
                    ),
                  ],
                ),
              ),
              bottomNavigationBar: useRail
                  ? null
                  : DecoratedBox(
                      decoration: BoxDecoration(
                        border: Border(
                          top: BorderSide(
                            color: UsqueTokens.of(context).hairline,
                          ),
                        ),
                      ),
                      child: SafeArea(
                        top: false,
                        child: Focus(
                          canRequestFocus: false,
                          onKeyEvent: (_, event) =>
                              _handleNavigationKey(event, vertical: false),
                          child: NavigationBar(
                            selectedIndex: selected,
                            onDestinationSelected: (index) => controller
                                .selectSection(AppSection.values[index]),
                            destinations: List<NavigationDestination>.generate(
                              labels.length,
                              (index) => NavigationDestination(
                                icon: Icon(_icons[index]),
                                selectedIcon: Icon(_icons[index]),
                                label: labels[index],
                                tooltip: labels[index],
                              ),
                            ),
                          ),
                        ),
                      ),
                    ),
            );
          },
        );
      },
    );
  }
}

/// Top of the rail: brand aligned with destination icons; theme at the
/// trailing edge when the rail is extended, or centred alone when compact.
class _RailLeading extends StatelessWidget {
  const _RailLeading({required this.controller, required this.extended});

  final AppController controller;
  final bool extended;

  @override
  Widget build(BuildContext context) {
    final Widget theme = _ThemeCycleButton(controller: controller);
    if (!extended) {
      return SizedBox(
        width: _railMinWidth,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(0, 10, 0, 18),
          child: Center(child: theme),
        ),
      );
    }

    return Padding(
      padding: EdgeInsets.fromLTRB(
        _destinationPadding.left,
        10,
        _destinationPadding.right,
        18,
      ),
      child: SizedBox(
        width: _railMinExtendedWidth,
        child: Row(
          children: <Widget>[
            SizedBox(
              width: _railMinWidth,
              child: Center(
                child: Image.asset(
                  'assets/branding/usque-ui-icon.png',
                  width: 30,
                  height: 30,
                  filterQuality: FilterQuality.medium,
                ),
              ),
            ),
            Expanded(
              child: Text(
                'Usque',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.titleLarge,
              ),
            ),
            theme,
          ],
        ),
      ),
    );
  }
}

class _ThemeCycleButton extends StatelessWidget {
  const _ThemeCycleButton({required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    return ControllerSelector<ThemePreference>(
      controller: controller,
      selector: (controller) => controller.themePreference,
      builder: (context, preference) {
        final bool touchPlatform =
            Theme.of(context).platform == TargetPlatform.android;
        final IconData icon = switch (preference) {
          ThemePreference.system => LucideIcons.sunMoon,
          ThemePreference.light => LucideIcons.sun,
          ThemePreference.dark => LucideIcons.moon,
        };
        final String label = strings.get(switch (preference) {
          ThemePreference.system => 'theme_system',
          ThemePreference.light => 'theme_light',
          ThemePreference.dark => 'theme_dark',
        });
        return IconButton(
          tooltip: '${strings.get('theme')} · $label',
          iconSize: 19,
          visualDensity: touchPlatform
              ? VisualDensity.standard
              : VisualDensity.compact,
          style: IconButton.styleFrom(
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
            padding: const EdgeInsets.all(8),
            minimumSize: Size.square(touchPlatform ? 48 : 36),
          ),
          onPressed: () => controller.setTheme(
            ThemePreference.values[(preference.index + 1) %
                ThemePreference.values.length],
          ),
          icon: AnimatedSwitcher(
            duration: UsqueMotion.of(context, UsqueMotion.base),
            transitionBuilder: (child, animation) => FadeTransition(
              opacity: animation,
              child: ScaleTransition(scale: animation, child: child),
            ),
            child: Icon(icon, key: ValueKey<ThemePreference>(preference)),
          ),
        );
      },
    );
  }
}
