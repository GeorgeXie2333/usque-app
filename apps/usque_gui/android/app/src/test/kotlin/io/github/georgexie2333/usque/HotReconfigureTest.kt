package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.atomic.AtomicReference

class HotReconfigureTest {
    private val openFd = Any()

    private fun dispatch(
        extras: Map<String, Any?>,
        what: Int = UsqueVpnService.MSG_RECONFIGURE,
    ): Triple<AtomicReference<Any?>, AtomicReference<TunIdentity?>, MutableList<Any?>> {
        val tunnel = AtomicReference<Any?>(openFd)
        val lastIdentity =
            AtomicReference(
                TunIdentity(
                    profileId = "8c30b771-9ebd-457a-b67b-bbc74a1ddba6",
                    mtu = 1280,
                    dnsMode = "tunnel",
                    dnsV4 = "1.1.1.1",
                    dnsV6 = "2606:4700:4700::1111",
                    allowLan = false,
                    bypassCidrs = emptyList(),
                ),
            )
        val closed = mutableListOf<Any?>()
        UsqueVpnService.handleReconfigureNativeOk(
            what,
            extras,
            tunnel,
            lastIdentity,
        ) { fd -> closed.add(fd) }
        return Triple(tunnel, lastIdentity, closed)
    }

    @Test
    fun msgReconfigureTunnelFalseModeVpnTearsDownTun() {
        val extras =
            mapOf(
                UsqueVpnService.EXTRA_PROFILE_JSON to
                    """{"mode":"vpn","frontends":{"tunnel":false,"socks5":true,"http":true}}""",
            )
        val (tunnel, lastIdentity, closed) = dispatch(extras)
        assertNull(tunnel.get())
        assertNull(lastIdentity.get())
        assertEquals(listOf(openFd), closed)
        assertFalse(UsqueVpnService.tunnelFrontendEnabled(extras.getValue(UsqueVpnService.EXTRA_PROFILE_JSON)))
    }

    @Test
    fun msgReconfigureTunnelTrueModeVpnKeepsTun() {
        val extras =
            mapOf(
                UsqueVpnService.EXTRA_PROFILE_JSON to
                    """{"mode":"vpn","frontends":{"tunnel":true,"socks5":true,"http":true}}""",
            )
        val (tunnel, lastIdentity, closed) = dispatch(extras)
        assertSame(openFd, tunnel.get())
        assertTrue(lastIdentity.get() != null)
        assertTrue(closed.isEmpty())
    }

    @Test
    fun msgReconfigureLegacyVpnModeWithoutFrontendsKeepsTun() {
        val extras =
            mapOf(
                UsqueVpnService.EXTRA_PROFILE_JSON to """{"mode":"vpn"}""",
            )
        val (tunnel, lastIdentity, closed) = dispatch(extras)
        assertSame(openFd, tunnel.get())
        assertTrue(lastIdentity.get() != null)
        assertTrue(closed.isEmpty())
    }

    @Test
    fun otherControlMessagesDoNotCloseTun() {
        val extras =
            mapOf(
                UsqueVpnService.EXTRA_PROFILE_JSON to
                    """{"mode":"vpn","frontends":{"tunnel":false,"socks5":true,"http":true}}""",
            )
        val (tunnel, lastIdentity, closed) = dispatch(extras, what = UsqueVpnService.MSG_RETRY)
        assertSame(openFd, tunnel.get())
        assertTrue(lastIdentity.get() != null)
        assertTrue(closed.isEmpty())
    }
}
