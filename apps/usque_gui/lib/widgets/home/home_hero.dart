import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/app_strings.dart';
import '../../core/chain_home_status.dart';
import '../../core/connection_presentation.dart';
import '../../core/usque_motion.dart';
import '../../core/usque_theme.dart';
import '../../models/app_models.dart';
import '../../models/scenarios.dart';
import '../../state/app_controller.dart';
import '../common.dart';
import '../connection_ring.dart';
import '../controller_selector.dart';
import '../country_flag.dart';
import '../live_duration.dart';
import '../profile_identity_dialog.dart';
import '../setup_handoff.dart';
import 'home_details.dart';
import 'home_scenario.dart';

typedef _HeroView = ({
  ConnectionPhase phase,
  bool busy,
  String? errorCode,
  String profileName,
  bool identityReady,
  bool chainEnabled,
  String chainStage,
  String gateStage,
  bool chainProfile,
  bool gateServer,
});

_HeroView _heroView(AppController controller) => (
  phase: controller.snapshot.phase,
  busy: controller.busy,
  errorCode: controller.snapshot.errorCode,
  profileName: controller.activeProfile.name,
  identityReady:
      controller.identityState(controller.activeProfileId) ==
      ProfileIdentityState.ready,
  chainEnabled: controller.activeProfile.chainEnabled,
  chainStage: controller.snapshot.chainExit.stage,
  gateStage: controller.snapshot.vpnGate.stage,
  chainProfile: controller.snapshot.chainExit.currentProfile != null,
  gateServer: controller.snapshot.vpnGate.server != null,
);

/// The connection control and the one overall status beside or under it.
///
/// Desktop puts the ring beside the session readouts when [ringSize] is
/// given and stacks them otherwise; [fill] lets a stretched card centre the
/// pair above the details row. Phones centre everything under the ring.
class HomeConnectionHero extends StatelessWidget {
  const HomeConnectionHero({
    required this.controller,
    required this.strings,
    this.compact = false,
    this.ringSize,
    this.fill = false,
    this.banner,
    this.footer,
    super.key,
  });

  final AppController controller;
  final AppStrings strings;
  final bool compact;

  /// Desktop ring diameter beside the readouts; null stacks them.
  final double? ringSize;

  /// The hero has a bounded height to distribute (a stretched card).
  final bool fill;

  /// Phone error slot, kept next to the status it explains.
  final Widget? banner;

  /// Desktop details row along the bottom of the card.
  final Widget? footer;

  @override
  Widget build(BuildContext context) => ControllerSelector<_HeroView>(
    controller: controller,
    active: (controller) => controller.section == AppSection.home,
    selector: _heroView,
    builder: (context, view) => _buildHero(context, view),
  );

  Widget _buildHero(BuildContext context, _HeroView view) {
    final theme = Theme.of(context);
    final connection = ConnectionPresentation.of(view.phase);
    final chainStatus = ChainHomeStatus.of(
      phase: view.phase,
      chainEnabled: view.chainEnabled,
      chainStage: view.chainStage,
      gateStage: view.gateStage,
      hasCurrentProfile: view.chainProfile,
      hasGateServer: view.gateServer,
    );
    final presentation = chainStatus.drivesHome
        ? chainStatus.presentation(connection)
        : connection;
    final status = chainStatus.drivesHome
        ? chainStatus.label(strings)
        : strings.get(connection.labelKey);
    final error = view.phase == ConnectionPhase.error;
    final recoveryBlocked =
        error && view.errorCode == 'WINDOWS_RECOVERY_BLOCKED';
    final primaryRetry = error && !recoveryBlocked && view.identityReady;
    final action = strings.get(
      primaryRetry
          ? 'retry'
          : error && !recoveryBlocked && !view.identityReady
          ? 'configure_identity'
          : connection.actionKey,
    );
    final canAct =
        (!view.busy ||
            view.phase == ConnectionPhase.preparing ||
            view.phase == ConnectionPhase.connectingH3 ||
            view.phase == ConnectionPhase.connectingH2 ||
            view.phase == ConnectionPhase.reconnecting) &&
        view.phase != ConnectionPhase.disconnecting &&
        !recoveryBlocked;
    // After first-run setup the ring settles into place inside the arriving
    // page, picking up the ring that setup just closed.
    Widget ring(double size) => SetupEntranceTransition(
      lift: 0,
      scale: 0.92,
      fade: false,
      child: ConnectionRing(
        phase: view.phase,
        presentation: chainStatus.drivesHome ? presentation : null,
        busy: view.busy,
        actionLabel: action,
        semanticLabel: '${strings.get('connection_status')}: $status',
        size: size,
        compactControl: compact,
        onPressed: !canAct
            ? null
            : primaryRetry
            ? controller.retry
            : () => _connectOrRepairIdentity(context),
      ),
    );
    final headline = _StatusHeadline(
      status: status,
      statusKey: ValueKey(
        chainStatus.drivesHome ? chainStatus.labelKey : view.phase,
      ),
      color: presentation.indicatorColor(
        UsqueTokens.of(context),
        theme.colorScheme,
      ),
      centered: compact,
      style: compact
          ? theme.textTheme.headlineSmall
          : theme.textTheme.headlineMedium,
    );
    final retry = connection.recoverable && !error
        ? OutlinedButton.icon(
            onPressed: view.busy ? null : controller.retry,
            icon: const Icon(LucideIcons.refreshCw, size: 18),
            label: Text(strings.get('retry')),
          )
        : null;

    if (compact) {
      return LayoutBuilder(
        key: const ValueKey('mobile-connection-section'),
        builder: (context, constraints) {
          final size = (constraints.maxWidth * 0.54).clamp(180.0, 228.0);
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Center(child: ring(size)),
              const SizedBox(height: UsqueSpace.m),
              headline,
              // A failure is explained by the banner below, not a location.
              if (!error) ...[
                const SizedBox(height: UsqueSpace.xs - 2),
                _CompactSessionLine(controller: controller),
              ],
              const SizedBox(height: UsqueSpace.m),
              ?banner,
              if (retry != null) ...[
                Center(child: retry),
                const SizedBox(height: UsqueSpace.m),
              ],
            ],
          );
        },
      );
    }

    final session = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        _AccountEyebrow(
          name: view.profileName,
          strings: strings,
          onOpen: () => controller.openProfiles(ProfilesTab.accounts),
        ),
        const SizedBox(height: UsqueSpace.xs),
        headline,
        const SizedBox(height: UsqueSpace.s),
        _DesktopSessionReadout(controller: controller),
        if (retry != null) ...[
          const SizedBox(height: UsqueSpace.m),
          Align(alignment: AlignmentDirectional.centerStart, child: retry),
        ],
      ],
    );
    final Widget body;
    if (ringSize case final size?) {
      body = Row(
        children: [
          ring(size),
          const SizedBox(width: UsqueSpace.xl),
          Expanded(child: session),
        ],
      );
    } else {
      body = LayoutBuilder(
        builder: (context, constraints) => Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Center(child: ring(constraints.maxWidth.clamp(180.0, 260.0))),
            const SizedBox(height: UsqueSpace.l),
            session,
          ],
        ),
      );
    }
    return Column(
      key: const ValueKey('home-desktop-connection'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: fill ? MainAxisSize.max : MainAxisSize.min,
      children: [
        if (fill)
          Expanded(
            child: Align(
              alignment: AlignmentDirectional.centerStart,
              child: body,
            ),
          )
        else
          body,
        if (footer case final footer?) ...[
          const SizedBox(height: UsqueSpace.m),
          Divider(height: 1, color: UsqueTokens.of(context).hairline),
          footer,
        ],
      ],
    );
  }

  Future<void> _connectOrRepairIdentity(BuildContext context) async {
    if (controller.snapshot.isConnected) {
      await controller.connectOrDisconnect();
      return;
    }
    final profile = controller.activeProfile;
    if (controller.identityState(profile.id) != ProfileIdentityState.ready) {
      final repaired = await showProfileIdentityDialog(
        context,
        controller: controller,
        profile: profile,
      );
      if (!repaired || !context.mounted) return;
    }
    await controller.connectOrDisconnect();
  }
}

/// The overall status with a lamp in the phase colour. The words carry the
/// state; the lamp only supports them.
class _StatusHeadline extends StatelessWidget {
  const _StatusHeadline({
    required this.status,
    required this.statusKey,
    required this.color,
    required this.centered,
    required this.style,
  });

  final String status;
  final Key statusKey;
  final Color color;
  final bool centered;
  final TextStyle? style;

  @override
  Widget build(BuildContext context) {
    final lampSize = centered ? 9.0 : 10.0;
    return Semantics(
      liveRegion: true,
      child: Row(
        mainAxisAlignment: centered
            ? MainAxisAlignment.center
            : MainAxisAlignment.start,
        children: [
          ExcludeSemantics(
            child: AnimatedContainer(
              duration: UsqueMotion.of(context, UsqueMotion.base),
              curve: UsqueMotion.standard,
              width: lampSize,
              height: lampSize,
              decoration: BoxDecoration(
                color: color,
                shape: BoxShape.circle,
                boxShadow: [
                  BoxShadow(
                    color: color.withValues(alpha: 0.35),
                    blurRadius: 8,
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(width: UsqueSpace.s - 2),
          Flexible(
            child: FadeThroughSwitcher(
              alignment: centered
                  ? Alignment.center
                  : AlignmentDirectional.centerStart,
              child: Text(
                status,
                key: statusKey,
                textAlign: centered ? TextAlign.center : TextAlign.start,
                style: style,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// The active account above the status, named for assistive technology.
class _AccountEyebrow extends StatelessWidget {
  const _AccountEyebrow({
    required this.name,
    required this.strings,
    required this.onOpen,
  });

  final VoidCallback onOpen;

  final String name;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final muted = theme.colorScheme.onSurfaceVariant;
    return Tooltip(
      message: '${strings.get('active_profile')}: $name',
      child: Semantics(
        label: '${strings.get('active_profile')}: $name',
        button: true,
        excludeSemantics: true,
        child: InkWell(
          onTap: onOpen,
          child: Row(
            key: const ValueKey('home-active-account'),
            children: [
              Icon(LucideIcons.circleUserRound, size: 16, color: muted),
              const SizedBox(width: UsqueSpace.xs - 2),
              Flexible(
                child: Text(
                  name,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.bodyMedium?.copyWith(
                    color: muted,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

typedef _SessionView = ({
  String catalogId,
  String? transport,
  String? family,
  DateTime? since,
  bool connected,
  String location,
  String? countryCode,
  String killKey,
  bool alwaysOn,
  bool lockdown,
});

_SessionView _sessionView(AppController app) {
  final snapshot = app.snapshot;
  final strings = app.strings;
  return (
    catalogId: strings.catalogId,
    transport: snapshot.dataPlane == DataPlaneMode.l4Proxy
        ? 'L4 / H3'
        : snapshot.transport,
    family: snapshot.addressFamily,
    since: snapshot.connectedAt,
    connected: snapshot.isConnected,
    location: snapshot.exit.country?.trim().isNotEmpty == true
        ? snapshot.exit.country!.trim()
        : strings.get('home_exit_unknown'),
    countryCode: snapshot.exit.countryCode,
    killKey: killSwitchStatusKey(
      profile: app.activeProfile,
      snapshot: snapshot,
    ),
    alwaysOn: snapshot.alwaysOn,
    lockdown: snapshot.platformLockdown,
  );
}

/// Exit location, session metrics and protection under the desktop status.
class _DesktopSessionReadout extends StatelessWidget {
  const _DesktopSessionReadout({required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) => ControllerSelector<_SessionView>(
    controller: controller,
    active: (app) => app.section == AppSection.home,
    selector: _sessionView,
    builder: (context, view) {
      final strings = controller.strings;
      final theme = Theme.of(context);
      final muted = theme.colorScheme.onSurfaceVariant;
      Widget metric(String label, Widget value) => Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(label, style: theme.textTheme.bodySmall?.copyWith(color: muted)),
          const SizedBox(height: UsqueSpace.xxs),
          value,
        ],
      );
      Widget reading(String? value) => value == null
          ? const EmptyValue(label: '—')
          : Text(value, style: UsqueTheme.readout(context, size: 15));
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(
            key: const ValueKey('home-exit-location'),
            children: [
              Icon(LucideIcons.mapPin, size: 16, color: muted),
              const SizedBox(width: UsqueSpace.xs),
              if (view.connected) ...[
                CountryFlag(countryCode: view.countryCode),
                const SizedBox(width: UsqueSpace.xs),
              ],
              Expanded(
                child: Text(
                  view.connected
                      ? view.location
                      : strings.get('location_disconnected'),
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: view.connected
                      ? theme.textTheme.titleSmall
                      : theme.textTheme.bodyMedium?.copyWith(color: muted),
                ),
              ),
            ],
          ),
          const SizedBox(height: UsqueSpace.l - 4),
          Wrap(
            spacing: UsqueSpace.l,
            runSpacing: UsqueSpace.s,
            children: [
              metric(strings.get('protocol'), reading(view.transport)),
              metric(strings.get('address_family'), reading(view.family)),
              metric(
                strings.get('duration'),
                LiveDuration(
                  since: view.since,
                  style: UsqueTheme.readout(context, size: 15),
                ),
              ),
            ],
          ),
          const SizedBox(height: UsqueSpace.l - 4),
          ScenarioHomeLink(controller: controller),
          HomeProtectionLine(
            killKey: view.killKey,
            alwaysOn: view.alwaysOn,
            lockdown: view.lockdown,
            strings: strings,
          ),
        ],
      );
    },
  );
}

/// Kill Switch and platform protection as one quiet line of statuses.
class HomeProtectionLine extends StatelessWidget {
  const HomeProtectionLine({
    required this.killKey,
    required this.alwaysOn,
    required this.lockdown,
    required this.strings,
    super.key,
  });

  final String killKey;
  final bool alwaysOn;
  final bool lockdown;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final (icon, tone) = killSwitchPresentation(killKey);
    Widget item(
      IconData icon,
      Color iconColor,
      String label,
      String value, {
      Color? valueColor,
    }) => Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        ExcludeSemantics(child: Icon(icon, size: 16, color: iconColor)),
        const SizedBox(width: UsqueSpace.xs - 2),
        // Long translations wrap the value under its label.
        Flexible(
          child: Wrap(
            spacing: UsqueSpace.xs - 2,
            children: [
              Text(
                label,
                style: theme.textTheme.bodySmall?.copyWith(
                  color: theme.colorScheme.onSurfaceVariant,
                ),
              ),
              Text(
                value,
                style: theme.textTheme.bodySmall?.copyWith(
                  color: valueColor ?? theme.colorScheme.onSurface,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ],
          ),
        ),
      ],
    );
    final toneColor = statusToneColor(context, tone);
    return Wrap(
      key: const ValueKey('home-protection'),
      crossAxisAlignment: WrapCrossAlignment.center,
      spacing: UsqueSpace.m,
      runSpacing: UsqueSpace.xs,
      children: [
        item(
          icon,
          toneColor,
          strings.get('kill_switch'),
          strings.get(killKey),
          valueColor: tone == StatusTone.neutral ? null : toneColor,
        ),
        if (alwaysOn)
          item(
            LucideIcons.shield,
            theme.colorScheme.onSurfaceVariant,
            strings.get('always_on'),
            strings.get('on'),
          ),
        if (lockdown)
          item(
            LucideIcons.shieldBan,
            theme.colorScheme.onSurfaceVariant,
            strings.get('lockdown'),
            strings.get('on'),
          ),
      ],
    );
  }
}

/// The exit and session clock under the phone status, or a quiet waiting
/// line that holds the same place while disconnected.
class _CompactSessionLine extends StatelessWidget {
  const _CompactSessionLine({required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) => ControllerSelector<_SessionView>(
    controller: controller,
    active: (app) => app.section == AppSection.home,
    selector: _sessionView,
    builder: (context, view) {
      final strings = controller.strings;
      final theme = Theme.of(context);
      final muted = theme.colorScheme.onSurfaceVariant;
      if (!view.connected) {
        return Text(
          strings.get('location_disconnected'),
          key: const ValueKey('home-exit-location'),
          textAlign: TextAlign.center,
          style: theme.textTheme.bodyMedium?.copyWith(color: muted),
        );
      }
      return Wrap(
        key: const ValueKey('home-exit-location'),
        alignment: WrapAlignment.center,
        crossAxisAlignment: WrapCrossAlignment.center,
        spacing: UsqueSpace.xs,
        runSpacing: UsqueSpace.xxs,
        children: [
          Tooltip(
            message: '${strings.get('home_exit_region')}: ${view.location}',
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                CountryFlag(countryCode: view.countryCode),
                const SizedBox(width: UsqueSpace.xs),
                Flexible(
                  child: Text(
                    view.location,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.titleSmall,
                  ),
                ),
              ],
            ),
          ),
          // A session without a reported start shows no clock rather
          // than a dangling dash.
          if (view.since != null) ...[
            ExcludeSemantics(
              child: Text(
                '·',
                style: theme.textTheme.bodyMedium?.copyWith(color: muted),
              ),
            ),
            Semantics(
              label: strings.get('duration'),
              child: LiveDuration(
                since: view.since,
                style: UsqueTheme.readout(context, size: 14, color: muted),
              ),
            ),
          ],
        ],
      );
    },
  );
}
