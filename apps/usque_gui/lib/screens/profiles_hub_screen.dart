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
import '../widgets/profiles_status.dart';
import '../widgets/proton_vpn_panel.dart' show loadCachedProtonServers;
import '../widgets/usque_dialog.dart';
import '../widgets/zero_trust_endpoint_warning.dart';
import 'profiles_screen.dart'
    show AccountsPage, AccountsScreen, AddAccountButton;
import 'scenario_editor_screen.dart';

class ProfilesScreen extends StatefulWidget {
  const ProfilesScreen({
    required this.controller,
    this.gateObservation = false,
    this.onOpenAccounts,
    super.key,
  });
  final bool gateObservation;
  final AppController controller;
  final VoidCallback? onOpenAccounts;
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
        danger: true,
        content: Text(strings.get('scenario_delete_body')),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton(
            style: dangerButtonStyle(context),
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
        app.profiles
            .where((p) => p.id == id)
            .map(strings.accountName)
            .firstOrNull ??
        strings.get('scenario_reference_missing');
    final fields = current.values.keys
        .where(
          (key) =>
              jsonEncode(current.values[key]) !=
              jsonEncode(scene.settings.values[key]),
        )
        .expand((key) {
          if (key != 'endpoint') return [key];
          final before = scenarioMap(scene.settings.values[key]);
          final after = scenarioMap(current.values[key]);
          return {...before.keys, ...after.keys}
              .where((field) => before[field] != after[field])
              .map((field) => 'endpoint.$field');
        })
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

  /// The selected scene's bound account no longer matches the current one.
  bool _bindingDiffers(ConnectionScenario scene) {
    final account = scene.boundAccountId;
    return account != null &&
        app.profiles.any((a) => a.id == account) &&
        account != app.scenarios.catalog?.currentAccountId;
  }

  /// Using the selected scene again only matters when its saved template and
  /// the current or applied state have drifted apart, or the last result is
  /// not a success.
  bool _selectionNeedsUse(ConnectionScenario scene) =>
      app.scenarios.catalog?.templateUpdated == true ||
      _bindingDiffers(scene) ||
      const {
        'scenario_modified',
        'scenario_failed',
        'scenario_unknown',
      }.contains(scenarioStateKey(app));

  /// What sets one scene apart: the exit, the endpoint, any non-default
  /// transport or IP version, and a bound account. Defaults stay unstated.
  List<Widget> _facts(ConnectionScenario scene, UsqueProfile? account) {
    final strings = app.strings;
    final profile = scene.settings.toProfile(app.activeProfile);
    return [
      _SceneFact(
        icon: profile.chainEnabled ? LucideIcons.route : LucideIcons.globe,
        label: profile.chainEnabled
            ? scenarioValueLabel(
                strings,
                scene.settings.values,
                profile.chainSource == ChainSource.vpnGate
                    ? 'vpn_gate'
                    : 'chain_exit',
                exitNames: _exitNames,
              )
            : 'WARP',
      ),
      _SceneFact(
        icon: LucideIcons.mapPin,
        label: scenarioFact(
          strings,
          strings.get('endpoint_section'),
          scenarioEndpointLabel(strings, profile),
        ),
      ),
      if (profile.transport != TransportPolicy.automatic)
        _SceneFact(
          icon: LucideIcons.arrowDownUp,
          label: scenarioFact(
            strings,
            strings.get('transport'),
            strings.get(profile.transport.name),
          ),
        ),
      if (profile.ipPolicy != IpPolicy.automatic)
        _SceneFact(
          icon: LucideIcons.network,
          label: scenarioValueLabel(
            strings,
            scene.settings.values,
            'ip_policy',
          ),
        ),
      if (scene.boundAccountId != null)
        _SceneFact(
          icon: LucideIcons.user,
          label: scenarioFact(
            strings,
            strings.get('scenario_bound_account'),
            account == null
                ? strings.get('not_available')
                : strings.accountName(account),
          ),
        ),
    ];
  }

  Widget _menuItem(IconData icon, String label, {bool danger = false}) {
    final color = danger ? statusToneColor(context, StatusTone.danger) : null;
    return Row(
      children: [
        Icon(icon, size: 18, color: color),
        const SizedBox(width: 12),
        Flexible(
          child: Text(
            label,
            style: color == null ? null : TextStyle(color: color),
          ),
        ),
      ],
    );
  }

  Widget _scene(ConnectionScenario scene) {
    final strings = app.strings;
    final theme = Theme.of(context);
    final tokens = UsqueTokens.of(context);
    final state = app.scenarios.catalog;
    final busy = app.scenarios.busy;
    final selected = state?.selectedId == scene.id;
    final account = app.profiles
        .where((a) => a.id == scene.boundAccountId)
        .firstOrNull;
    final invalid = state?.invalidIds.contains(scene.id) == true;
    final bindingMissing = scene.boundAccountId != null && account == null;
    // The row marks the list choice; the selection's Engine state, drift and
    // their actions live in the status block above the list.
    final tags = <Widget>[
      if (selected)
        InlineStatus(
          label: strings.get('scenario_selected'),
          tone: StatusTone.brand,
          icon: LucideIcons.circleDot,
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
      children: _facts(scene, account),
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
        // A switch action, not a completed state: no check mark. The selected
        // scene offers it again only when using it would change something.
        if (!selected || _selectionNeedsUse(scene))
          TextButton.icon(
            key: ValueKey('scenario-use-${scene.id}'),
            onPressed: busy || invalid || bindingMissing
                ? null
                : () => previewAndUseScenario(context, app, scene),
            icon: const Icon(LucideIcons.arrowLeftRight, size: 18),
            label: Text(strings.get('scenario_use')),
          ),
        PopupMenuButton<String>(
          key: ValueKey('scenario-menu-${scene.id}'),
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
              child: _menuItem(
                LucideIcons.copy,
                strings.get('scenario_duplicate'),
              ),
            ),
            PopupMenuItem(
              value: 'update',
              child: _menuItem(
                LucideIcons.save,
                strings.get('scenario_update_current'),
              ),
            ),
            const PopupMenuDivider(),
            PopupMenuItem(
              value: 'delete',
              child: _menuItem(
                LucideIcons.trash2,
                strings.get('delete'),
                danger: true,
              ),
            ),
          ],
        ),
      ],
    );
    // Rows share the Accounts selection marks (wash and start marker) and put
    // the name above its tags; actions sit beside the block or under a
    // hairline.
    return Semantics(
      selected: selected,
      container: true,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 20),
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

  void _openAccounts() {
    if (widget.onOpenAccounts case final open?) {
      open();
      return;
    }
    Navigator.of(context).push<void>(
      MaterialPageRoute<void>(
        settings: const RouteSettings(name: '/profiles/accounts'),
        builder: (_) => AccountsPage(controller: app),
      ),
    );
  }

  Widget _createButton() {
    final scenes = app.scenarios;
    return FilledButton.icon(
      key: const ValueKey('scenario-create'),
      onPressed: scenes.busy || scenes.catalog?.currentSettings == null
          ? null
          : () => _edit(),
      icon: const Icon(LucideIcons.plus),
      label: Text(app.strings.get('scenario_create')),
    );
  }

  /// The selection, its Engine state and the account and connection behind
  /// it, with the actions that resolve a drifted or uncertain selection.
  Widget _status(ScenarioCatalog state) {
    final strings = app.strings, scenes = app.scenarios;
    final theme = Theme.of(context);
    final muted = theme.colorScheme.onSurfaceVariant;
    final busy = scenes.busy;
    final scene = state.selected;
    final status = scenarioStatus(app);
    final running = profilesRunningLabel(app);
    // The heading already says "selected"; a tag repeats it only as a state.
    final tags = <Widget>[
      if (status.key != 'scenario_selected')
        InlineStatus(
          label: strings.get(status.key),
          tone: status.tone,
          icon: status.icon,
        ),
      if (scene != null && state.templateUpdated)
        InlineStatus(
          label: strings.get('scenario_template_updated'),
          tone: StatusTone.warning,
          icon: LucideIcons.refreshCcw,
        ),
      if (scene != null && _bindingDiffers(scene))
        InlineStatus(
          label: strings.get('scenario_binding_modified'),
          tone: StatusTone.warning,
        ),
    ];
    Widget readout(
      IconData icon,
      String label,
      String value, {
      required Key key,
    }) => Wrap(
      // Long labels or large text put the value on the next line instead
      // of pushing it past the card's edge.
      spacing: 8,
      runSpacing: 2,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            ExcludeSemantics(child: Icon(icon, size: 16, color: muted)),
            const SizedBox(width: 6),
            Flexible(
              child: Text(
                label,
                style: theme.textTheme.bodySmall?.copyWith(color: muted),
              ),
            ),
          ],
        ),
        Text(value, key: key, style: theme.textTheme.titleSmall),
      ],
    );
    final actions = <Widget>[
      // Live settings left the template: keep them in it, or as a new scene.
      if (scene != null && state.modified) ...[
        FilledButton.tonalIcon(
          key: ValueKey('scenario-update-${scene.id}'),
          onPressed: busy ? null : () => _update(scene),
          icon: const Icon(LucideIcons.save, size: 18),
          label: Text(strings.get('scenario_update_current')),
        ),
        TextButton.icon(
          key: const ValueKey('scenario-save-as'),
          onPressed: busy || state.currentSettings == null
              ? null
              : () => _edit(),
          icon: const Icon(LucideIcons.copyPlus, size: 18),
          label: Text(strings.get('scenario_save_as')),
        ),
      ],
      if (state.canRestore)
        TextButton.icon(
          key: const ValueKey('scenario-restore'),
          onPressed: busy
              ? null
              : () => previewAndUseScenario(context, app, null),
          icon: const Icon(LucideIcons.history, size: 18),
          label: Text(strings.get('scenario_restore')),
        ),
      // Leaving the selection previews Usque's default scene settings.
      if (state.selectedId != null)
        TextButton.icon(
          key: const ValueKey('scenario-use-custom'),
          onPressed: busy || state.currentAccountId == null
              ? null
              : () => previewAndUseCustom(context, app),
          icon: const Icon(LucideIcons.rotateCcw, size: 18),
          label: Text(strings.get('scenario_use_custom')),
        ),
    ];
    final narrow = MediaQuery.sizeOf(context).width < 600;
    return Panel(
      key: const ValueKey('profiles-scene-status'),
      padding: EdgeInsetsDirectional.fromSTEB(
        narrow ? 16 : 20,
        16,
        narrow ? 8 : 12,
        narrow ? 16 : 18,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    const SizedBox(height: 4),
                    Text(
                      strings.get('scenario_current'),
                      style: theme.textTheme.bodySmall?.copyWith(color: muted),
                    ),
                    const SizedBox(height: 6),
                    Wrap(
                      spacing: 14,
                      runSpacing: 6,
                      crossAxisAlignment: WrapCrossAlignment.center,
                      children: [
                        Text(
                          scene?.name ?? strings.get('scenario_custom'),
                          key: const ValueKey('profiles-selected-scene'),
                          style: theme.textTheme.headlineSmall,
                        ),
                        ...tags,
                      ],
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 8),
              // Small and always beside the state: an uncertain switch is
              // resolved by refreshing before anything else.
              IconButton(
                tooltip: strings.get('settings_refresh'),
                onPressed: busy ? null : scenes.refresh,
                icon: const Icon(LucideIcons.refreshCw),
              ),
            ],
          ),
          const SizedBox(height: 12),
          Padding(
            padding: const EdgeInsetsDirectional.only(end: 8),
            child: Wrap(
              spacing: 28,
              runSpacing: 8,
              children: [
                readout(
                  LucideIcons.user,
                  strings.get('scenario_current_account'),
                  profilesSelectedAccountLabel(app),
                  key: const ValueKey('profiles-selected-account'),
                ),
                if (running != null)
                  readout(
                    LucideIcons.activity,
                    strings.chain('current'),
                    running,
                    key: const ValueKey('profiles-running'),
                  ),
              ],
            ),
          ),
          if (actions.isNotEmpty) ...[
            const SizedBox(height: 16),
            Wrap(spacing: 8, runSpacing: 8, children: actions),
          ],
        ],
      ),
    );
  }

  /// The list's label and size; creating a scene is a page action.
  Widget _listHeading(ScenarioCatalog? state) {
    final theme = Theme.of(context);
    final count = state?.scenarios.length ?? 0;
    return Row(
      children: [
        Text(
          app.strings.get('scenario_scenes'),
          style: theme.textTheme.titleSmall,
        ),
        if (count > 0) ...[
          const SizedBox(width: 8),
          Text(
            '$count',
            style: theme.textTheme.titleSmall?.copyWith(
              color: theme.colorScheme.onSurfaceVariant,
            ),
          ),
        ],
      ],
    );
  }

  Widget _scenes() {
    final strings = app.strings, scenes = app.scenarios, state = scenes.catalog;
    final theme = Theme.of(context);
    if (state == null) {
      if (scenes.queryUnconfirmed) {
        return Center(
          child: TextButton.icon(
            key: const ValueKey('scenario-retry'),
            onPressed: scenes.refresh,
            icon: const Icon(LucideIcons.refreshCw),
            label: Text(strings.get('retry')),
          ),
        );
      }
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
                const SizedBox(height: 4),
                // What a scene leaves to the device, before the first one.
                Text(
                  strings.get('scenario_global_hint'),
                  textAlign: TextAlign.center,
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: theme.colorScheme.onSurfaceVariant,
                  ),
                ),
                const SizedBox(height: 24),
                _createButton(),
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
    final available = scenes.supported;
    final state = scenes.catalog;
    return PageFrame(
      title: strings.get('profiles'),
      subtitle: strings.get('profiles_subtitle'),
      contentWidth: 880,
      actions: [
        if (available) ...[
          TextButton.icon(
            key: const ValueKey('profiles-manage-accounts'),
            onPressed: _openAccounts,
            icon: const Icon(LucideIcons.users, size: 18),
            label: Text(strings.get('profiles_manage_accounts')),
          ),
          // An empty library carries its only create action in the list.
          if (state == null || state.scenarios.isNotEmpty) _createButton(),
        ],
      ],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          BannerSlot(child: profilesOperationBanner(app)),
          if (available) ...[
            if (state != null) ...[_status(state), const SizedBox(height: 28)],
            _listHeading(state),
          ] else ...[
            // Without scenes there is nothing to switch to, only Accounts.
            HintText(strings.get('scenario_unsupported')),
            const SizedBox(height: 16),
            Align(
              alignment: AlignmentDirectional.centerEnd,
              child: AddAccountButton(controller: app),
            ),
          ],
          SizedBox(height: available ? 8 : 16),
          if (!available) AccountsScreen(controller: app) else _scenes(),
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
  final scenes = app.scenarios, state = scenes.catalog;
  if (scenes.unconfirmed) return 'scenario_unknown';
  // Without a session there is no application result to await: a saved
  // selection waits for the next connection. A failed read, an unresolved
  // switch or an outcome the owner reports as unknown or failed still shows.
  final idle =
      state != null &&
      state.sessionId == null &&
      !app.snapshot.isConnected &&
      !app.snapshot.isTransitional;
  if (state != null && !scenes.runtimeConfirmed) {
    // A saved-configuration read is authoritative for the selection and its
    // drift, never for a running connection.
    if (idle && !state.runtimeObserved && !scenes.queryUnconfirmed) {
      return state.modified ? 'scenario_modified' : 'scenario_selected';
    }
    return 'scenario_unknown';
  }
  if (state?.modified == true) return 'scenario_modified';
  if (state?.applyStatus == 'failed') return 'scenario_failed';
  if (state?.applyStatus == 'unknown') return 'scenario_unknown';
  if (state?.appliedMatches == true) return 'scenario_applied';
  if (state?.applyStatus == 'applying') return 'scenario_pending';
  if (state?.applyStatus == 'deferred') {
    return idle ? 'scenario_selected' : 'scenario_pending';
  }
  if ((state?.applyStatus == 'unknown' || app.scenarios.queryUnconfirmed) &&
      app.snapshot.isConnected) {
    return 'scenario_unknown';
  }
  return 'scenario_selected';
}

final _scenePreviewLocks = Expando<bool>();

/// Previews and applies [scene], or restores the previous settings when null.
Future<bool> previewAndUseScenario(
  BuildContext context,
  AppController app,
  ConnectionScenario? scene,
) => _previewAndUse(
  context,
  app,
  scene,
  () => scene == null
      ? app.scenarios.previewRestore()
      : app.scenarios.preview(scene),
);

/// Previews and applies Custom: no selected scene, with Usque's default
/// scene settings for the current account.
Future<bool> previewAndUseCustom(BuildContext context, AppController app) =>
    _previewAndUse(context, app, null, app.scenarios.previewReset);

Future<bool> _previewAndUse(
  BuildContext context,
  AppController app,
  ConnectionScenario? scene,
  Future<ScenarioPreview?> Function() load,
) async {
  if (_scenePreviewLocks[app] == true) return false;
  _scenePreviewLocks[app] = true;
  Future<void> Function()? releaseControl;
  try {
    try {
      releaseControl = await app.acquireControlOperation();
    } on Object {
      app.scenarios.previewUnavailable();
      return false;
    }
    if (!context.mounted) return false;
    return await _previewAndUseScenario(context, app, scene, load);
  } finally {
    await app.releaseControlOperation(releaseControl);
    _scenePreviewLocks[app] = false;
  }
}

Future<bool> _previewAndUseScenario(
  BuildContext context,
  AppController app,
  ConnectionScenario? scene,
  Future<ScenarioPreview?> Function() load,
) async {
  final route = ModalRoute.of(context);
  bool canPresent() =>
      app.section == AppSection.profiles && (route?.isCurrent ?? true);
  final preview = await load();
  if (preview == null || !context.mounted || !canPresent()) return false;
  if (preview.values['zero_trust_endpoint_warning'] == true) {
    final acknowledged = await showDialog<bool>(
      context: context,
      barrierDismissible: false,
      builder: (_) => ZeroTrustEndpointWarning(strings: app.strings),
    );
    if (acknowledged != true || !context.mounted || !canPresent()) return false;
  }
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
      app.profiles
          .where((a) => a.id == id)
          .map(strings.accountName)
          .firstOrNull ??
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
        preview.reset
            ? 'scenario_use_custom'
            : preview.restore
            ? 'scenario_restore'
            : 'scenario_preview',
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
            if (preview.reset) ...[
              const SizedBox(height: 16),
              Text(
                strings.get('scenario_custom_hint'),
                key: const ValueKey('scenario-custom-hint'),
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
          child: Text(
            strings.get(
              preview.reset ? 'scenario_use_custom' : 'scenario_confirm_use',
            ),
          ),
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
      'endpoint.preset_region' => 'endpoint_preset_country',
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
      'proxy.dns_mode' ||
      'proxy.dns_servers' => 'dns',
      'proxy.system_proxy' => 'home_system_proxy',
      'vpn_gate' || 'chain_exit' => 'scenario_exit',
      _ => 'scenario_connection',
    });

const _managedScenarioFields = {
  'endpoint.selection',
  'endpoint.preset_region',
  'endpoint.ipv4',
  'endpoint.ipv6',
  'endpoint.port',
  'endpoint.sni',
  'transport',
  'ip_policy',
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

/// One labelled fact with the locale's own colon and spacing, such as a
/// full-width colon in Chinese and Japanese or a spaced one in French.
String scenarioFact(AppStrings strings, String label, String value) => strings
    .get('scenario_fact')
    .replaceAllMapped(
      RegExp(r'\{(label|value)\}'),
      (match) => match[1] == 'label' ? label : value,
    );

/// A scene's endpoint in one short value: the selection mode, a preset's
/// country, or the first manual address.
String scenarioEndpointLabel(AppStrings strings, UsqueProfile profile) {
  if (profile.endpointSelection == EndpointSelection.automatic) {
    return strings.get('endpoint_automatic');
  }
  if (profile.endpointPresetRegion case final region?) {
    return strings.get('endpoint_preset_${region.name}');
  }
  final address = [
    profile.endpointIpv4,
    profile.endpointIpv6,
  ].map((value) => value.trim()).where((value) => value.isNotEmpty);
  return address.firstOrNull ?? strings.get('endpoint_custom');
}

String scenarioValueLabel(
  AppStrings strings,
  Map<String, Object?> settings,
  String field, {
  Map<String, String> exitNames = const {},
}) {
  // Bounded previews can omit the prior settings; absence is not Custom mode.
  if (field.startsWith('endpoint.') && settings['endpoint'] is! Map) {
    return strings.get('not_available');
  }
  Object? value;
  if (field.startsWith('endpoint.')) {
    value = scenarioMap(
      settings[field.split('.').first],
    )[field.split('.').last];
  } else {
    value = settings[field];
  }
  if (field == 'endpoint.preset_region') {
    return value == null
        ? strings.get('endpoint_manual')
        : strings.get('endpoint_preset_$value');
  }
  if (field == 'endpoint.selection') {
    return strings.get(
      value == 'automatic' ? 'endpoint_automatic' : 'endpoint_custom',
    );
  }
  if (value == null) return strings.get('not_available');
  if (value is bool) return strings.get(value ? 'on' : 'off');
  if (value is List) return value.isEmpty ? '—' : value.join(', ');
  if (value is Map) {
    if (field == 'chain_exit' || field == 'vpn_gate') {
      if (value['enabled'] != true) return strings.get('off');
      if (field == 'vpn_gate') return 'VPN Gate';
      final source = value['source'] as String?;
      final label =
          ChainSource.values
              .where((item) => item.wire == source)
              .map(strings.chainSourceLabel)
              .firstOrNull ??
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
    return strings.get('not_available');
  }
  return switch (value) {
    'auto' => strings.get('automatic'),
    'prefer_ipv4' => strings.get('prefer_ipv4'),
    'prefer_ipv6' => strings.get('prefer_ipv6'),
    'ipv4_only' => strings.get('ipv4_only'),
    'ipv6_only' => strings.get('ipv6_only'),
    'http3' => strings.get('http3'),
    'http2' => strings.get('http2'),
    _ => value.toString(),
  };
}
