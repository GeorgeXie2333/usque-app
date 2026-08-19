package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class HotReconfigureTest {
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
