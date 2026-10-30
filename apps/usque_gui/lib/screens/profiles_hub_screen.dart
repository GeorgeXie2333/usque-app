import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/chain_strings.dart';
import '../core/proton_strings.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../models/scenarios.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/proton_vpn_panel.dart' show loadCachedProtonServers;
import '../widgets/usque_dialog.dart';
import 'profiles_screen.dart' show AccountsScreen, AddAccountButton;
import 'scenario_editor_screen.dart';

class ProfilesScreen extends StatefulWidget {
  const ProfilesScreen({
    required this.controller,
    this.gateObservation = false,
    super.key,
  });
  final bool gateObservation;
  final AppController controller;
  @override
  State<ProfilesScreen> createState() => _ProfilesScreenState();
}

class _ProfilesScreenState extends State<ProfilesScreen> {
  AppController get app => widget.controller;
  Map<String, String> _exitNames = {};
  String? _labelsKey;
  int _labelGeneration = 0;

  void _refreshExitNames() {
    final scenes =
        app.scenarios.catalog?.scenarios ?? const <ConnectionScenario>[];
    final refs =
        scenes
            .map((scene) => jsonEncode(scene.settings.values['chain_exit']))
            .toList()
          ..sort();
    final key = '${app.chainLibraryRevision}:${refs.join('|')}';
    if (_labelsKey == key || scenes.isEmpty) return;
    _labelsKey = key;
    final generation = ++_labelGeneration;
    unawaited(() async {
      final names = await scenarioExitNames(
        app,
        settings: scenes.map((scene) => scene.settings.values),
      );
      if (mounted && generation == _labelGeneration) {
        setState(() => _exitNames = names);
      }
    }());
  }

  @override
  void initState() {
    super.initState();
    app.addListener(_changed);
    if (app.scenarios.supported) {
      unawaited(app.scenarios.refresh());
      _refreshExitNames();
    }
  }

  void _changed() {
    if (!mounted ||
        widget.gateObservation &&
            (app.section != AppSection.profiles || !app.uiVisible)) {
      return;
    }
    setState(() {});
    _refreshExitNames();
  }

  @override
  void dispose() {
    app.removeListener(_changed);
    super.dispose();
  }

  Future<void> _edit({
    ConnectionScenario? scene,
    bool duplicate = false,
  }) async {
    await Navigator.of(context).push<void>(
      MaterialPageRoute(
        builder: (_) => ScenarioEditorScreen(
          controller: app,
          scene: scene,
          duplicate: duplicate,
        ),
      ),
    );
  }

  /// Repair a binding whose account was deleted, keeping the saved settings.
  Future<void> _unbind(ConnectionScenario scene) async {
    await app.scenarios.save(
      previous: scene,
      name: scene.name,
      boundAccountId: null,
      settings: scene.settings,
    );
  }

  Future<void> _remove(ConnectionScenario scene) async {
    final strings = app.strings;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: LucideIcons.trash2,
        title: strings.get('scenario_delete_title'),
        subtitle: scene.name,
        content: Text(strings.get('scenario_delete_body')),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(strings.get('delete')),
          ),
        ],
      ),
    );
    if (confirmed == true) await app.scenarios.delete(scene);
  }

  Future<void> _update(ConnectionScenario scene) async {
    final catalog = app.scenarios.catalog;
    final current = catalog?.currentSettings;
    if (current == null || catalog?.currentAccountId == null) return;
    final binding = scene.boundAccountId == null
        ? null
        : catalog!.currentAccountId;
    final strings = app.strings;
    String account(String? id) =>
        app.profiles.where((p) => p.id == id).firstOrNull?.name ??
        strings.get('scenario_reference_missing');
    final fields = current.values.keys
        .where(
          (key) =>
              jsonEncode(current.values[key]) !=
              jsonEncode(scene.settings.values[key]),
        )
        .toList();
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: LucideIcons.save,
        title: strings.get('scenario_update_current'),
        subtitle: scene.name,
        content: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (binding == null)
                Text(strings.get('scenario_follow_account'))
              else
                _SceneChange(
                  label: strings.get('scenario_bound_account'),
                  from: account(scene.boundAccountId),
                  to: account(binding),
                ),
              if (fields.isNotEmpty) ...[
                const SizedBox(height: 8),
                ContentList(
                  children: [
                    for (final field in fields)
                      _SceneChange(
                        label: scenarioFieldLabel(strings, field),
                        from: scenarioValueLabel(
                          strings,
                          scene.settings.values,
                          field,
                        ),
                        to: scenarioValueLabel(strings, current.values, field),
                      ),
                  ],
                ),
              ],
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(strings.get('scenario_save')),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    await app.scenarios.save(
      previous: scene,
      name: scene.name,
      boundAccountId: binding,
      settings: current,
    );
  }

  Widget _scene(ConnectionScenario scene) {
    final strings = app.strings;
    final theme = Theme.of(context);
    final tokens = UsqueTokens.of(context);
    final state = app.scenarios.catalog;
    final busy = app.scenarios.busy;
    final selected = state?.selectedId == scene.id;
    final profile = scene.settings.toProfile(app.activeProfile);
    final account = app.profiles
        .where((a) => a.id == scene.boundAccountId)
        .firstOrNull;
    final invalid = state?.invalidIds.contains(scene.id) == true;
    final bindingMissing = scene.boundAccountId != null && account == null;
    final outputs = [
      if (profile.frontends.tunnel) strings.tunnelOutputLabel(theme.platform),
      if (profile.frontends.socks5) 'SOCKS5',
      if (profile.frontends.http) 'HTTP',
    ].join(' · ');
    final exit = profile.chainEnabled
        ? scenarioValueLabel(
            strings,
            scene.settings.values,
            profile.chainSource == ChainSource.vpnGate
                ? 'vpn_gate'
                : 'chain_exit',
            exitNames: _exitNames,
          )
        : 'WARP';
    final status = selected ? scenarioStatus(app) : null;
    // States that need attention; the plain facts follow in a quieter line.
    final tags = <Widget>[
      if (status != null)
        InlineStatus(
          label: strings.get(status.key),
          tone: status.tone,
          icon: status.icon,
        ),
      if (selected && state?.templateUpdated == true)
        InlineStatus(
          label: strings.get('scenario_template_updated'),
          tone: StatusTone.warning,
          icon: LucideIcons.refreshCcw,
        ),
      if (selected &&
          scene.boundAccountId != null &&
          !bindingMissing &&
          scene.boundAccountId != state?.currentAccountId)
        InlineStatus(
          label: strings.get('scenario_binding_modified'),
          tone: StatusTone.warning,
        ),
      if (bindingMissing)
        InlineStatus(
          label: strings.get('scenario_reference_missing'),
          tone: StatusTone.warning,
        ),
      if (invalid)
        InlineStatus(
          label: strings.get('scenario_settings_invalid'),
          tone: StatusTone.warning,
        ),
    ];
    final facts = Wrap(
      spacing: 16,
      runSpacing: 6,
      children: [
        _SceneFact(
          icon: profile.chainEnabled ? LucideIcons.route : LucideIcons.globe,
          label: exit,
        ),
        if (outputs.isNotEmpty)
          _SceneFact(icon: LucideIcons.monitorSmartphone, label: outputs),
        _SceneFact(
          icon: scene.boundAccountId == null
              ? LucideIcons.user
              : LucideIcons.link,
          label: scene.boundAccountId == null
              ? strings.get('scenario_follow_account')
              : '${strings.get('scenario_bound_account')}: ${account?.name ?? strings.get('not_available')}',
        ),
      ],
    );
    // Edit stays directly before Use so D-pad focus moves between them.
    final actions = Wrap(
      spacing: 6,
      alignment: WrapAlignment.end,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        TextButton.icon(
          onPressed: busy ? null : () => _edit(scene: scene),
          icon: const Icon(LucideIcons.pencil, size: 18),
          label: Text(strings.get('edit')),
        ),
        if (bindingMissing)
          TextButton.icon(
            key: ValueKey('scenario-rebind-${scene.id}'),
            onPressed: busy ? null : () => _unbind(scene),
            icon: const Icon(LucideIcons.user, size: 18),
            label: Text(strings.get('scenario_follow_account')),
          ),
        // A switch action, not a completed state: no check mark.
        TextButton.icon(
          key: ValueKey('scenario-use-${scene.id}'),
          onPressed: busy || invalid || bindingMissing
              ? null
              : () => previewAndUseScenario(context, app, scene),
          icon: const Icon(LucideIcons.arrowLeftRight, size: 18),
          label: Text(strings.get('scenario_use')),
        ),
        // The template's most likely next step once live settings diverge.
        if (selected && state?.modified == true)
          TextButton.icon(
            key: ValueKey('scenario-update-${scene.id}'),
            onPressed: busy ? null : () => _update(scene),
            icon: const Icon(LucideIcons.save, size: 18),
            label: Text(strings.get('scenario_update_current')),
          ),
        PopupMenuButton<String>(
          enabled: !busy,
          onSelected: (value) {
            switch (value) {
              case 'duplicate':
                unawaited(_edit(scene: scene, duplicate: true));
              case 'update':
                unawaited(_update(scene));
              case 'delete':
                unawaited(_remove(scene));
            }
          },
          itemBuilder: (_) => [
            PopupMenuItem(
              value: 'duplicate',
              child: Text(strings.get('scenario_duplicate')),
            ),
            PopupMenuItem(
              value: 'update',
              child: Text(strings.get('scenario_update_current')),
            ),
            PopupMenuItem(value: 'delete', child: Text(strings.get('delete'))),
          ],
        ),
      ],
    );
    // Rows share the Accounts grammar: selection wash and start marker, the
    // name above its tags, actions beside the block or under a hairline.
    return Semantics(
      selected: selected,
      container: true,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 24),
        decoration: BoxDecoration(
          color: selected
              ? theme.colorScheme.primary.withValues(alpha: tokens.tint * 0.55)
              : null,
          border: BorderDirectional(
            start: BorderSide(
              width: 3,
              color: selected ? theme.colorScheme.primary : Colors.transparent,
            ),
          ),
        ),
        child: LayoutBuilder(
          builder: (context, constraints) {
            final compact =
                constraints.maxWidth < 620 ||
                MediaQuery.textScalerOf(context).scale(14) > 21;
            final details = Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                // Selection is not application, so no check mark here.
                Icon(
                  LucideIcons.layers3,
                  size: 24,
                  color: selected
                      ? theme.colorScheme.primary
                      : theme.colorScheme.onSurfaceVariant,
                ),
                const SizedBox(width: 13),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        scene.name,
                        maxLines: compact ? 2 : 1,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.titleLarge,
                      ),
                      if (tags.isNotEmpty) ...[
                        const SizedBox(height: 8),
                        Wrap(spacing: 12, runSpacing: 6, children: tags),
                      ],
                      const SizedBox(height: 8),
                      facts,
                    ],
                  ),
                ),
              ],
            );
            if (!compact) {
              // Extra contextual actions wrap instead of squeezing the name.
              return Row(
                children: [
                  Expanded(child: details),
                  const SizedBox(width: 16),
                  ConstrainedBox(
                    constraints: BoxConstraints(
                      maxWidth: constraints.maxWidth * 0.55,
                    ),
                    child: actions,
                  ),
                ],
              );
            }
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                details,
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 14),
                  child: Divider(height: 1, color: tokens.hairline),
                ),
                Align(
                  alignment: AlignmentDirectional.centerEnd,
                  child: actions,
                ),
              ],
            );
          },
        ),
      ),
    );
  }

  /// The saved choices beside the confirmed running combination, which can
  /// differ until a switch is applied.
  Widget _summary() {
    final strings = app.strings, state = app.scenarios.catalog;
    final theme = Theme.of(context);
    final accountId = state?.currentAccountId ?? app.activeProfileId;
    final account = app.profiles.where((a) => a.id == accountId).firstOrNull;
    final appliedAccount = app.profiles
        .where((a) => a.id == state?.appliedAccountId)
        .firstOrNull;
    final appliedScene = state?.appliedSceneConfirmed == true
        ? state?.scenarios
                  .where((scene) => scene.id == state.appliedScenarioId)
                  .firstOrNull
                  ?.name ??
              strings.get('scenario_custom')
        : strings.get('scenario_unknown');
    Widget item(IconData icon, String label, String value, {Key? key}) =>
        Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(icon, size: 16, color: theme.colorScheme.onSurfaceVariant),
                const SizedBox(width: 6),
                Flexible(
                  child: Text(
                    label,
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: theme.colorScheme.onSurfaceVariant,
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 4),
            Text(value, key: key, style: theme.textTheme.titleSmall),
          ],
        );
    final facts = Wrap(
      spacing: 40,
      runSpacing: 12,
      children: [
        item(
          LucideIcons.layers3,
          strings.get('scenario_current'),
          state?.selected?.name ?? strings.get('scenario_custom'),
        ),
        item(
          LucideIcons.user,
          strings.get('scenario_current_account'),
          account?.name ?? strings.get('not_available'),
        ),
        if (state?.sessionId != null || app.snapshot.isConnected)
          item(
            LucideIcons.activity,
            strings.chain('current'),
            '${appliedAccount?.name ?? strings.get('not_available')} · $appliedScene',
            key: const ValueKey('profiles-running'),
          ),
      ],
    );
    // Refresh stays beside the facts at every width; it is small, and an
    // uncertain switch is resolved by refreshing before anything else.
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(child: facts),
            const SizedBox(width: 16),
            IconButton(
              tooltip: strings.get('settings_refresh'),
              onPressed: app.scenarios.busy ? null : app.scenarios.refresh,
              icon: const Icon(LucideIcons.refreshCw),
            ),
          ],
        ),
        if (state?.canRestore == true) ...[
          const SizedBox(height: 8),
          Align(
            alignment: AlignmentDirectional.centerStart,
            child: TextButton.icon(
              onPressed: app.scenarios.busy
                  ? null
                  : () => previewAndUseScenario(context, app, null),
              icon: const Icon(LucideIcons.history, size: 18),
              label: Text(strings.get('scenario_restore')),
            ),
          ),
        ],
      ],
    );
  }

  /// Engine and switch problems, above the content they concern.
  Widget? _banner() {
    final strings = app.strings, scenes = app.scenarios, state = scenes.catalog;
    final failed = !scenes.unconfirmed && state?.applyStatus == 'failed';
    final banners = [
      if (state?.currentSettingsError != null)
        WarningBanner(
          title: null,
          message: strings.get(scenarioErrorKey(state!.currentSettingsError)),
        ),
      if (scenes.errorCode != null || scenes.unconfirmed || failed)
        WarningBanner(
          title: null,
          danger: failed,
          message: strings.get(
            scenes.unconfirmed
                ? 'scenario_unknown'
                : failed
                ? 'scenario_failed'
                : scenarioErrorKey(scenes.errorCode),
          ),
        ),
    ];
    if (banners.isEmpty) return null;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (var i = 0; i < banners.length; i++) ...[
          if (i > 0) const SizedBox(height: 8),
          banners[i],
        ],
      ],
    );
  }

  /// The tab selector shares a row with the visible tab's create action.
  Widget _toolbar(ProfilesTab selected) {
    final strings = app.strings, scenes = app.scenarios;
    final tabs = SegmentedButton<ProfilesTab>(
      segments: [
        ButtonSegment(
          value: ProfilesTab.scenes,
          label: Text(strings.get('scenario_scenes')),
        ),
        ButtonSegment(
          value: ProfilesTab.accounts,
          label: Text(strings.get('scenario_accounts')),
        ),
      ],
      selected: {selected},
      onSelectionChanged: (values) => app.selectProfilesTab(values.first),
    );
    final create = selected == ProfilesTab.accounts
        ? AddAccountButton(controller: app)
        : FilledButton.icon(
            key: const ValueKey('scenario-create'),
            onPressed: scenes.busy || scenes.catalog?.currentSettings == null
                ? null
                : () => _edit(),
            icon: const Icon(LucideIcons.plus),
            label: Text(strings.get('scenario_create')),
          );
    return LayoutBuilder(
      builder: (context, constraints) {
        if (constraints.maxWidth < 560 ||
            MediaQuery.textScalerOf(context).scale(14) > 21) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              tabs,
              const SizedBox(height: 16),
              Align(alignment: AlignmentDirectional.centerEnd, child: create),
            ],
          );
        }
        return Row(
          children: [
            ConstrainedBox(
              constraints: const BoxConstraints(minWidth: 280),
              child: tabs,
            ),
            const Spacer(),
            const SizedBox(width: 16),
            create,
          ],
        );
      },
    );
  }

  Widget _scenes() {
    final strings = app.strings, scenes = app.scenarios, state = scenes.catalog;
    final theme = Theme.of(context);
    if (state == null) {
      return const Padding(
        padding: EdgeInsets.symmetric(vertical: 32),
        child: Center(child: CircularProgressIndicator()),
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // A fixed slot, so a running operation never shifts the list.
        SizedBox(
          height: 4,
          child: scenes.busy ? const LinearProgressIndicator() : null,
        ),
        const SizedBox(height: 12),
        if (state.scenarios.isEmpty)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 40, horizontal: 16),
            child: Column(
              children: [
                Icon(
                  LucideIcons.layers3,
                  size: 40,
                  color: theme.colorScheme.onSurfaceVariant,
                ),
                const SizedBox(height: 16),
                Text(
                  strings.get('scenario_empty'),
                  textAlign: TextAlign.center,
                  style: theme.textTheme.titleLarge,
                ),
                const SizedBox(height: 8),
                Text(
                  strings.get('scenario_empty_hint'),
                  textAlign: TextAlign.center,
                  style: theme.textTheme.bodyMedium?.copyWith(
                    color: theme.colorScheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          )
        else
          ContentList(children: state.scenarios.map(_scene).toList()),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final strings = app.strings, scenes = app.scenarios;
    final selected = scenes.supported ? app.profilesTab : ProfilesTab.accounts;
    return PageFrame(
      title: strings.get('profiles'),
      subtitle: strings.get('profiles_subtitle'),
      contentWidth: 880,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          BannerSlot(child: _banner()),
          if (scenes.supported) ...[
            _summary(),
            const SizedBox(height: 24),
            _toolbar(selected),
          ] else ...[
            // Without scenes there is nothing to switch to, only Accounts.
            HintText(strings.get('scenario_unsupported')),
            const SizedBox(height: 16),
            Align(
              alignment: AlignmentDirectional.centerEnd,
              child: AddAccountButton(controller: app),
            ),
          ],
          const SizedBox(height: 16),
          if (selected == ProfilesTab.accounts)
            AccountsScreen(controller: app, embedded: true)
          else
            _scenes(),
        ],
      ),
    );
  }
}

/// One plain fact about a scene, quieter than its status tags.
class _SceneFact extends StatelessWidget {
  const _SceneFact({required this.icon, required this.label});
  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final color = theme.colorScheme.onSurfaceVariant;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        ExcludeSemantics(child: Icon(icon, size: 16, color: color)),
        const SizedBox(width: 6),
        Flexible(
          child: Text(
            label,
            style: theme.textTheme.bodyMedium?.copyWith(color: color),
          ),
        ),
      ],
    );
  }
}

/// A settings difference: the field, then its old value leading to the new.
class _SceneChange extends StatelessWidget {
  const _SceneChange({
    required this.label,
    required this.from,
    required this.to,
  });
  final String label, from, to;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final muted = theme.colorScheme.onSurfaceVariant;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(label, style: theme.textTheme.titleSmall),
          const SizedBox(height: 4),
          Text.rich(
            TextSpan(
              children: [
                TextSpan(
                  text: from,
                  style: TextStyle(color: muted),
                ),
                TextSpan(
                  text: '  →  ',
                  style: TextStyle(color: muted),
                ),
                TextSpan(
                  text: to,
                  style: TextStyle(
                    color: theme.colorScheme.onSurface,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ],
            ),
            style: theme.textTheme.bodyMedium,
          ),
        ],
      ),
    );
  }
}

/// A short statement of what applying the preview will do.
class _SceneNote extends StatelessWidget {
  const _SceneNote({
    required this.icon,
    required this.message,
    required this.tone,
  });
  final IconData icon;
  final String message;
  final StatusTone tone;

  @override
  Widget build(BuildContext context) => Row(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      Padding(
        padding: const EdgeInsets.only(top: 2),
        child: ExcludeSemantics(
          child: Icon(icon, size: 18, color: statusToneColor(context, tone)),
        ),
      ),
      const SizedBox(width: 10),
      Expanded(
        child: Text(message, style: Theme.of(context).textTheme.bodyMedium),
      ),
    ],
  );
}

typedef SceneStatus = ({String key, StatusTone tone, IconData icon});

/// The selected scene's state with a tone and glyph that match its meaning:
/// selection alone is neither success nor a notice.
SceneStatus scenarioStatus(AppController app) {
  final key = scenarioStateKey(app);
  return switch (key) {
    'scenario_applied' => (
      key: key,
      tone: StatusTone.success,
      icon: LucideIcons.circleCheck,
    ),
    'scenario_failed' => (
      key: key,
      tone: StatusTone.danger,
      icon: LucideIcons.circleX,
    ),
    'scenario_modified' => (
      key: key,
      tone: StatusTone.warning,
      icon: LucideIcons.pencilLine,
    ),
    'scenario_unknown' => (
      key: key,
      tone: StatusTone.warning,
      icon: LucideIcons.circleQuestionMark,
    ),
    'scenario_pending' => (
      key: key,
      tone: StatusTone.neutral,
      icon: LucideIcons.clock,
    ),
    _ => (key: key, tone: StatusTone.brand, icon: LucideIcons.circleDot),
  };
}

String scenarioStateKey(AppController app) {
  final state = app.scenarios.catalog;
  if (app.scenarios.unconfirmed) return 'scenario_unknown';
  if (state?.modified == true) return 'scenario_modified';
  if (state?.applyStatus == 'failed') return 'scenario_failed';
  if (state?.appliedMatches == true) return 'scenario_applied';
  if (state?.applyStatus == 'applying' || state?.applyStatus == 'deferred') {
    return 'scenario_pending';
  }
  if ((state?.applyStatus == 'unknown' || app.scenarios.queryUnconfirmed) &&
      app.snapshot.isConnected) {
    return 'scenario_unknown';
  }
  return 'scenario_selected';
}

final _scenePreviewLocks = Expando<bool>();
Future<bool> previewAndUseScenario(
  BuildContext context,
  AppController app,
  ConnectionScenario? scene,
) async {
  if (_scenePreviewLocks[app] == true) return false;
  _scenePreviewLocks[app] = true;
  try {
    return await _previewAndUseScenario(context, app, scene);
  } finally {
    _scenePreviewLocks[app] = false;
  }
}

Future<bool> _previewAndUseScenario(
  BuildContext context,
  AppController app,
  ConnectionScenario? scene,
) async {
  final route = ModalRoute.of(context);
  bool canPresent() =>
      app.section == AppSection.profiles && (route?.isCurrent ?? true);
  final preview = scene == null
      ? await app.scenarios.previewRestore()
      : await app.scenarios.preview(scene);
  if (preview == null || !context.mounted || !canPresent()) return false;
  final strings = app.strings;
  final exitNames = await scenarioExitNames(
    app,
    settings: [
      preview.appliedSettings ?? preview.beforeSettings,
      preview.targetSettings,
    ],
  );
  if (!context.mounted || !canPresent()) return false;
  final managedFields = {
    ...preview.changedFields,
    ...preview.actualChangedFields.where(_managedScenarioFields.contains),
  };
  final globalFields = preview.actualChangedFields
      .where((field) => !_managedScenarioFields.contains(field))
      .toSet();
  String account(String? id) =>
      app.profiles.where((a) => a.id == id).firstOrNull?.name ??
      strings.get('scenario_reference_missing');
  final impact = switch (preview.applyKind == 'select'
      ? 'scenario_disconnected_hint'
      : preview.needsReconnect == null
      ? 'scenario_unknown'
      : preview.needsReconnect == true
      ? 'scenario_will_reconnect'
      : 'scenario_new_flows') {
    'scenario_will_reconnect' => (
      key: 'scenario_will_reconnect',
      tone: StatusTone.warning,
      icon: LucideIcons.refreshCw,
    ),
    'scenario_unknown' => (
      key: 'scenario_unknown',
      tone: StatusTone.warning,
      icon: LucideIcons.circleQuestionMark,
    ),
    final key => (key: key, tone: StatusTone.neutral, icon: LucideIcons.info),
  };
  final confirmed = await showDialog<bool>(
    context: context,
    builder: (context) => UsqueDialog(
      icon: LucideIcons.layers3,
      title: strings.get(
        preview.restore ? 'scenario_restore' : 'scenario_preview',
      ),
      subtitle: scene?.name,
      content: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: [
            // What applying does comes first; the field details follow.
            _SceneNote(
              icon: impact.icon,
              tone: impact.tone,
              message: strings.get(impact.key),
            ),
            if (preview.killSwitchOutputRemoved) ...[
              const SizedBox(height: 12),
              WarningBanner(
                key: const ValueKey('scenario-kill-switch-warning'),
                title: null,
                message: strings.get('scenario_kill_switch_output'),
              ),
            ],
            const SizedBox(height: 16),
            if (preview.accountChanged)
              _SceneChange(
                label: strings.get('scenario_account_change'),
                from: account(preview.previousAccountId),
                to: account(preview.accountId),
              )
            else
              Text(strings.get('scenario_keep_account')),
            if (managedFields.isEmpty && globalFields.isEmpty) ...[
              const SizedBox(height: 12),
              Text(strings.get('scenario_no_changes')),
            ],
            if (managedFields.isNotEmpty) ...[
              const SizedBox(height: 8),
              ContentList(
                children: [
                  for (final field in managedFields)
                    _SceneChange(
                      label: scenarioFieldLabel(strings, field),
                      from: scenarioValueLabel(
                        strings,
                        preview.appliedSettings ?? preview.beforeSettings,
                        field,
                        exitNames: exitNames,
                      ),
                      to: scenarioValueLabel(
                        strings,
                        preview.targetSettings,
                        field,
                        exitNames: exitNames,
                      ),
                    ),
                ],
              ),
            ],
            if (scenarioMap(preview.targetSettings['chain_exit'])['source'] ==
                    'proton_vpn' &&
                scenarioMap(
                      (preview.appliedSettings ??
                          preview.beforeSettings)['chain_exit'],
                    )['proton_server_id'] !=
                    scenarioMap(
                      preview.targetSettings['chain_exit'],
                    )['proton_server_id'])
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(
                  '${strings.proton('selected_server')} · ${strings.get('scenario_modified')}',
                ),
              ),
            if (globalFields.isNotEmpty) ...[
              const SizedBox(height: 20),
              ContentHeading(title: strings.get('scenario_inherited_changes')),
              const SizedBox(height: 4),
              for (final field in globalFields)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 6),
                  child: Text(scenarioFieldLabel(strings, field)),
                ),
            ],
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context, false),
          child: Text(strings.get('cancel')),
        ),
        FilledButton(
          key: const ValueKey('scenario-confirm-use'),
          onPressed: () => Navigator.pop(context, true),
          child: Text(strings.get('scenario_confirm_use')),
        ),
      ],
    ),
  );
  return confirmed == true ? app.scenarios.activate(preview) : false;
}

String scenarioFieldLabel(AppStrings strings, String field) =>
    strings.get(switch (field) {
      'account_id' || 'active_profile_id' => 'scenario_account_change',
      'frontends' => 'scenario_output',
      'frontends.tunnel' => 'tunnel_output',
      'frontends.socks5' => 'socks_listener',
      'frontends.http' => 'http_listener',
      'transport' => 'transport',
      'data_plane' => 'mode',
      'tls_fingerprint' => 'tls_fingerprint',
      'congestion_control' => 'nq_congestion',
      'endpoint.ipv4' => 'endpoint_ipv4',
      'endpoint.ipv6' => 'endpoint_ipv6',
      'endpoint.port' => 'port',
      'endpoint.sni' => 'sni',
      'endpoint.selection' => 'endpoint_selection',
      'endpoint.preset_region' => 'endpoint_section',
      'ip_policy' => 'ip_policy',
      'mtu' => 'mtu',
      'allow_lan' => 'allow_lan',
      'disable_quic' => 'disable_quic',
      'kill_switch' => 'kill_switch',
      'auto_connect' => 'auto_connect',
      'split_exclusions' || 'bypass_domains' => 'routing_protection',
      'proxy.socks5_listeners' => 'socks_listener',
      'proxy.http_listeners' => 'http_listener',
      'proxy.udp_idle_timeout_seconds' => 'scenario_connection',
      'routing' => 'routing_protection',
      'geo_direct_countries' => 'bypass_countries',
      'direct_dns' => 'nq_direct_dns',
      'warp_dns' => 'warp_dns_type',
      'dns_mode' ||
      'dns_servers' ||
      'proxy_dns_mode' ||
      'proxy_dns_servers' ||
      'proxy.dns_mode' ||
      'proxy.dns_servers' => 'dns',
      'proxy_system_proxy' || 'proxy.system_proxy' => 'home_system_proxy',
      'vpn_gate' || 'chain_exit' => 'scenario_exit',
      _ => 'scenario_connection',
    });

const _managedScenarioFields = {
  'frontends',
  'frontends.tunnel',
  'frontends.socks5',
  'frontends.http',
  'transport',
  'ip_policy',
  'allow_lan',
  'routing',
  'geo_direct_countries',
  'direct_dns',
  'warp_dns',
  'dns_mode',
  'dns_servers',
  'proxy_system_proxy',
  'proxy_dns_mode',
  'proxy_dns_servers',
  'proxy.system_proxy',
  'proxy.dns_mode',
  'proxy.dns_servers',
  'vpn_gate',
  'chain_exit',
};

final _protonNameCaches = Expando<_ScenarioProtonNames>();

class _ScenarioProtonNames {
  final requests = <String, Future<Map<String, String>>>{};
  Future<Map<String, String>> get(
    AppController app,
    String profileId,
    String revision,
  ) {
    final key = '$profileId:$revision:${app.chainLibraryRevision}';
    final existing = requests[key];
    if (existing != null) return existing;
    while (requests.length >= 32) {
      requests.remove(requests.keys.first);
    }
    final result = () async {
      final nodes = await loadCachedProtonServers(
        app,
        profileId,
        isCurrent: () => app.uiVisible,
      );
      return <String, String>{
        for (final node in nodes)
          if (node['id'] is String && node['name'] is String)
            'proton:$profileId:${node['id']}': node['name'] as String,
      };
    }();
    requests[key] = result;
    unawaited(
      result.then<void>(
        (value) {
          if (value.isEmpty) requests.remove(key);
        },
        onError: (Object _) {
          requests.remove(key);
        },
      ),
    );
    return result;
  }
}

Future<Map<String, String>> scenarioExitNames(
  AppController app, {
  Iterable<Map<String, Object?>> settings = const [],
}) async {
  final names = <String, String>{};
  try {
    final result = await app.chainProfile({'action': 'list'});
    if (result.error == null) {
      names.addEntries(
        result.profiles.map((entry) => MapEntry(entry.id, entry.name)),
      );
    }
  } on Object {
    // An unavailable local library does not change the selected exit or routes.
  }
  final cache = _protonNameCaches[app] ??= _ScenarioProtonNames();
  final profiles = <String, String>{};
  for (final value in settings) {
    final exit = scenarioMap(value['chain_exit']);
    if (exit['source'] == 'proton_vpn' &&
        exit['profile_id'] is String &&
        exit['revision'] is String &&
        exit['proton_server_id'] is String &&
        (exit['proton_server_id'] as String).isNotEmpty) {
      profiles[exit['profile_id'] as String] = exit['revision'] as String;
    }
  }
  for (final profile in profiles.entries) {
    try {
      names.addAll(await cache.get(app, profile.key, profile.value));
    } on Object {
      /* Keep a visible unavailable label; never show a raw server ID. */
    }
  }
  return names;
}

String scenarioErrorKey(String? code) => switch (code) {
  'SCENARIO_ACCOUNT_UNAVAILABLE' ||
  'SCENARIO_DEPENDENCY_UNAVAILABLE' ||
  'SCENARIO_ACCOUNT_MISSING' => 'scenario_reference_missing',
  'SCENARIO_VPN_PERMISSION_REQUIRED' => 'scenario_vpn_permission',
  'SCENARIO_RULES_UNAVAILABLE' => 'scenario_rules_missing',
  'SCENARIO_UNCONFIRMED' ||
  'SCENARIO_COMMIT_UNCONFIRMED' ||
  'SCENARIO_RESULT_UNKNOWN' => 'scenario_unknown',
  'SCENARIO_PLATFORM_UNSUPPORTED' => 'scenario_platform_unsupported',
  'SCENARIO_NOT_APPLIED' => 'scenario_not_applied',
  'SCENARIO_INVALID_SETTINGS' => 'scenario_settings_invalid',
  'SCENARIO_PROTECTION_UNAVAILABLE' => 'protection_handoff_unavailable',
  'SCENARIO_LIMIT' => 'scenario_limit',
  'SCENARIOS_UNSUPPORTED' => 'scenario_unsupported',
  _ => 'scenario_save_failed',
};

String scenarioValueLabel(
  AppStrings strings,
  Map<String, Object?> settings,
  String field, {
  Map<String, String> exitNames = const {},
}) {
  Object? value;
  if (field.startsWith('proxy.')) {
    value = settings[field.replaceFirst('proxy.', 'proxy_')];
  } else if (field.startsWith('frontends.')) {
    value = scenarioMap(settings['frontends'])[field.split('.').last];
  } else {
    value = settings[field];
  }
  if (value == null) return strings.get('not_available');
  if (value is bool) return strings.get(value ? 'on' : 'off');
  if (value is List) return value.isEmpty ? '—' : value.join(', ');
  if (value is Map) {
    if (field == 'routing') {
      return '${strings.get('scenario_rules_count').replaceAll('{count}', '${(value['rules'] as List? ?? []).length}')} · ${strings.get('routing_ads')}: ${strings.get(value['ads_enabled'] == true ? 'on' : 'off')}';
    }
    if (field == 'chain_exit' || field == 'vpn_gate') {
      if (value['enabled'] != true) return strings.get('off');
      if (field == 'vpn_gate') return 'VPN Gate';
      final source = value['source'] as String?;
      final label =
          ChainSource.values
              .where((item) => item.wire == source)
              .firstOrNull
              ?.label ??
          strings.get('not_available');
      final name = exitNames[value['profile_id']];
      final base = name == null ? label : '$label · $name';
      final serverId = value['proton_server_id'];
      if (source == 'proton_vpn' && serverId is String) {
        final serverName = serverId.isEmpty
            ? strings.proton('automatic_server')
            : exitNames['proton:${value['profile_id']}:$serverId'] ??
                  strings.get('not_available');
        return '$base · $serverName';
      }
      return base;
    }
    if (value.containsKey('mode')) {
      return [
        value['mode'],
        value['server_name'],
      ].whereType<String>().where((v) => v.isNotEmpty).join(' · ');
    }
    return strings.get('scenario_output');
  }
  return switch (value) {
    'auto' => strings.get('automatic'),
    'prefer_ipv4' => strings.get('prefer_ipv4'),
    'prefer_ipv6' => strings.get('prefer_ipv6'),
    'ipv4_only' => strings.get('ipv4_only'),
    'ipv6_only' => strings.get('ipv6_only'),
    'tunnel' || 'remote' => strings.get('proxy_dns_remote'),
    'local_configured' => strings.get('proxy_dns_configured'),
    'system' => strings.get('proxy_dns_system'),
    'edge_resolved' => strings.get('proxy_dns_edge_resolved'),
    'http3' => strings.get('http3'),
    'http2' => strings.get('http2'),
    _ => value.toString(),
  };
}

class ScenarioHomeLink extends StatelessWidget {
  const ScenarioHomeLink({required this.controller, super.key});
  final AppController controller;
  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: controller.scenarios,
    builder: (context, _) {
      if (!controller.scenarios.supported) return const SizedBox.shrink();
      final strings = controller.strings;
      return Align(
        alignment: AlignmentDirectional.centerStart,
        child: TextButton.icon(
          key: const ValueKey('home-scene-link'),
          onPressed: () => controller.openProfiles(ProfilesTab.scenes),
          icon: const Icon(LucideIcons.layers3, size: 18),
          label: Text(
            '${strings.get('scenario_current')}: ${controller.scenarios.catalog?.selected?.name ?? strings.get('scenario_custom')}',
          ),
        ),
      );
    },
  );
}
