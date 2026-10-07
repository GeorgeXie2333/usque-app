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

    fun fromSnapshot(
        phase: String?,
        vpnFrontendActive: Boolean,
    ): Presentation =
        if (!vpnFrontendActive) {
            inactive()
        } else {
            fromActiveVpnPhase(phase)
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
