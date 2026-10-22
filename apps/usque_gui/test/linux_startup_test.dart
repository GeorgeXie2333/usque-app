import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/app.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/profiles_screen.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';

import 'app_test.dart' show FakeEngineClient;

class _LockedStartupEngine extends FakeEngineClient
    implements StartupProfileClient {
  final validating = Completer<void>();
  final unlock = Completer<void>();
  int imports = 0;

  _LockedStartupEngine() {
    legacyProfilesImported = true;
    storedProfiles = [
      UsqueProfile.defaultProfile().copyWith(
        id: 'saved-account',
        name: 'Saved account',
        autoConnect: true,
      ),
    ];
    storedActiveProfileId = storedProfiles.single.id;
  }

  @override
  Future<ProfileCatalog> readStartupProfiles() async => ProfileCatalog(
    profiles: storedProfiles,
    activeProfileId: storedActiveProfileId,
    sharedNetwork: storedProfiles.single,
    // Even a legacy peer's readiness must not bypass the validation phase.
    identityStates: {storedActiveProfileId: ProfileIdentityState.ready},
  );

  @override
  Future<ProfileCatalog> importLegacyProfiles(
    List<UsqueProfile> profiles,
    String activeProfileId,
  ) async {
    imports++;
    if (!validating.isCompleted) validating.complete();
    await unlock.future;
    return super.importLegacyProfiles(profiles, activeProfileId);
  }
}

class _RetryingStartupEngine extends _LockedStartupEngine {
  final retryStarted = Completer<void>();
  final retryReply = Completer<void>();
  int attempts = 0;

  @override
  Future<ProfileCatalog> importLegacyProfiles(
    List<UsqueProfile> profiles,
    String activeProfileId,
  ) async {
    if (++attempts == 1) {
      throw const EngineException(
        'ENGINE_IPC_UNAVAILABLE',
        'Temporarily unavailable',
      );
    }
    if (!retryStarted.isCompleted) retryStarted.complete();
    await retryReply.future;
    return super.importLegacyProfiles(profiles, activeProfileId);
  }
}

class _StartupOrderEngine extends _LockedStartupEngine {
  bool updatesFollowedValidation = false;

  @override
  Future<UpdateCheckResult> checkForUpdates({bool manual = true}) {
    updatesFollowedValidation = validating.isCompleted;
    return super.checkForUpdates(manual: manual);
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(() {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    SharedPreferences.setMockInitialValues({
      'onboarding_complete': true,
      'update_checks_enabled': false,
      'locale': 'english',
    });
  });
  tearDown(() => debugDefaultTargetPlatformOverride = null);

  testWidgets('Linux opens a navigable shell while the keyring is waiting', (
    tester,
  ) async {
    final engine = _LockedStartupEngine();
    await tester.pumpWidget(UsqueBootstrap(engine: engine));
    // Deliberately never resolve the vault during these frames.
    for (var i = 0; i < 8; i++) {
      await tester.pump(const Duration(milliseconds: 16));
    }
    expect(engine.validating.isCompleted, isTrue);
    expect(find.byType(ShellScreen), findsOneWidget);
    expect(find.bySemanticsLabel('Loading Usque'), findsNothing);
    expect(engine.calls, isNot(contains('connect')));
    // Navigation and local application preferences remain usable.
    await tester.tap(find.text('Accounts').first);
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.byType(ProfilesScreen), findsOneWidget);
    final create = tester.widget<FilledButton>(
      find.widgetWithText(FilledButton, 'Add account'),
    );
    expect(create.onPressed, isNull);
    engine.unlock.complete();
    for (var i = 0; i < 8; i++) {
      await tester.pump(const Duration(milliseconds: 16));
    }
    expect(engine.calls.where((c) => c == 'connect'), hasLength(1));
    expect(find.text('Saved account'), findsWidgets);
    await tester.pumpWidget(const SizedBox.shrink());
    debugDefaultTargetPlatformOverride = null;
  });

  test(
    'startup metadata cannot grant readiness or start a connection',
    () async {
      final engine = _LockedStartupEngine();
      final controller = AppController(engine);
      addTearDown(controller.dispose);
      final initialized = controller.initialize();
      await engine.validating.future;
      expect(controller.initialized, isTrue);
      expect(controller.activeProfile.name, 'Saved account');
      expect(controller.profileIdentitiesLoading, isTrue);
      expect(controller.busy, isTrue);
      expect(controller.networkShortcutsLocked, isTrue);
      expect(
        controller.identityState('saved-account'),
        isNot(ProfileIdentityState.ready),
      );
      expect(engine.calls, isNot(contains('connect')));
      // A direct caller also shares the in-flight validation instead of starting
      // another migration or sending a connect with the metadata-only account.
      final connect = controller.connectOrDisconnect();
      await Future<void>.delayed(Duration.zero);
      expect(engine.imports, 1);
      expect(engine.calls, isNot(contains('connect')));
      engine.unlock.complete();
      await initialized;
      await connect;
      expect(controller.profileIdentitiesLoading, isFalse);
      expect(
        controller.identityState('saved-account'),
        ProfileIdentityState.ready,
      );
      expect(engine.calls.where((c) => c == 'connect'), hasLength(1));
      await controller.initialize();
      expect(controller.profileIdentitiesLoading, isFalse);
      expect(
        controller.identityState('saved-account'),
        ProfileIdentityState.ready,
      );
      expect(engine.imports, 1);
    },
  );

  test('late validation after disposal cannot auto-connect', () async {
    final engine = _LockedStartupEngine();
    final controller = AppController(engine);
    final initialized = controller.initialize();
    await engine.validating.future;
    controller.dispose();
    engine.unlock.complete();
    await initialized;
    expect(engine.calls, isNot(contains('connect')));
  });

  test(
    'required startup reads are queued before optional update checks',
    () async {
      SharedPreferences.setMockInitialValues({'onboarding_complete': true});
      final engine = _StartupOrderEngine();
      final controller = AppController(engine);
      addTearDown(controller.dispose);
      final initialized = controller.initialize();
      await engine.validating.future;
      await Future<void>.delayed(Duration.zero);
      expect(engine.updatesFollowedValidation, isTrue);
      engine.unlock.complete();
      await initialized;
    },
  );

  test(
    'catalog retry retains startup accounts and locks pending actions',
    () async {
      final engine = _RetryingStartupEngine()..unlock.complete();
      final controller = AppController(engine);
      addTearDown(controller.dispose);
      await controller.initialize();
      await engine.retryStarted.future.timeout(const Duration(seconds: 5));
      expect(controller.activeProfile.name, 'Saved account');
      expect(controller.profileIdentitiesLoading, isTrue);
      expect(controller.networkShortcutsLocked, isTrue);
      expect(engine.calls, isNot(contains('connect')));
      engine.retryReply.complete();
      await Future<void>.delayed(Duration.zero);
      expect(controller.activeProfile.name, 'Saved account');
      expect(controller.profileIdentitiesLoading, isFalse);
    },
  );
}
