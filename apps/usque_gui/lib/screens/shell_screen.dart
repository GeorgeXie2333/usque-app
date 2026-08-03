import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/controller_selector.dart';
import 'diagnostics_screen.dart';
import 'home_screen.dart';
import 'profiles_screen.dart';
import 'proxy_screen.dart';
import 'settings_screen.dart';

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
          ControllerSelector<
            ({
              EngineSnapshot snapshot,
              String? error,
              bool busy,
              UsqueProfile profile,
            })
          >(
            key: const ValueKey<String>('home-controller-selector'),
            controller: controller,
            active: (controller) => controller.section == AppSection.home,
            selector: (controller) => (
              snapshot: controller.snapshot,
              error: controller.lastError,
              busy: controller.busy,
              profile: controller.activeProfile,
            ),
            builder: (context, _) => HomeScreen(controller: controller),
          ),
          ControllerSelector<
            ({List<UsqueProfile> profiles, String activeProfileId})
          >(
            key: const ValueKey<String>('profiles-controller-selector'),
            controller: controller,
            active: (controller) => controller.section == AppSection.profiles,
            selector: (controller) => (
              profiles: controller.profiles,
              activeProfileId: controller.activeProfileId,
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
            final useRail = constraints.maxWidth >= 760;
            final extended = constraints.maxWidth >= 1050;
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
                          minExtendedWidth: 230,
                          selectedIndex: selected,
                          onDestinationSelected: (index) => controller
                              .selectSection(AppSection.values[index]),
                          labelType: extended
                              ? NavigationRailLabelType.none
                              : NavigationRailLabelType.all,
                          leading: Padding(
                            padding: const EdgeInsets.fromLTRB(12, 12, 12, 28),
                            child: Row(
                              mainAxisSize: MainAxisSize.min,
                              children: <Widget>[
                                Image.asset(
                                  'assets/branding/usque-ui-icon.png',
                                  width: 44,
                                  height: 44,
                                ),
                                if (extended) ...<Widget>[
                                  const SizedBox(width: 12),
                                  Text(
                                    'Usque',
                                    style: Theme.of(
                                      context,
                                    ).textTheme.titleLarge,
                                  ),
                                  const SizedBox(width: 10),
                                  const _BetaBadge(),
                                ],
                              ],
                            ),
                          ),
                          destinations:
                              List<NavigationRailDestination>.generate(
                                labels.length,
                                (index) => NavigationRailDestination(
                                  icon: Icon(_icons[index]),
                                  selectedIcon: Icon(_icons[index]),
                                  label: Text(labels[index]),
                                  padding: const EdgeInsets.symmetric(
                                    vertical: 5,
                                  ),
                                ),
                              ),
                        ),
                      ),
                      VerticalDivider(
                        width: 1,
                        thickness: 1,
                        color: Theme.of(context).dividerColor,
                      ),
                    ],
                    Expanded(
                      child: IndexedStack(index: selected, children: pages),
                    ),
                  ],
                ),
              ),
              bottomNavigationBar: useRail
                  ? null
                  : SafeArea(
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
            );
          },
        );
      },
    );
  }
}

class _BetaBadge extends StatelessWidget {
  const _BetaBadge();

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.secondaryContainer,
        borderRadius: BorderRadius.circular(999),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
        child: Text(
          'BETA',
          style: Theme.of(context).textTheme.labelSmall?.copyWith(
            fontWeight: FontWeight.w800,
            letterSpacing: 0.7,
          ),
        ),
      ),
    );
  }
}
