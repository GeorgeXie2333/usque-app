enum InitialIdentityPhase { idle, pending, interrupted, completed, failed }

enum OnboardingPhase {
  idle,
  submitting,
  reconciling,
  ready,
  interrupted,
  failed,
  unavailable,
}

class InitialIdentityState {
  const InitialIdentityState({
    required this.profileId,
    required this.phase,
    this.operationId = '',
    this.errorCode = '',
    this.reused = false,
  });

  factory InitialIdentityState.fromMap(Map<Object?, Object?> value) {
    final phase = switch (value['phase']) {
      'idle' => InitialIdentityPhase.idle,
      'pending' => InitialIdentityPhase.pending,
      'interrupted' => InitialIdentityPhase.interrupted,
      'completed' => InitialIdentityPhase.completed,
      'failed' => InitialIdentityPhase.failed,
      _ => throw const FormatException('Invalid initial identity state'),
    };
    return InitialIdentityState(
      profileId: value['profile_id'] as String,
      operationId: value['operation_id'] as String? ?? '',
      phase: phase,
      errorCode: value['error_code'] as String? ?? '',
      reused: value['reused'] == true,
    );
  }

  final String profileId;
  final String operationId;
  final InitialIdentityPhase phase;
  final String errorCode;
  final bool reused;
}

enum OnboardingNotificationPermission {
  notRequired,
  granted,
  notGranted,
  notRequested,
}

class OnboardingPermissionState {
  const OnboardingPermissionState({
    required this.vpnGranted,
    required this.notification,
  });

  factory OnboardingPermissionState.fromMap(Map<Object?, Object?> value) =>
      OnboardingPermissionState(
        vpnGranted: value['vpnGranted'] == true,
        notification: switch (value['notification']) {
          'notRequired' => OnboardingNotificationPermission.notRequired,
          'granted' => OnboardingNotificationPermission.granted,
          'notGranted' => OnboardingNotificationPermission.notGranted,
          'notRequested' => OnboardingNotificationPermission.notRequested,
          _ => throw const FormatException('Invalid notification permission'),
        },
      );

  final bool vpnGranted;
  final OnboardingNotificationPermission notification;
}
