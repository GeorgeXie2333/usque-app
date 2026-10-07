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
    fun `disconnected and error phases remain actionable`() {
        listOf("disconnected", "error").forEach { phase ->
            assertEquals(
                QuickSettingsTileState.State.INACTIVE,
                QuickSettingsTileState.fromSnapshot(phase, vpnFrontendActive = true).state,
            )
        }
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
