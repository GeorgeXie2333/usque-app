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

    fun active(subtitle: String = "Connected") = Presentation(State.ACTIVE, subtitle)

    fun inactive(subtitle: String? = "Disconnected") = Presentation(State.INACTIVE, subtitle)

    fun pending(subtitle: String) = Presentation(State.UNAVAILABLE, subtitle)

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
            "preparing", "connectingH3", "connectingH2" -> pending("Connecting")
            "reconnecting" -> pending("Reconnecting")
            "disconnecting" -> pending("Disconnecting")
            "disconnected", "error" -> inactive()
            else -> pending("Checking")
        }
}
