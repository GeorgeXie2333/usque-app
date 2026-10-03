import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';

import 'app_test.dart' show FakeEngineClient;

class ReadbackFailureEngine extends FakeEngineClient {
  bool failReadback = false;

  @override
  Future<InitialIdentityState> initializeIdentity(
    UsqueProfile profile, {
    required String operationId,
    required IdentityProvisioningMethod method,
    bool resumeOnly = false,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) async {
    final state = await super.initializeIdentity(
      profile,
      operationId: operationId,
      method: method,
      resumeOnly: resumeOnly,
      licenseKey: licenseKey,
      teamName: teamName,
      callbackUri: callbackUri,
    );
    if (!resumeOnly) failReadback = true;
    return state;
  }

  @override
  Future<ProfileCatalog> importLegacyProfiles(
    List<UsqueProfile> profiles,
    String activeProfileId,
  ) async {
    if (failReadback) {
      failReadback = false;
      throw const EngineException(
        'ENGINE_IPC_UNAVAILABLE',
        'Interrupted readback',
      );
    }
    return super.importLegacyProfiles(profiles, activeProfileId);
  }
}

class TimedOutInitialEngine extends FakeEngineClient {
  int starts = 0;
  @override
  Future<InitialIdentityState> initializeIdentity(
    UsqueProfile profile, {
    required String operationId,
    required IdentityProvisioningMethod method,
    bool resumeOnly = false,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) async {
    if (resumeOnly) return getInitialIdentityState(profile.id);
    starts++;
    initialSetupState = InitialIdentityState(
      profileId: profile.id,
      operationId: operationId,
      phase: InitialIdentityPhase.pending,
    );
    throw TimeoutException('Native registration is still executing');
  }
}

class DeferredLoginEngine extends FakeEngineClient {
  final firstLogin = Completer<String?>();
  int logins = 0;
  @override
  Future<String?> beginZeroTrustLogin(String teamName) {
    logins++;
    return logins == 1
        ? firstLogin.future
        : Future.value('https://$teamName.cloudflareaccess.com/warp');
  }
}

class QueuedInitialEngine extends FakeEngineClient {
  final replies = <Completer<InitialIdentityState>>[];
  final entered = StreamController<void>.broadcast();
  @override
  Future<InitialIdentityState> initializeIdentity(
    UsqueProfile profile, {
    required String operationId,
    required IdentityProvisioningMethod method,
    bool resumeOnly = false,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) {
    if (resumeOnly) return getInitialIdentityState(profile.id);
    initialSetupState = InitialIdentityState(
      profileId: profile.id,
      operationId: operationId,
      phase: InitialIdentityPhase.pending,
    );
    final reply = Completer<InitialIdentityState>();
    replies.add(reply);
    entered.add(null);
    return reply.future;
  }
}

class DeletedInitialProfileEngine extends FakeEngineClient {
  DeletedInitialProfileEngine(this.errorCode);
  final String errorCode;
  static const removedId = '11111111-1111-4111-8111-111111111111';
  @override
  Future<InitialIdentityState> getInitialIdentityState(String profileId) async {
    if (profileId == removedId) {
      throw EngineException(errorCode, 'Account was deleted.');
    }
    return super.getInitialIdentityState(profileId);
  }
}

void main() {
  setUp(
    () => SharedPreferences.setMockInitialValues({
      'update_checks_enabled': false,
    }),
  );

  for (final code in ['PROFILE_NOT_FOUND', 'INITIAL_IDENTITY_STATE_FAILED']) {
    test(
      'deleted saved onboarding account recovers from $code without registration',
      () async {
        SharedPreferences.setMockInitialValues({
          'onboarding_profile_id': DeletedInitialProfileEngine.removedId,
          'onboarding_step': 3,
          'onboarding_terms_accepted': true,
          'update_checks_enabled': false,
        });
        final engine = DeletedInitialProfileEngine(code);
        final app = AppController(engine);
        addTearDown(app.dispose);
        await app.initialize();
        expect(app.onboardingStep, 0);
        expect(app.onboardingPhase, OnboardingPhase.idle);
        expect(
          (await SharedPreferences.getInstance()).getString(
            'onboarding_profile_id',
          ),
          isNull,
        );
        expect(engine.calls.where((call) => call == 'provision'), isEmpty);
      },
    );
  }

  for (final method in [
    IdentityProvisioningMethod.registerWithLicense,
    IdentityProvisioningMethod.zeroTrust,
  ]) {
    test(
      'readback failure preserves $method and resumes without registration',
      () async {
        final engine = ReadbackFailureEngine();
        final first = AppController(engine);
        await first.initialize();
        await first.setOnboardingTermsAccepted(true);
        expect(
          await first.finishOnboarding(
            method: method,
            licenseKey: 'test-license',
            teamName: 'example-team',
            callbackUri: 'test-callback',
          ),
          isFalse,
        );
        expect(engine.provisioned, isTrue);
        first.dispose();
        final restored = AppController(engine);
        addTearDown(restored.dispose);
        await restored.initialize();
        expect(restored.onboardingComplete, isFalse);
        expect(restored.onboardingPhase, OnboardingPhase.ready);
        expect(await restored.finishOnboarding(), isTrue);
        expect(engine.calls.where((call) => call == 'provision'), hasLength(1));
        expect(engine.lastProvisioningMethod, method);
        expect(
          restored.identityStatus(restored.activeProfileId).licenseState,
          method == IdentityProvisioningMethod.zeroTrust
              ? LicenseState.notApplicable
              : LicenseState.warpPlus,
        );
      },
    );
  }

  test('completion persistence failure retries only preferences', () async {
    final engine = FakeEngineClient();
    var permitWrite = false;
    final app = AppController(
      engine,
      onboardingCompletionWriter: () async => permitWrite,
    );
    addTearDown(app.dispose);
    await app.initialize();
    await app.setOnboardingTermsAccepted(true);
    expect(await app.finishOnboarding(), isFalse);
    expect(app.onboardingComplete, isFalse);
    expect(app.onboardingPhase, OnboardingPhase.ready);
    permitWrite = true;
    expect(await app.finishOnboarding(), isTrue);
    expect(engine.calls.where((call) => call == 'provision'), hasLength(1));
  });

  test(
    'clearing waits for an in-flight completion write and removes its result',
    () async {
      final engine = FakeEngineClient();
      final started = Completer<void>();
      final write = Completer<bool>();
      final app = AppController(
        engine,
        onboardingCompletionWriter: () {
          started.complete();
          return write.future;
        },
      );
      addTearDown(app.dispose);
      await app.initialize();
      await app.setOnboardingTermsAccepted(true);
      final finishing = app.finishOnboarding();
      await started.future;
      final clearing = app.clearAllData();
      await Future<void>.delayed(Duration.zero);
      expect(engine.provisioned, isTrue);
      write.complete(true);
      expect(await finishing, isFalse);
      expect(await clearing, isTrue);
      expect(app.onboardingComplete, isFalse);
      expect((await SharedPreferences.getInstance()).getKeys(), isEmpty);
    },
  );

  test(
    'timeout reconciles pending operation without another registration',
    () async {
      final engine = TimedOutInitialEngine();
      final app = AppController(engine);
      addTearDown(app.dispose);
      await app.initialize();
      await app.setOnboardingTermsAccepted(true);
      expect(await app.finishOnboarding(), isFalse);
      expect(app.onboardingPhase, OnboardingPhase.reconciling);
      expect(await app.finishOnboarding(), isFalse);
      expect(engine.starts, 1);
      engine.provisioned = true;
      engine.initialSetupState = InitialIdentityState(
        profileId: app.activeProfileId,
        phase: InitialIdentityPhase.completed,
      );
      expect(await app.refreshInitialIdentityState(), isTrue);
      expect(app.onboardingPhase, OnboardingPhase.ready);
      expect(await app.finishOnboarding(), isTrue);
      expect(engine.starts, 1);
    },
  );

  test(
    'initial registration requires terms and current VPN authorization',
    () async {
      debugDefaultTargetPlatformOverride = TargetPlatform.android;
      addTearDown(() => debugDefaultTargetPlatformOverride = null);
      final engine = FakeEngineClient()
        ..permissionState = const OnboardingPermissionState(
          vpnGranted: false,
          notification: OnboardingNotificationPermission.notGranted,
        );
      final app = AppController(engine);
      addTearDown(app.dispose);
      await app.initialize();
      expect(await app.finishOnboarding(), isFalse);
      await app.setOnboardingTermsAccepted(true);
      expect(await app.finishOnboarding(), isFalse);
      expect(engine.calls.where((call) => call == 'provision'), isEmpty);
    },
  );

  test(
    'old registration result cannot unlock a new submission after reset',
    () async {
      final engine = QueuedInitialEngine();
      addTearDown(engine.entered.close);
      final app = AppController(engine);
      addTearDown(app.dispose);
      await app.initialize();
      await app.setOnboardingTermsAccepted(true);
      var entry = engine.entered.stream.first;
      final first = app.finishOnboarding();
      await entry;
      expect(await app.clearAllData(), isTrue);
      await app.setOnboardingTermsAccepted(true);
      entry = engine.entered.stream.first;
      final next = app.finishOnboarding();
      await entry;
      engine.replies.first.complete(
        InitialIdentityState(
          profileId: app.activeProfileId,
          phase: InitialIdentityPhase.completed,
        ),
      );
      expect(await first, isFalse);
      expect(app.onboardingOperationPending, isTrue);
      engine.initialSetupState = InitialIdentityState(
        profileId: app.activeProfileId,
        phase: InitialIdentityPhase.completed,
      );
      engine.replies.last.complete(engine.initialSetupState!);
      expect(await next, isTrue);
    },
  );

  test('late old login is retired without cancelling a newer owner', () async {
    final engine = DeferredLoginEngine();
    final app = AppController(engine);
    addTearDown(app.dispose);
    final firstOwner = app.createZeroTrustLoginOwner();
    final first = app.beginZeroTrustLogin('first-team', owner: firstOwner);
    final cancelled = expectLater(
      first,
      throwsA(
        isA<EngineException>().having(
          (error) => error.code,
          'code',
          'ZERO_TRUST_LOGIN_CANCELLED',
        ),
      ),
    );
    await Future<void>.delayed(Duration.zero);
    app.releaseZeroTrustLoginOwner(firstOwner);
    final nextOwner = app.createZeroTrustLoginOwner();
    final next = app.beginZeroTrustLogin('second-team', owner: nextOwner);
    engine.firstLogin.complete('https://first-team.cloudflareaccess.com/warp');
    await cancelled;
    expect(await next, 'https://second-team.cloudflareaccess.com/warp');
    final previousCancellations = engine.zeroTrustCancelCount;
    await app.cancelZeroTrustLogin(owner: firstOwner);
    expect(engine.zeroTrustCancelCount, previousCancellations);
    engine.pendingZeroTrustCallback = 'second-callback';
    expect(
      await app.consumeZeroTrustCallback(owner: nextOwner),
      'second-callback',
    );
  });
}
