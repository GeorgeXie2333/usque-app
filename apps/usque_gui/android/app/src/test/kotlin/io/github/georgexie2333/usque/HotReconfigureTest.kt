package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.atomic.AtomicReference

class HotReconfigureTest {
    @Test
    fun rejectedNativeUpdateRestoresConfirmedProfileAndKeepsLaterIntentSeparate() {
        val previous = """{"id":"account","routing":{"rules":[]}}"""
        val requested = """{"id":"account","routing":{"rules":[{"action":"direct"}]}}"""
        val active = AtomicReference<String?>(requested)
        val mode = AtomicReference<String?>("vpn")
        assertTrue(VpnReconfigure.restoreRejectedProfile(requested, previous, "vpn", active, mode))
        assertEquals(previous, active.get())
        assertEquals("vpn", mode.get())
        assertEquals(-11, NativeEngine.RECONFIGURE_REJECTED)
    }

    @Test
    fun lateRejectedReplyCannotOverwriteANewerProfileOrMode() {
        val rejected = String(charArrayOf('o', 'l', 'd'))
        val latest = String(charArrayOf('o', 'l', 'd'))
        val active = AtomicReference<String?>(latest)
        val mode = AtomicReference<String?>("socks5")
        assertFalse(VpnReconfigure.restoreRejectedProfile(rejected, "previous", "vpn", active, mode))
        assertEquals(latest, active.get())
        assertEquals("socks5", mode.get())
    }

    @Test
    fun msgReconfigureTunnelFalseModeVpnTearsDownTun() {
        val profileJson = """{"mode":"vpn","frontends":{"tunnel":false,"socks5":true,"http":true}}"""
        assertTrue(VpnReconfigure.shouldTearDownTun(UsqueVpnService.MSG_RECONFIGURE, profileJson))
        assertFalse(VpnReconfigure.tunnelFrontendEnabled(profileJson))
    }

    @Test
    fun msgReconfigureTunnelTrueModeVpnKeepsTun() {
        val profileJson = """{"mode":"vpn","frontends":{"tunnel":true,"socks5":true,"http":true}}"""
        assertFalse(VpnReconfigure.shouldTearDownTun(UsqueVpnService.MSG_RECONFIGURE, profileJson))
        assertTrue(VpnReconfigure.tunnelFrontendEnabled(profileJson))
    }

    @Test
    fun msgReconfigureLegacyVpnModeWithoutFrontendsKeepsTun() {
        val profileJson = """{"mode":"vpn"}"""
        assertFalse(VpnReconfigure.shouldTearDownTun(UsqueVpnService.MSG_RECONFIGURE, profileJson))
        assertTrue(VpnReconfigure.tunnelFrontendEnabled(profileJson))
    }

    @Test
    fun canonicalizeProfileArgumentsDerivesModeFromTunnelFlag() {
        val normalized =
            VpnReconfigure.canonicalizeProfileArguments(
                mapOf(
                    "mode" to "vpn",
                    "frontends" to mapOf("tunnel" to false, "socks5" to true, "http" to true),
                ),
            )
        assertEquals("socks5", normalized["mode"])
    }

    @Test
    fun otherControlMessagesDoNotCloseTun() {
        val profileJson = """{"mode":"vpn","frontends":{"tunnel":false,"socks5":true,"http":true}}"""
        assertFalse(VpnReconfigure.shouldTearDownTun(UsqueVpnService.MSG_RETRY, profileJson))
    }
}
