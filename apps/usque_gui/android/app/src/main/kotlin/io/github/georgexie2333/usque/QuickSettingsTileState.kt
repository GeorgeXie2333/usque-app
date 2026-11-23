package io.github.georgexie2333.usque

/** Platform-free mapping between the engine state machine and the three Android tile states. */
internal object QuickSettingsTileState {
    enum class State {
        ACTIVE,
        INACTIVE,
        UNAVAILABLE,
    }

    data class Presentation(
        val state: State,
        val subtitle: String?,
    )

    fun active(subtitle: String = "connected") = Presentation(State.ACTIVE, subtitle)

    fun inactive(subtitle: String? = "disconnected") = Presentation(State.INACTIVE, subtitle)

    // SystemUI can cache a tile after the app process dies. Temporary work must
    // stay clickable so another tap can query the service and recover control.
    fun pending(subtitle: String) = inactive(subtitle)

    enum class ToggleAction {
        CONNECT,
        DISCONNECT,
        OPEN_APP_PROTECTION_ACTIVE,
        ERROR_PROXY_ONLY,
    }

    private fun needsProtectionControls(
        phase: String?,
        vpnFrontendActive: Boolean,
        protectionRetained: Boolean,
    ) = protectionRetained && (!vpnFrontendActive || phase == "error" || phase == "disconnected")

    fun toggleAction(
        phase: String?,
        anyFrontendActive: Boolean,
        vpnFrontendActive: Boolean,
        protectionRetained: Boolean,
    ): ToggleAction =
        when {
            needsProtectionControls(phase, vpnFrontendActive, protectionRetained) -> {
                ToggleAction.OPEN_APP_PROTECTION_ACTIVE
            }

            vpnFrontendActive && phase != "error" && phase != "disconnected" -> {
                ToggleAction.DISCONNECT
            }

            anyFrontendActive && !vpnFrontendActive -> {
                ToggleAction.ERROR_PROXY_ONLY
            }

            else -> {
                ToggleAction.CONNECT
            }
        }

    fun fromSnapshot(
        phase: String?,
        vpnFrontendActive: Boolean,
        protectionRetained: Boolean = false,
    ): Presentation =
        when {
            needsProtectionControls(phase, vpnFrontendActive, protectionRetained) -> active("open_app")
            vpnFrontendActive -> fromActiveVpnPhase(phase)
            else -> inactive()
        }

    private fun fromActiveVpnPhase(phase: String?): Presentation =
        when (phase) {
            "connected", "degraded" -> active()
            "preparing", "connectingH3", "connectingH2" -> active("connecting")
            "reconnecting" -> active("reconnecting")
            "disconnecting" -> active("disconnecting")
            "disconnected", "error" -> inactive()
            else -> pending("checking")
        }
}
