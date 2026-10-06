package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Test
import java.io.IOException

class TileControlPolicyTest {
    @Test
    fun recoveryFallbackPreservesTheSavedPresentation() {
        assertEquals(QuickSettingsTileState.active(), TileControlPolicy.recoveryPresentation { true })
        assertEquals(QuickSettingsTileState.inactive(), TileControlPolicy.recoveryPresentation { false })
    }

    @Test
    fun failedRecoveryReadKeepsTheTileUnknownWithoutCrashingTheVpnProcess() {
        listOf(IOException("Policy is unavailable"), IllegalStateException("Policy is corrupt")).forEach { failure ->
            val presentation = TileControlPolicy.recoveryPresentation { throw failure }
            assertEquals(QuickSettingsTileState.pending("checking"), presentation)
            assertEquals(QuickSettingsTileState.State.UNAVAILABLE, presentation.state)
        }
    }

    @Test
    fun tileRefreshAndToggleNeverLoadFullDiagnostics() {
        val compact = mapOf("phase" to "connected", "tile_vpn_active" to true)
        listOf(UsqueVpnService.MSG_TILE_SNAPSHOT, UsqueVpnService.MSG_TILE_TOGGLE).forEach { request ->
            val reply =
                TileControlPolicy.snapshotFor(
                    request,
                    tileSnapshot = { compact },
                    fullSnapshot = { error("Tile replies must not load the native diagnostic provider") },
                )
            assertSame(compact, reply)
            assertEquals(
                QuickSettingsTileState.active(),
                QuickSettingsTileState.fromSnapshot(reply["phase"] as String, reply["tile_vpn_active"] as Boolean),
            )
        }
    }

    @Test
    fun uiAndCleanupRepliesRetainTheFullSnapshot() {
        val full = mapOf("phase" to "error", "pending_cleanup" to true, "direct_dns_configuration" to "invalid")
        listOf(UsqueVpnService.MSG_SNAPSHOT, UsqueVpnService.MSG_DISCONNECT, UsqueVpnService.MSG_CLEAR_ALL_DATA)
            .forEach { request ->
                val reply =
                    TileControlPolicy.snapshotFor(
                        request,
                        tileSnapshot = { error("UI and cleanup replies need the full snapshot") },
                        fullSnapshot = { full },
                    )
                assertSame(full, reply)
                assertEquals(true, reply["pending_cleanup"])
                assertEquals("invalid", reply["direct_dns_configuration"])
            }
    }
}
