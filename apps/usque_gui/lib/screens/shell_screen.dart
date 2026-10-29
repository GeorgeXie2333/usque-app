import 'dart:async';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/animated_index_stack.dart';
import '../widgets/controller_selector.dart';
import '../widgets/desktop_shortcuts.dart';
import '../widgets/section_navigator.dart';
import '../widgets/usque_logo.dart';
import 'diagnostics_screen.dart';
import 'home_screen.dart';
import 'profiles_screen.dart';
import 'proxy_section.dart';
import 'settings_screen.dart';

const double _railMinWidth = 78;
const double _railMinExtendedWidth = 232;

/// Matches [NavigationRailDestination.padding] so the brand sits on the
/// same vertical axis as the destination icons.
const EdgeInsets _destinationPadding = EdgeInsets.symmetric(
  vertical: 3,
  horizontal: 8,
);

class ShellScreen extends StatefulWidget {
  const ShellScreen({required this.controller, super.key});

  final AppController controller;

  @override
  State<ShellScreen> createState() => _ShellScreenState();
}

class _ShellScreenState extends State<ShellScreen> {
  AppController get controller => widget.controller;
  final _proxySection = GlobalKey<ProxySectionState>();
  final _sectionNavigators = <AppSection, GlobalKey<SectionNavigatorState>>{
    AppSection.profiles: GlobalKey<SectionNavigatorState>(),
    AppSection.proxy: GlobalKey<SectionNavigatorState>(),
    AppSection.settings: GlobalKey<SectionNavigatorState>(),
  };
  final _subpageOpen = <AppSection>{};
  bool _changingSection = false;
  bool _openingChainProxy = false;
  bool _openingDiagnostics = false;

  static const _sectionKeys = <LogicalKeyboardKey>[
    LogicalKeyboardKey.digit1,
    LogicalKeyboardKey.digit2,
    LogicalKeyboardKey.digit3,
    LogicalKeyboardKey.digit4,
  ];

  @override
  void initState() {
    super.initState();
    if (desktopShortcutsSupported) {
      HardwareKeyboard.instance.addHandler(_handleShortcut);
    }
  }

  @override
  void dispose() {
    if (desktopShortcutsSupported) {
      HardwareKeyboard.instance.removeHandler(_handleShortcut);
    }
    super.dispose();
  }

  /// Ctrl+1–4 select a section; Escape and Alt+Left leave a subpage.
  bool _handleShortcut(KeyEvent event) {
    if (event is! KeyDownEvent || !mounted || !routeChainIsCurrent(context)) {
      return false;
    }
    final keyboard = HardwareKeyboard.instance;
    final key = event.logicalKey;
    final index = _sectionKeys.indexOf(key);
    if (index >= 0 &&
        keyboard.isControlPressed &&
        !keyboard.isAltPressed &&
        !keyboard.isShiftPressed &&
        !keyboard.isMetaPressed) {
      final sections = controller.availableSections;
      if (index >= sections.length) return false;
      unawaited(_selectSection(sections[index]));
      return true;
    }
    final back =
        key == LogicalKeyboardKey.escape &&
            !keyboard.isControlPressed &&
            !keyboard.isAltPressed &&
            !textInputHasFocus ||
        key == LogicalKeyboardKey.arrowLeft &&
            keyboard.isAltPressed &&
            !keyboard.isControlPressed &&
            !keyboard.isShiftPressed;
    return back && _leaveSubpage();
  }

  /// Pops the current section's subpage through its unsaved-changes guard.
  bool _leaveSubpage() {
    final section = controller.section;
    if (!_subpageOpen.contains(section)) return false;
    final sectionNavigator = _sectionNavigators[section]?.currentState;
    final navigator = sectionNavigator?.navigator;
    // A popup on the section navigator handles Escape itself.
    if (navigator == null || sectionNavigator!.popupOpen) return false;
    unawaited(navigator.maybePop());
    return true;
  }

  ValueChanged<bool> _onSubpageChanged(AppSection section) => (open) {
    if (!mounted) return;
    setState(() {
      if (open) {
        _subpageOpen.add(section);
      } else {
        _subpageOpen.remove(section);
      }
    });
  };

  Future<void> _openChainProxy() async {
    if (_openingChainProxy) return;
    _openingChainProxy = true;
    final selectedController = controller;
    try {
      if (!await _selectSection(AppSection.proxy)) return;
      // First visits mount lazily. Wait for the section's navigator and state
      // before its subpage starts foreground work.
      await WidgetsBinding.instance.endOfFrame;
      if (!mounted ||
          controller != selectedController ||
          controller.section != AppSection.proxy) {
        return;
      }
      unawaited(_proxySection.currentState?.openChainProxy());
    } finally {
      _openingChainProxy = false;
    }
  }

  /// Opens Diagnostics inside Settings, so desktop navigation stays visible
  /// and Back returns to the Settings list, then starts a Standard run.
  Future<void> _openDiagnostics() async {
    if (_openingDiagnostics) return;
    _openingDiagnostics = true;
    final selectedController = controller;
    try {
      if (!await _selectSection(AppSection.settings)) return;
      await WidgetsBinding.instance.endOfFrame;
      if (!mounted ||
          controller != selectedController ||
          controller.section != AppSection.settings) {
        return;
      }
      final section = _sectionNavigators[AppSection.settings]?.currentState;
      // An open Settings subpage keeps its unsaved-changes guard.
      if (section == null || !await section.closeSubpages()) return;
      final navigator = section.navigator;
      if (!mounted || controller != selectedController || navigator == null) {
        return;
      }
      startStandardDiagnostics(controller);
      unawaited(
        navigator.push<void>(
          MaterialPageRoute<void>(
            settings: const RouteSettings(name: '/settings/diagnostics'),
            builder: (_) => DiagnosticsScreen(controller: controller),
          ),
        ),
      );
    } finally {
      _openingDiagnostics = false;
    }
  }

  Future<bool> _selectSection(AppSection section) async {
    if (_changingSection) return false;
    _changingSection = true;
    final selectedController = controller;
    try {
      final navigator = _sectionNavigators[controller.section]?.currentState;
      if (navigator != null && !await navigator.closeSubpages()) {
        return false;
      }
      if (!mounted || controller != selectedController) return false;
      controller.selectSection(section);
      return true;
    } finally {
      _changingSection = false;
    }
  }

  final _destinationKeys = <AppSection, GlobalKey<TooltipState>>{
    for (final section in AppSection.values) section: GlobalKey<TooltipState>(),
  };

  static const _icons = <AppSection, IconData>{
    AppSection.home: LucideIcons.house,
    AppSection.profiles: LucideIcons.layers3,
    AppSection.proxy: LucideIcons.waypoints,
    AppSection.settings: LucideIcons.settings,
  };

  /// Width at which the bottom bar gives way to the side rail.
  static const double _railBreakpoint = 760;

  /// Width at which the rail can afford to show labels beside the icons.
  /// The extended rail takes 249 px with its divider, so this leaves Home its
  /// 860 px side-by-side width plus page gutters instead of stacking it.
  static const double _extendedBreakpoint = 1180;

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
    final sections = controller.availableSections;
    final next =
        (sections.indexOf(controller.section) + delta + sections.length) %
        sections.length;
    final selected = sections[next];
    final selectedController = controller;
    final navigationFocus = FocusManager.instance.primaryFocus;
    unawaited(
      _selectSection(selected).then((changed) {
        if (changed) {
          // A section's first visit creates its Navigator, which can request
          // focus. Keep arrow-key navigation in the bar or rail after mounting.
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (!mounted ||
                controller != selectedController ||
                controller.section != selected) {
              return;
            }
            if (!vertical) {
              if (navigationFocus?.context?.mounted ?? false) {
                navigationFocus!.requestFocus();
              }
              return;
            }
            final destinationContext =
                _destinationKeys[selected]?.currentContext;
            if (destinationContext == null) return;
            final destinationFocus = Focus.of(destinationContext);
            destinationFocus.requestFocus();
            // Selection alone does not reveal a scrollable rail destination.
            unawaited(
              Scrollable.ensureVisible(
                destinationFocus.context ?? destinationContext,
              ),
            );
          });
        }
      }),
    );
    return KeyEventResult.handled;
  }

  @override
  Widget build(BuildContext context) {
    return ControllerSelector<AppSection>(
      controller: controller,
      selector: (controller) => controller.section,
      builder: (context, section) {
        final sections = controller.availableSections;
        final strings = controller.strings;
        final labels = <String>[
          strings.get('nav_home'),
          strings.get('nav_profiles'),
          strings.get('nav_proxy'),
          strings.get('nav_settings'),
        ];
        final fullLabels = <String>[
          strings.get('home'),
          strings.get('profiles'),
          strings.get('proxy'),
          strings.get('settings'),
        ];
        final pages = <Widget>[
          // Home subscribes per block, so it takes the controller directly.
          HomeScreen(
            key: const ValueKey<String>('home-page'),
            controller: controller,
            onOpenChainProxy: () => unawaited(_openChainProxy()),
            onOpenDiagnostics: () => unawaited(_openDiagnostics()),
          ),
          SectionNavigator(
            key: _sectionNavigators[AppSection.profiles],
            active: section == AppSection.profiles,
            rootName: '/profiles',
            onSubpageChanged: _onSubpageChanged(AppSection.profiles),
            builder: (context) =>
                ProfilesScreen(controller: controller, gateObservation: true),
          ),
          ProxySection(
            key: _proxySection,
            navigatorKey: _sectionNavigators[AppSection.proxy]!,
            controller: controller,
            active: section == AppSection.proxy,
            onSubpageChanged: _onSubpageChanged(AppSection.proxy),
          ),
          SectionNavigator(
            key: _sectionNavigators[AppSection.settings],
            active: section == AppSection.settings,
            rootName: '/settings',
            onSubpageChanged: _onSubpageChanged(AppSection.settings),
            builder: (context) =>
                ControllerSelector<
                  ({
                    ThemePreference theme,
                    LocalePreference locale,
                    bool networkQualitySupported,
                    bool updateChecksEnabled,
                    UpdateCheckResult? updateResult,
                    UpdateOperationPhase updatePhase,
                    int updateDownloadedBytes,
                    int updateTotalBytes,
                    String? updateError,
                    String? downloadedUpdatePath,
                    bool busy,
                    String? error,
                    String? notice,
                    UsqueProfile profile,
                  })
                >(
                  key: const ValueKey<String>('settings-controller-selector'),
                  controller: controller,
                  active: (controller) =>
                      controller.section == AppSection.settings,
                  selector: (controller) => (
                    theme: controller.themePreference,
                    locale: controller.localePreference,
                    networkQualitySupported:
                        controller.engineCapabilities?.networkQuality ?? false,
                    updateChecksEnabled: controller.updateChecksEnabled,
                    updateResult: controller.updateResult,
                    updatePhase: controller.updatePhase,
                    updateDownloadedBytes: controller.updateDownloadedBytes,
                    updateTotalBytes: controller.updateTotalBytes,
                    updateError: controller.updateError,
                    downloadedUpdatePath: controller.downloadedUpdatePath,
                    busy: controller.busy,
                    error: controller.lastError,
                    notice: controller.lastNotice,
                    profile: controller.activeProfile,
                  ),
                  builder: (context, _) =>
                      SettingsScreen(controller: controller),
                ),
          ),
        ];
        final selected = sections
            .indexOf(section)
            .clamp(0, sections.length - 1);

        return LayoutBuilder(
          builder: (context, constraints) {
            final useRail = constraints.maxWidth >= _railBreakpoint;
            final extended = constraints.maxWidth >= _extendedBreakpoint;
            final scaffold = Scaffold(
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
                          scrollable: true,
                          minWidth: _railMinWidth,
                          minExtendedWidth: _railMinExtendedWidth,
                          selectedIndex: selected,
                          onDestinationSelected: (index) =>
                              unawaited(_selectSection(sections[index])),
                          labelType: extended
                              ? NavigationRailLabelType.none
                              : NavigationRailLabelType.all,
                          leading: _RailLeading(extended: extended),
                          destinations:
                              List<NavigationRailDestination>.generate(
                                labels.length,
                                (index) => NavigationRailDestination(
                                  icon: Tooltip(
                                    message: fullLabels[index],
                                    excludeFromSemantics: true,
                                    child: Icon(_icons[sections[index]]),
                                  ),
                                  selectedIcon: Tooltip(
                                    message: fullLabels[index],
                                    excludeFromSemantics: true,
                                    child: Icon(_icons[sections[index]]),
                                  ),
                                  label: Tooltip(
                                    key: _destinationKeys[sections[index]],
                                    message: fullLabels[index],
                                    excludeFromSemantics: true,
                                    child: Text(labels[index]),
                                  ),
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
                      key: const ValueKey('shell-content'),
                      child: AnimatedIndexStack(
                        index: selected,
                        children: pages,
                      ),
                    ),
                  ],
                ),
              ),
              bottomNavigationBar: useRail || _subpageOpen.contains(section)
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
                            onDestinationSelected: (index) =>
                                unawaited(_selectSection(sections[index])),
                            destinations: List<NavigationDestination>.generate(
                              labels.length,
                              (index) => NavigationDestination(
                                icon: Icon(_icons[sections[index]]),
                                selectedIcon: Icon(_icons[sections[index]]),
                                label: labels[index],
                                tooltip: fullLabels[index],
                              ),
                            ),
                          ),
                        ),
                      ),
                    ),
            );
            if (!desktopShortcutsSupported) return scaffold;
            return Listener(
              behavior: HitTestBehavior.translucent,
              onPointerDown: (event) {
                if (event.buttons & kBackMouseButton != 0) _leaveSubpage();
              },
              child: scaffold,
            );
          },
        );
      },
    );
  }
}

/// Top of the rail: the brand aligned with destination icons when the rail is
/// extended. Theme is chosen in Settings, so the compact rail only keeps the
/// spacing above its first destination.
class _RailLeading extends StatelessWidget {
  const _RailLeading({required this.extended});

  final bool extended;

  @override
  Widget build(BuildContext context) {
    if (!extended) {
      return const SizedBox(width: _railMinWidth, height: 28);
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
              child: Center(child: const UsqueLogo(size: 30)),
            ),
            Expanded(
              child: Text(
                'Usque',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.titleLarge,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
