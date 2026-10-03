import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/onboarding_models.dart';
import 'package:usque/screens/onboarding_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/common.dart';
import 'package:usque/widgets/zero_trust_enrollment_editor.dart';

import 'app_test.dart' show FakeEngineClient;
import 'ui_workflow_test.dart' show workflowHost;

Future<void> _continue(WidgetTester tester) async {
  final button = find.widgetWithText(FilledButton, 'Continue');
  await tester.ensureVisible(button);
  await tester.tap(button);
  await tester.pumpAndSettle();
}

Future<void> _identityStep(WidgetTester tester) async {
  await _continue(tester);
  await _continue(tester);
  final terms = find.byType(CheckboxListTile);
  await tester.ensureVisible(terms);
  await tester.tap(terms);
  await tester.pumpAndSettle();
  await _continue(tester);
}

class _CallbackFailureEngine extends FakeEngineClient {
  @override
  Future<String?> consumeZeroTrustCallback() async =>
      throw PlatformException(code: 'ENGINE_UNAVAILABLE', message: 'private');
}

class _PermissionScenarioEngine extends FakeEngineClient {
  int preparations = 0;
  final reply = Completer<OnboardingPermissionState>();

  @override
  Future<OnboardingPermissionState> prepareOnboardingPermissions() async {
    preparations++;
    return permissionState = await reply.future;
  }
}

void main() {
  setUp(() => SharedPreferences.setMockInitialValues(<String, Object>{}));
  testWidgets(
    'pending setup permits a read-only check but blocks an actual busy operation',
    (tester) async {
      final engine = FakeEngineClient()
        ..initialSetupState = const InitialIdentityState(
          profileId: UsqueProfile.defaultProfileId,
          phase: InitialIdentityPhase.pending,
        );
      final app = AppController(engine)
        ..localePreference = LocalePreference.english
        ..onboardingStep = 3
        ..onboardingTermsAccepted = true
        ..onboardingPhase = OnboardingPhase.reconciling;
      var disposed = false;
      addTearDown(() {
        if (!disposed) app.dispose();
      });
      await tester.pumpWidget(
        workflowHost(app, home: OnboardingScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      final check = find.widgetWithText(
        FilledButton,
        app.strings.get('onboarding_check_result'),
      );
      expect(tester.widget<FilledButton>(check).onPressed, isNotNull);
      final methods = tester.widget<IdentityProvisioningMethodSelector>(
        find.byType(IdentityProvisioningMethodSelector),
      );
      expect(methods.enabled, isFalse);
      await tester.ensureVisible(check);
      await tester.tap(check);
      await tester.pumpAndSettle();
      expect(app.onboardingPhase, OnboardingPhase.reconciling);
      expect(engine.provisioned, isFalse);
      app.busy = true;
      await tester.pumpWidget(
        workflowHost(app, home: OnboardingScreen(controller: app)),
      );
      await tester.pump();
      expect(tester.widget<FilledButton>(check).onPressed, isNull);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      app.dispose();
      disposed = true;
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );
  for (final granted in [false, true]) {
    testWidgets(
      'Android setup waits for VPN permission: granted=$granted',
      (tester) async {
        final engine = _PermissionScenarioEngine()
          ..permissionState = const OnboardingPermissionState(
            vpnGranted: false,
            notification: OnboardingNotificationPermission.notRequested,
          );
        final app = AppController(engine)
          ..localePreference = LocalePreference.english;
        addTearDown(app.dispose);
        await tester.pumpWidget(
          workflowHost(app, home: OnboardingScreen(controller: app)),
        );
        await tester.pumpAndSettle();
        await _continue(tester);
        final authorize = find.widgetWithText(
          FilledButton,
          app.strings.get('onboarding_grant_continue'),
        );
        await tester.ensureVisible(authorize);
        await tester.tap(authorize);
        await tester.pump();
        expect(engine.preparations, 1);
        expect(tester.widget<FilledButton>(authorize).onPressed, isNull);
        expect(find.text('System permissions'), findsOneWidget);
        expect(engine.provisioned, isFalse);
        expect(engine.lastConnectedProfile, isNull);
        engine.reply.complete(
          OnboardingPermissionState(
            vpnGranted: granted,
            notification: OnboardingNotificationPermission.notGranted,
          ),
        );
        await tester.pumpAndSettle();
        expect(
          find.text('Cloudflare terms'),
          granted ? findsOneWidget : findsNothing,
        );
        if (!granted) {
          expect(
            find.descendant(
              of: find.byType(InlineStatus),
              matching: find.text(app.strings.get('onboarding_vpn_required')),
            ),
            findsOneWidget,
          );
          expect(tester.widget<FilledButton>(authorize).onPressed, isNotNull);
        }
        expect(engine.lastConnectedProfile, isNull);
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
      },
      variant: TargetPlatformVariant.only(TargetPlatform.android),
    );
  }

  testWidgets(
    'revoked Android VPN permission returns to its saved step',
    (tester) async {
      final engine = FakeEngineClient();
      final app = AppController(engine)
        ..localePreference = LocalePreference.english
        ..onboardingStep = 3
        ..onboardingTermsAccepted = true;
      addTearDown(app.dispose);
      await app.refreshOnboardingPermissions();
      await tester.pumpWidget(
        workflowHost(app, home: OnboardingScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      expect(find.text('Set up WARP account'), findsOneWidget);
      engine.permissionState = const OnboardingPermissionState(
        vpnGranted: false,
        notification: OnboardingNotificationPermission.notGranted,
      );
      await app.refreshOnboardingPermissions();
      await tester.pumpAndSettle();
      expect(find.text('System permissions'), findsOneWidget);
      expect(app.onboardingStep, 1);
      expect(app.onboardingTermsAccepted, isTrue);
      expect(engine.provisioned, isFalse);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets(
    'a saved Zero Trust identity completes without asking for another callback',
    (tester) async {
      final engine = FakeEngineClient()
        ..storedIdentityStatuses = {
          UsqueProfile.defaultProfileId: const ProfileIdentityStatus(
            state: ProfileIdentityState.ready,
            provider: IdentityProvider.zeroTrust,
            organization: 'example-team',
            licenseState: LicenseState.notApplicable,
          ),
        };
      final app = AppController(engine)
        ..localePreference = LocalePreference.english
        ..onboardingStep = 3
        ..onboardingTermsAccepted = true
        ..onboardingPhase = OnboardingPhase.ready;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(app, home: OnboardingScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      expect(find.byType(IdentityProvisioningMethodSelector), findsNothing);
      expect(find.byType(TextField), findsNothing);
      final complete = find.widgetWithText(
        FilledButton,
        app.strings.get('onboarding_continue_existing'),
      );
      await tester.ensureVisible(complete);
      await tester.tap(complete);
      await tester.pumpAndSettle();
      expect(app.onboardingComplete, isTrue);
      expect(engine.calls.where((call) => call == 'provision'), isEmpty);
      expect(engine.lastZeroTrustCallback, isNull);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'an interrupted registration requires its explicit confirmation',
    (tester) async {
      final engine = FakeEngineClient()
        ..initialSetupState = const InitialIdentityState(
          profileId: UsqueProfile.defaultProfileId,
          phase: InitialIdentityPhase.interrupted,
        );
      final app = AppController(engine)
        ..localePreference = LocalePreference.english
        ..onboardingStep = 3
        ..onboardingTermsAccepted = true
        ..onboardingPhase = OnboardingPhase.interrupted;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(app, home: OnboardingScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.text('Cloudflare Zero Trust'));
      await tester.tap(find.text('Cloudflare Zero Trust'));
      await tester.pumpAndSettle();
      final team = find.widgetWithText(TextField, 'Organization team name');
      await tester.ensureVisible(team);
      await tester.enterText(team, 'example-team');
      final callback = find.widgetWithText(TextField, 'Login return link');
      await tester.ensureVisible(callback);
      await tester.enterText(
        callback,
        'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=test',
      );
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(engine.provisioned, isFalse);
      final retry = find.widgetWithText(
        TextButton,
        app.strings.get('onboarding_retry_registration'),
      );
      await tester.ensureVisible(retry);
      await tester.tap(retry);
      await tester.pumpAndSettle();
      expect(find.byType(AlertDialog), findsOneWidget);
      await tester.tap(find.widgetWithText(TextButton, 'Cancel'));
      await tester.pumpAndSettle();
      expect(engine.provisioned, isFalse);
      await tester.tap(retry);
      await tester.pumpAndSettle();
      await tester.tap(
        find.widgetWithText(
          FilledButton,
          app.strings.get('onboarding_retry_registration'),
        ),
      );
      await tester.pumpAndSettle();
      expect(engine.calls.where((call) => call == 'provision'), hasLength(1));
      expect(
        engine.lastProvisioningMethod,
        IdentityProvisioningMethod.zeroTrust,
      );
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );
  testWidgets(
    'rapid identity back and forward keeps one enrollment editor',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1280, 1000);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      final app = AppController(FakeEngineClient())
        ..localePreference = LocalePreference.english;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(
          app,
          reducedMotion: false,
          home: OnboardingScreen(controller: app),
        ),
      );
      await tester.pumpAndSettle();
      await _identityStep(tester);
      await tester.tap(find.text('Cloudflare Zero Trust'));
      await tester.pumpAndSettle();
      for (var i = 0; i < 3; i++) {
        await tester.ensureVisible(find.widgetWithText(OutlinedButton, 'Back'));
        await tester.tap(find.widgetWithText(OutlinedButton, 'Back'));
        await tester.pump(const Duration(milliseconds: 100));
        await tester.ensureVisible(
          find.widgetWithText(FilledButton, 'Continue'),
        );
        await tester.tap(find.widgetWithText(FilledButton, 'Continue'));
        await tester.pump(const Duration(milliseconds: 100));
        expect(find.byType(ZeroTrustEnrollmentEditor), findsOneWidget);
        expect(tester.takeException(), isNull);
      }
      await tester.pumpAndSettle();
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'all setup forms fit a wide short viewport at 200 percent',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(900, 375);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      final app = AppController(FakeEngineClient())
        ..localePreference = LocalePreference.english;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(app, scale: 2, home: OnboardingScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await _identityStep(tester);
      for (final method in [
        'Use a WARP License Key',
        'Cloudflare Zero Trust',
      ]) {
        await tester.ensureVisible(find.text(method));
        await tester.tap(find.text(method));
        await tester.pumpAndSettle();
        final finish = find.widgetWithText(FilledButton, 'Finish setup');
        await tester.ensureVisible(finish);
        await tester.pumpAndSettle();
        expect(finish.hitTestable(), findsOneWidget);
        expect(tester.takeException(), isNull);
      }
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'terms open the three provider-specific official policies',
    (tester) async {
      final opened = <Uri>[];
      final app = AppController(FakeEngineClient())
        ..localePreference = LocalePreference.english;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: OnboardingScreen(
            controller: app,
            externalLinkLauncher: (uri) async {
              opened.add(uri);
              return true;
            },
          ),
        ),
      );
      await tester.pumpAndSettle();
      await _continue(tester);
      await _continue(tester);
      for (final key in [
        'onboarding_application_terms',
        'onboarding_personal_privacy',
        'onboarding_zero_trust_privacy',
      ]) {
        final link = find.widgetWithText(TextButton, app.strings.get(key));
        await tester.ensureVisible(link);
        await tester.tap(link);
        await tester.pumpAndSettle();
      }
      expect(opened.map((uri) => uri.toString()), [
        'https://www.cloudflare.com/application/terms/',
        'https://www.cloudflare.com/application/privacypolicy/',
        'https://www.cloudflare.com/privacypolicy/',
      ]);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'automatic callback failure stays visible and permits manual input',
    (tester) async {
      final app = AppController(_CallbackFailureEngine())
        ..localePreference = LocalePreference.english;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(app, home: OnboardingScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      await _identityStep(tester);
      await tester.ensureVisible(find.text('Cloudflare Zero Trust'));
      await tester.tap(find.text('Cloudflare Zero Trust'));
      await tester.pumpAndSettle();
      final team = find.widgetWithText(TextField, 'Organization team name');
      await tester.ensureVisible(team);
      await tester.enterText(team, 'example-team');
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('engine_unavailable')), findsOneWidget);
      expect(find.textContaining('private'), findsNothing);
      expect(tester.takeException(), isNull);
      final callback = find.widgetWithText(TextField, 'Login return link');
      await tester.ensureVisible(callback);
      await tester.enterText(
        callback,
        'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=test',
      );
      await tester.pumpAndSettle();
      final finish = tester.widget<FilledButton>(
        find.widgetWithText(FilledButton, 'Finish setup'),
      );
      expect(finish.onPressed, isNotNull);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );
}
