package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.atomic.AtomicLong

class ServiceSnapshotStateTest {
    private val clock = AtomicLong(1_700_000_000_000L)

    private fun state(): ServiceSnapshotState = ServiceSnapshotState(clockMillis = clock::get)

    private fun platform(
        tunnelOpen: Boolean = true,
        activeMode: String? = "vpn",
        platformLockdown: Boolean = false,
        alwaysOn: Boolean = false,
    ): ServiceSnapshotState.PlatformFlags =
        ServiceSnapshotState.PlatformFlags(
            tunnelOpen = tunnelOpen,
            activeMode = activeMode,
            platformLockdown = platformLockdown,
            alwaysOn = alwaysOn,
        )

    @Test
    fun snapshotFieldsMapEveryMessengerKey() {
        val snapshot = state()
        snapshot.phase = "connected"
        snapshot.warning = "degraded path"
        snapshot.errorCode = null
        snapshot.transport = "h3"
        snapshot.addressFamily = "dual"
        snapshot.connectedAt = "2024-01-01T00:00:00Z"
        snapshot.downloadBytesPerSecond = 11
        snapshot.uploadBytesPerSecond = 22
        snapshot.downloadedBytes = 33
        snapshot.uploadedBytes = 44
        snapshot.reconnectCount = 2
        snapshot.activeListeners = listOf("127.0.0.1:1080", "[::1]:1080")
        snapshot.exitIpv4 = "1.1.1.1"
        snapshot.exitIpv6 = "2606:4700::1"
        snapshot.exitCity = "Lisbon"
        snapshot.exitCountry = "Portugal"
        snapshot.exitCountryCode = "PT"
        snapshot.exitFlagSvg = "<svg/>"
        snapshot.killSwitchEnabled = true

        val fields = snapshot.snapshotFields(platform(tunnelOpen = true, alwaysOn = true))

        assertEquals("connected", fields.phase)
        assertEquals("degraded path", fields.warning)
        assertNull(fields.errorCode)
        assertEquals("h3", fields.transport)
        assertEquals("dual", fields.addressFamily)
        assertEquals("2024-01-01T00:00:00Z", fields.connectedAt)
        assertEquals(11L, fields.downloadBytesPerSecond)
        assertEquals(22L, fields.uploadBytesPerSecond)
        assertEquals(33L, fields.downloadedBytes)
        assertEquals(44L, fields.uploadedBytes)
        assertEquals(2, fields.reconnectCount)
        assertEquals(listOf("127.0.0.1:1080", "[::1]:1080"), fields.activeListeners)
        assertEquals("1.1.1.1", fields.exitIpv4)
        assertEquals("2606:4700::1", fields.exitIpv6)
        assertEquals("Lisbon", fields.exitCity)
        assertEquals("Portugal", fields.exitCountry)
        assertEquals("PT", fields.exitCountryCode)
        assertEquals("<svg/>", fields.exitFlagSvg)
        assertEquals("active", fields.killSwitchState)
        assertEquals(0, fields.captivePauseRemainingSeconds)
        assertEquals(false, fields.platformLockdown)
        assertEquals(true, fields.alwaysOn)
    }

    @Test
    fun applyNativeSnapshotMergesJsonFields() {
        val snapshot = state()
        val source =
            ServiceSnapshotState.NativeSnapshotFields(
                phase = "connected",
                warning = null,
                errorCode = null,
                transport = "h3",
                addressFamily = "ipv4",
                downloadBytesPerSecond = 100,
                uploadBytesPerSecond = 50,
                downloadedBytes = 1000,
                uploadedBytes = 500,
                reconnectCount = 1,
                activeListeners = listOf("127.0.0.1:8080"),
                exitIpv4 = "203.0.113.10",
                exitIpv6 = null,
                exitCity = "Austin",
                exitCountry = "United States",
                exitCountryCode = "US",
                exitFlagSvg = "<svg id='us'/>",
            )

        val merge = snapshot.applyNativeSnapshot(source)

        assertEquals("connected", snapshot.phase)
        assertEquals("h3", snapshot.transport)
        assertEquals("ipv4", snapshot.addressFamily)
        assertEquals(100L, snapshot.downloadBytesPerSecond)
        assertEquals(50L, snapshot.uploadBytesPerSecond)
        assertEquals(1000L, snapshot.downloadedBytes)
        assertEquals(500L, snapshot.uploadedBytes)
        assertEquals(1, snapshot.reconnectCount)
        assertEquals(listOf("127.0.0.1:8080"), snapshot.activeListeners)
        assertEquals("203.0.113.10", snapshot.exitIpv4)
        assertNull(snapshot.exitIpv6)
        assertEquals("Austin", snapshot.exitCity)
        assertEquals("United States", snapshot.exitCountry)
        assertEquals("US", snapshot.exitCountryCode)
        assertEquals("<svg id='us'/>", snapshot.exitFlagSvg)
        assertNotNull(snapshot.connectedAt)
        assertTrue(merge.phaseChanged)
        assertEquals(
            ServiceSnapshotState.FlagCacheWrite("US", "<svg id='us'/>"),
            merge.cacheWrite,
        )

        val fields = snapshot.snapshotFields(platform())
        assertEquals("connected", fields.phase)
        assertEquals("h3", fields.transport)
        assertEquals("ipv4", fields.addressFamily)
        assertEquals(100L, fields.downloadBytesPerSecond)
        assertEquals(listOf("127.0.0.1:8080"), fields.activeListeners)
        assertEquals("203.0.113.10", fields.exitIpv4)
        assertEquals("US", fields.exitCountryCode)
        assertEquals("<svg id='us'/>", fields.exitFlagSvg)
    }

    @Test
    fun fingerprintDeduplicatesUnchangedBroadcasts() {
        val snapshot = state()
        snapshot.phase = "connected"
        snapshot.transport = "h3"
        val flags = platform()

        val firstFingerprint = snapshot.fingerprint(flags)
        assertTrue(snapshot.markBroadcastIfChanged(flags))
        assertEquals(firstFingerprint, snapshot.lastBroadcastFingerprintForTest())
        assertFalse(snapshot.markBroadcastIfChanged(flags))

        snapshot.downloadBytesPerSecond = 9
        val secondFingerprint = snapshot.fingerprint(flags)
        assertNotEquals(firstFingerprint, secondFingerprint)
        assertTrue(snapshot.markBroadcastIfChanged(flags))
        assertEquals(secondFingerprint, snapshot.lastBroadcastFingerprintForTest())
    }

    @Test
    fun killSwitchStateCoversPausedActiveInactiveAndNotApplicable() {
        val snapshot = state()

        snapshot.phase = "captivePortalPaused"
        snapshot.killSwitchEnabled = true
        assertEquals("paused", snapshot.killSwitchState(tunnelOpen = false, activeMode = "vpn"))

        snapshot.phase = "connected"
        snapshot.killSwitchEnabled = true
        assertEquals("active", snapshot.killSwitchState(tunnelOpen = true, activeMode = "vpn"))

        snapshot.killSwitchEnabled = false
        assertEquals("inactive", snapshot.killSwitchState(tunnelOpen = true, activeMode = "vpn"))

        assertEquals(
            "notApplicable",
            snapshot.killSwitchState(tunnelOpen = false, activeMode = "socks5"),
        )
    }

    @Test
    fun captiveCountdownUsesInjectedClock() {
        val snapshot = state()
        clock.set(1_000_000L)
        snapshot.scheduleCaptivePauseFromNow(30)

        assertEquals(30, snapshot.captivePauseRemainingSeconds())

        clock.addAndGet(29_500L)
        assertEquals(1, snapshot.captivePauseRemainingSeconds())

        clock.addAndGet(500L)
        assertEquals(0, snapshot.captivePauseRemainingSeconds())

        snapshot.clearCaptivePause()
        assertEquals(0, snapshot.captivePauseRemainingSeconds())
    }

    @Test
    fun notificationTextMatchesConnectionPhases() {
        val snapshot = state()

        snapshot.phase = "preparing"
        assertEquals("Preparing secure tunnel", snapshot.notificationText())

        snapshot.phase = "connectingH3"
        assertEquals("Connecting with HTTP/3", snapshot.notificationText())

        snapshot.phase = "connectingH2"
        assertEquals("Connecting with HTTP/2", snapshot.notificationText())

        snapshot.phase = "connected"
        snapshot.transport = "h3"
        assertEquals("Connected via H3", snapshot.notificationText())

        snapshot.phase = "degraded"
        assertEquals("Connected with reduced address-family support", snapshot.notificationText())

        snapshot.phase = "reconnecting"
        assertEquals("Reconnecting securely", snapshot.notificationText())

        clock.set(5_000L)
        snapshot.phase = "captivePortalPaused"
        snapshot.setCaptivePauseDeadline(15_000L)
        assertEquals("VPN paused for captive portal (10 s)", snapshot.notificationText())

        snapshot.phase = "error"
        assertEquals("Network service stopped after an error", snapshot.notificationText())

        snapshot.phase = "disconnecting"
        assertEquals("Disconnecting", snapshot.notificationText())

        snapshot.phase = "disconnected"
        assertEquals("Usque VPN", snapshot.notificationText())
    }

    @Test
    fun resetClearsCountersAndIdentityFields() {
        val snapshot = state()
        snapshot.phase = "connected"
        snapshot.warning = "x"
        snapshot.transport = "h3"
        snapshot.downloadBytesPerSecond = 5
        snapshot.exitCountryCode = "PT"
        snapshot.killSwitchEnabled = true
        snapshot.scheduleCaptivePauseFromNow(10)

        snapshot.reset("disconnected")
        snapshot.clearCaptivePause()

        val fields = snapshot.snapshotFields(platform(tunnelOpen = false, activeMode = null))
        assertEquals("disconnected", fields.phase)
        assertNull(fields.warning)
        assertNull(fields.transport)
        assertEquals(0L, fields.downloadBytesPerSecond)
        assertNull(fields.exitCountryCode)
        assertEquals("notApplicable", fields.killSwitchState)
        assertEquals(0, fields.captivePauseRemainingSeconds)
    }
}
