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
    fun `transition phases reject repeated tile taps`() {
        listOf("preparing", "connectingH3", "connectingH2", "reconnecting", "disconnecting")
            .forEach { phase ->
                assertEquals(
                    QuickSettingsTileState.State.UNAVAILABLE,
                    QuickSettingsTileState.fromSnapshot(phase, vpnFrontendActive = true).state,
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
    fun `unknown phase waits for an authoritative snapshot`() {
        assertEquals(
            QuickSettingsTileState.State.UNAVAILABLE,
            QuickSettingsTileState.fromSnapshot(null, vpnFrontendActive = true).state,
        )
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
        assertEquals(QuickSettingsTileState.State.UNAVAILABLE, reconnecting.state)
        assertNotEquals(connecting, connected)
        assertNotEquals(connected, reconnecting)
    }
}
