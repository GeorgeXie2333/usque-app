package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

class QuickSettingsTileStateTest {
    @Test
    fun `connected phases are active`() {
        listOf("connected", "degraded").forEach { phase ->
            assertEquals(
                QuickSettingsTileState.State.ACTIVE,
                QuickSettingsTileState.fromSnapshot(phase, vpnFrontendActive = true).state,
            )
        }
    }

    @Test
    fun `active VPN transitions stay actionable with their status`() {
        mapOf(
            "preparing" to "connecting",
            "connectingH3" to "connecting",
            "connectingH2" to "connecting",
            "reconnecting" to "reconnecting",
            "disconnecting" to "disconnecting",
        ).forEach { (phase, subtitle) ->
            assertEquals(
                QuickSettingsTileState.Presentation(QuickSettingsTileState.State.ACTIVE, subtitle),
                QuickSettingsTileState.fromSnapshot(phase, vpnFrontendActive = true),
            )
        }
    }

    @Test
    fun `temporary control work stays actionable`() {
        listOf("working", "checking").forEach { subtitle ->
            assertEquals(
                QuickSettingsTileState.Presentation(QuickSettingsTileState.State.INACTIVE, subtitle),
                QuickSettingsTileState.pending(subtitle),
            )
        }
    }

    @Test
    fun `inactive VPN frontend stays off through every phase`() {
        listOf(
            "connected",
            "degraded",
            "preparing",
            "connectingH3",
            "connectingH2",
            "reconnecting",
            "disconnecting",
            "disconnected",
            "error",
            "unknown",
            null,
        ).forEach { phase ->
            assertEquals(
                QuickSettingsTileState.inactive(),
                QuickSettingsTileState.fromSnapshot(phase, vpnFrontendActive = false),
            )
        }
    }

    @Test
    fun `disconnected and unretained error phases remain inactive and reconnect on tap`() {
        listOf("disconnected", "error").forEach { phase ->
            assertEquals(
                QuickSettingsTileState.State.INACTIVE,
                QuickSettingsTileState.fromSnapshot(phase, vpnFrontendActive = true, protectionRetained = false).state,
            )
            assertEquals(
                QuickSettingsTileState.ToggleAction.CONNECT,
                QuickSettingsTileState.toggleAction(
                    phase = phase,
                    anyFrontendActive = true,
                    vpnFrontendActive = true,
                    protectionRetained = false,
                ),
            )
        }
    }

    @Test
    fun `error phase with retained protection stays active and opens app instead of disconnecting`() {
        assertEquals(
            QuickSettingsTileState.Presentation(QuickSettingsTileState.State.ACTIVE, "open_app"),
            QuickSettingsTileState.fromSnapshot("error", vpnFrontendActive = true, protectionRetained = true),
        )
        assertEquals(
            QuickSettingsTileState.ToggleAction.OPEN_APP_PROTECTION_ACTIVE,
            QuickSettingsTileState.toggleAction(
                phase = "error",
                anyFrontendActive = true,
                vpnFrontendActive = true,
                protectionRetained = true,
            ),
        )
    }

    @Test
    fun `a proxy handoff with an inherited tunnel keeps the protection controls until release`() {
        for (phase in listOf("preparing", "error", "disconnected")) {
            assertEquals(
                QuickSettingsTileState.active("open_app"),
                QuickSettingsTileState.fromSnapshot(phase, false, true),
            )
            assertEquals(
                QuickSettingsTileState.ToggleAction.OPEN_APP_PROTECTION_ACTIVE,
                QuickSettingsTileState.toggleAction(phase, true, false, true),
            )
        }
        assertEquals(QuickSettingsTileState.inactive(), QuickSettingsTileState.fromSnapshot("error", false, false))
        assertEquals(
            QuickSettingsTileState.ToggleAction.ERROR_PROXY_ONLY,
            QuickSettingsTileState.toggleAction("error", true, false, false),
        )
    }

    @Test
    fun `active non-error vpn phases disconnect on tap and proxy-only reports error`() {
        listOf("connected", "degraded", "preparing", "reconnecting").forEach { phase ->
            assertEquals(
                QuickSettingsTileState.ToggleAction.DISCONNECT,
                QuickSettingsTileState.toggleAction(
                    phase = phase,
                    anyFrontendActive = true,
                    vpnFrontendActive = true,
                    protectionRetained = false,
                ),
            )
        }
        assertEquals(
            QuickSettingsTileState.ToggleAction.ERROR_PROXY_ONLY,
            QuickSettingsTileState.toggleAction(
                phase = "connected",
                anyFrontendActive = true,
                vpnFrontendActive = false,
                protectionRetained = false,
            ),
        )
    }

    @Test
    fun `unknown phase remains actionable to query the authority`() {
        listOf(null, "unknown").forEach { phase ->
            assertEquals(
                QuickSettingsTileState.Presentation(QuickSettingsTileState.State.INACTIVE, "checking"),
                QuickSettingsTileState.fromSnapshot(phase, vpnFrontendActive = true),
            )
        }
    }

    @Test
    fun `proxy-only connection does not light or block the VPN tile`() {
        assertEquals(
            QuickSettingsTileState.State.INACTIVE,
            QuickSettingsTileState.fromSnapshot("connected", vpnFrontendActive = false).state,
        )
    }

    @Test
    fun `terminal and reconnect transitions change the tile presentation`() {
        val connecting =
            QuickSettingsTileState.fromSnapshot("preparing", vpnFrontendActive = true)
        val anotherConnectingPhase =
            QuickSettingsTileState.fromSnapshot("connectingH3", vpnFrontendActive = true)
        val connected =
            QuickSettingsTileState.fromSnapshot("connected", vpnFrontendActive = true)
        val reconnecting =
            QuickSettingsTileState.fromSnapshot("reconnecting", vpnFrontendActive = true)

        assertEquals(connecting, anotherConnectingPhase)
        assertEquals(QuickSettingsTileState.State.ACTIVE, connected.state)
        assertEquals(QuickSettingsTileState.State.ACTIVE, reconnecting.state)
        assertNotEquals(connecting, connected)
        assertNotEquals(connected, reconnecting)
    }
}
