package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ServiceSnapshotStateTest {
    private fun state(): ServiceSnapshotState = ServiceSnapshotState()

    @Test
    fun physicalNetworkChangeDefersMigrationOrReconnectChoiceToNative() {
        val snapshot = state()
        snapshot.phase = "connected"
        snapshot.noteUnderlyingNetworkChange(networkPresent = true)
        assertEquals("connected", snapshot.phase)
        assertNull(snapshot.warning)
        snapshot.noteUnderlyingNetworkChange(networkPresent = false)
        assertEquals("reconnecting", snapshot.phase)
        assertNotNull(snapshot.warning)
        snapshot.noteUnderlyingNetworkChange(networkPresent = true)
        assertEquals("reconnecting", snapshot.phase)
        assertNull(snapshot.warning)
    }

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
        snapshot.activeFrontends = listOf("socks5", "http")
        snapshot.tunnelIpv4Available = true
        snapshot.tunnelIpv6Available = true
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
        assertEquals(listOf("socks5", "http"), fields.activeFrontends)
        assertEquals(true, fields.tunnelIpv4Available)
        assertEquals(true, fields.tunnelIpv6Available)
        assertEquals("1.1.1.1", fields.exitIpv4)
        assertEquals("2606:4700::1", fields.exitIpv6)
        assertEquals("Lisbon", fields.exitCity)
        assertEquals("Portugal", fields.exitCountry)
        assertEquals("PT", fields.exitCountryCode)
        assertEquals("<svg/>", fields.exitFlagSvg)
        assertEquals("active", fields.killSwitchState)
        assertEquals(false, fields.platformLockdown)
        assertEquals(true, fields.alwaysOn)
    }

    @Test
    fun wireEntriesLockEveryMessengerBundleKeyNameAndValue() {
        val snapshot = state()
        snapshot.phase = "connected"
        snapshot.warning = "degraded path"
        snapshot.errorCode = "E_TEST"
        snapshot.failure =
            ServiceSnapshotState.FailureFields(
                code = "H3_HANDSHAKE_TIMEOUT",
                stage = "quic_handshake",
                transport = "h3",
                addressFamily = "ipv6",
                retryable = true,
                fallbackAllowed = true,
                severity = "warning",
                remediationKey = "try_http2",
                sanitizedDetail = "attempt 2",
            )
        snapshot.transport = "h3"
        snapshot.addressFamily = "dual"
        snapshot.connectedAt = "2024-01-01T00:00:00Z"
        snapshot.downloadBytesPerSecond = 11
        snapshot.uploadBytesPerSecond = 22
        snapshot.downloadedBytes = 33
        snapshot.uploadedBytes = 44
        snapshot.reconnectCount = 2
        snapshot.activeListeners = listOf("127.0.0.1:1080", "[::1]:1080")
        snapshot.activeFrontends = listOf("socks5", "http")
        snapshot.tunnelIpv4Available = true
        snapshot.tunnelIpv6Available = true
        snapshot.exitIpv4 = "1.1.1.1"
        snapshot.exitIpv6 = "2606:4700::1"
        snapshot.exitCity = "Lisbon"
        snapshot.exitCountry = "Portugal"
        snapshot.exitCountryCode = "PT"
        snapshot.exitFlagSvg = "<svg/>"
        snapshot.killSwitchEnabled = true

        val keys = ServiceSnapshotState.WireKeys
        val wire =
            snapshot.wireEntries(
                platform(
                    tunnelOpen = true,
                    activeMode = "vpn",
                    platformLockdown = true,
                    alwaysOn = true,
                ),
            )

        assertEquals(
            setOf(
                keys.PHASE,
                keys.WARNING,
                keys.ERROR_CODE,
                keys.FAILURE_CODE,
                keys.FAILURE_STAGE,
                keys.FAILURE_TRANSPORT,
                keys.FAILURE_ADDRESS_FAMILY,
                keys.FAILURE_RETRYABLE,
                keys.FAILURE_FALLBACK_ALLOWED,
                keys.FAILURE_SEVERITY,
                keys.FAILURE_REMEDIATION_KEY,
                keys.FAILURE_SANITIZED_DETAIL,
                keys.TRANSPORT,
                keys.ADDRESS_FAMILY,
                keys.CONNECTED_AT,
                keys.DOWNLOAD_BYTES_PER_SECOND,
                keys.UPLOAD_BYTES_PER_SECOND,
                keys.DOWNLOADED_BYTES,
                keys.UPLOADED_BYTES,
                keys.RECONNECT_COUNT,
                keys.ACTIVE_LISTENERS,
                keys.ACTIVE_FRONTENDS,
                keys.TUNNEL_IPV4_AVAILABLE,
                keys.TUNNEL_IPV6_AVAILABLE,
                keys.EXIT_IPV4,
                keys.EXIT_IPV6,
                keys.EXIT_CITY,
                keys.EXIT_COUNTRY,
                keys.EXIT_COUNTRY_CODE,
                keys.EXIT_FLAG_SVG,
                keys.KILL_SWITCH_STATE,
                keys.PLATFORM_LOCKDOWN,
                keys.ALWAYS_ON,
                keys.VPN_SERVICE_STATE,
                keys.VPN_PROCESS_STATE,
                keys.TUN_FD_VALID,
                keys.TUN_INTERFACE_PRESENT,
                keys.UNDERLYING_NETWORK_PRESENT,
                keys.UNDERLYING_FAMILY_MASK,
                keys.NETWORK_GENERATION,
                keys.DNS_SERVER_COUNT,
                keys.NATIVE_RUNTIME_STATE,
                keys.FOREGROUND_NOTIFICATION_STATE,
                keys.PENDING_CLEANUP,
            ),
            wire.keys,
        )
        // Exact snake_case strings MainActivity.snapshotFromBundle reads.
        assertEquals("phase", keys.PHASE)
        assertEquals("warning", keys.WARNING)
        assertEquals("error_code", keys.ERROR_CODE)
        assertEquals("failure_code", keys.FAILURE_CODE)
        assertEquals("failure_stage", keys.FAILURE_STAGE)
        assertEquals("failure_transport", keys.FAILURE_TRANSPORT)
        assertEquals("failure_address_family", keys.FAILURE_ADDRESS_FAMILY)
        assertEquals("failure_retryable", keys.FAILURE_RETRYABLE)
        assertEquals("failure_fallback_allowed", keys.FAILURE_FALLBACK_ALLOWED)
        assertEquals("failure_severity", keys.FAILURE_SEVERITY)
        assertEquals("failure_remediation_key", keys.FAILURE_REMEDIATION_KEY)
        assertEquals("failure_sanitized_detail", keys.FAILURE_SANITIZED_DETAIL)
        assertEquals("transport", keys.TRANSPORT)
        assertEquals("address_family", keys.ADDRESS_FAMILY)
        assertEquals("connected_at", keys.CONNECTED_AT)
        assertEquals("download_bytes_per_second", keys.DOWNLOAD_BYTES_PER_SECOND)
        assertEquals("upload_bytes_per_second", keys.UPLOAD_BYTES_PER_SECOND)
        assertEquals("downloaded_bytes", keys.DOWNLOADED_BYTES)
        assertEquals("uploaded_bytes", keys.UPLOADED_BYTES)
        assertEquals("reconnect_count", keys.RECONNECT_COUNT)
        assertEquals("active_listeners", keys.ACTIVE_LISTENERS)
        assertEquals("active_frontends", keys.ACTIVE_FRONTENDS)
        assertEquals("tunnel_ipv4_available", keys.TUNNEL_IPV4_AVAILABLE)
        assertEquals("tunnel_ipv6_available", keys.TUNNEL_IPV6_AVAILABLE)
        assertEquals("exit_ipv4", keys.EXIT_IPV4)
        assertEquals("exit_ipv6", keys.EXIT_IPV6)
        assertEquals("exit_city", keys.EXIT_CITY)
        assertEquals("exit_country", keys.EXIT_COUNTRY)
        assertEquals("exit_country_code", keys.EXIT_COUNTRY_CODE)
        assertEquals("exit_flag_svg", keys.EXIT_FLAG_SVG)
        assertEquals("kill_switch_state", keys.KILL_SWITCH_STATE)
        assertEquals("platform_lockdown", keys.PLATFORM_LOCKDOWN)
        assertEquals("always_on", keys.ALWAYS_ON)
        assertEquals("vpn_service_state", keys.VPN_SERVICE_STATE)
        assertEquals("vpn_process_state", keys.VPN_PROCESS_STATE)
        assertEquals("tun_fd_valid", keys.TUN_FD_VALID)
        assertEquals("tun_interface_present", keys.TUN_INTERFACE_PRESENT)
        assertEquals("underlying_network_present", keys.UNDERLYING_NETWORK_PRESENT)
        assertEquals("underlying_family_mask", keys.UNDERLYING_FAMILY_MASK)
        assertEquals("network_generation", keys.NETWORK_GENERATION)
        assertEquals("dns_server_count", keys.DNS_SERVER_COUNT)
        assertEquals("native_runtime_state", keys.NATIVE_RUNTIME_STATE)
        assertEquals("foreground_notification_state", keys.FOREGROUND_NOTIFICATION_STATE)
        assertEquals("pending_cleanup", keys.PENDING_CLEANUP)

        assertEquals("connected", wire[keys.PHASE])
        assertEquals("degraded path", wire[keys.WARNING])
        assertEquals("E_TEST", wire[keys.ERROR_CODE])
        assertEquals("H3_HANDSHAKE_TIMEOUT", wire[keys.FAILURE_CODE])
        assertEquals("quic_handshake", wire[keys.FAILURE_STAGE])
        assertEquals("h3", wire[keys.FAILURE_TRANSPORT])
        assertEquals("ipv6", wire[keys.FAILURE_ADDRESS_FAMILY])
        assertEquals(true, wire[keys.FAILURE_RETRYABLE])
        assertEquals(true, wire[keys.FAILURE_FALLBACK_ALLOWED])
        assertEquals("warning", wire[keys.FAILURE_SEVERITY])
        assertEquals("try_http2", wire[keys.FAILURE_REMEDIATION_KEY])
        assertEquals("attempt 2", wire[keys.FAILURE_SANITIZED_DETAIL])
        assertEquals("h3", wire[keys.TRANSPORT])
        assertEquals("dual", wire[keys.ADDRESS_FAMILY])
        assertEquals("2024-01-01T00:00:00Z", wire[keys.CONNECTED_AT])
        assertEquals(11L, wire[keys.DOWNLOAD_BYTES_PER_SECOND])
        assertEquals(22L, wire[keys.UPLOAD_BYTES_PER_SECOND])
        assertEquals(33L, wire[keys.DOWNLOADED_BYTES])
        assertEquals(44L, wire[keys.UPLOADED_BYTES])
        assertEquals(2, wire[keys.RECONNECT_COUNT])
        assertEquals(arrayListOf("127.0.0.1:1080", "[::1]:1080"), wire[keys.ACTIVE_LISTENERS])
        assertEquals(arrayListOf("socks5", "http"), wire[keys.ACTIVE_FRONTENDS])
        assertEquals(true, wire[keys.TUNNEL_IPV4_AVAILABLE])
        assertEquals(true, wire[keys.TUNNEL_IPV6_AVAILABLE])
        assertEquals("1.1.1.1", wire[keys.EXIT_IPV4])
        assertEquals("2606:4700::1", wire[keys.EXIT_IPV6])
        assertEquals("Lisbon", wire[keys.EXIT_CITY])
        assertEquals("Portugal", wire[keys.EXIT_COUNTRY])
        assertEquals("PT", wire[keys.EXIT_COUNTRY_CODE])
        assertEquals("<svg/>", wire[keys.EXIT_FLAG_SVG])
        assertEquals("active", wire[keys.KILL_SWITCH_STATE])
        assertEquals(true, wire[keys.PLATFORM_LOCKDOWN])
        assertEquals(true, wire[keys.ALWAYS_ON])
        assertEquals("running", wire[keys.VPN_SERVICE_STATE])
        assertEquals("reachable", wire[keys.VPN_PROCESS_STATE])
        assertEquals(true, wire[keys.TUN_FD_VALID])
        assertEquals(true, wire[keys.TUN_INTERFACE_PRESENT])
        assertEquals(false, wire[keys.UNDERLYING_NETWORK_PRESENT])
        assertEquals(0, wire[keys.UNDERLYING_FAMILY_MASK])
        assertEquals(0L, wire[keys.NETWORK_GENERATION])
        assertEquals(0, wire[keys.DNS_SERVER_COUNT])
        assertEquals("stopped", wire[keys.NATIVE_RUNTIME_STATE])
        assertEquals("inactive", wire[keys.FOREGROUND_NOTIFICATION_STATE])
        assertEquals(false, wire[keys.PENDING_CLEANUP])

        // Fingerprint dedup path shares the same wire map source as toBundle.
        assertTrue(snapshot.markBroadcastIfChanged(platform(tunnelOpen = true, alwaysOn = true)))
        assertFalse(snapshot.markBroadcastIfChanged(platform(tunnelOpen = true, alwaysOn = true)))
    }

    @Test
    fun applyNativeSnapshotMergesJsonFields() {
        val snapshot = state()
        val source =
            ServiceSnapshotState.NativeSnapshotFields(
                phase = "connected",
                warning = null,
                errorCode = null,
                failure =
                    ServiceSnapshotState.FailureFields(
                        code = "H3_HANDSHAKE_TIMEOUT",
                        stage = "quic_handshake",
                        transport = "h3",
                        addressFamily = "ipv4",
                        retryable = true,
                        fallbackAllowed = true,
                        severity = "warning",
                        remediationKey = "try_http2",
                    ),
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
        assertEquals("H3_HANDSHAKE_TIMEOUT", snapshot.failure?.code)
        assertEquals("quic_handshake", snapshot.failure?.stage)
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
        assertEquals("try_http2", fields.failure?.remediationKey)
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
    fun killSwitchStateCoversActiveInactiveAndNotApplicable() {
        val snapshot = state()

        snapshot.phase = "connected"
        snapshot.killSwitchEnabled = true
        assertEquals("active", snapshot.killSwitchState(tunnelOpen = true, activeMode = "vpn"))

        // KS enabled but tunnel down (e.g. establishing / torn down) must not report active.
        snapshot.killSwitchEnabled = true
        assertEquals("inactive", snapshot.killSwitchState(tunnelOpen = false, activeMode = "vpn"))

        snapshot.killSwitchEnabled = false
        assertEquals("inactive", snapshot.killSwitchState(tunnelOpen = true, activeMode = "vpn"))

        assertEquals(
            "notApplicable",
            snapshot.killSwitchState(tunnelOpen = false, activeMode = "socks5"),
        )
    }

    @Test
    fun resetLeavesFingerprintForDedup() {
        val snapshot = state()
        snapshot.phase = "connected"
        snapshot.transport = "h3"
        val flags = platform()
        assertTrue(snapshot.markBroadcastIfChanged(flags))

        snapshot.reset("disconnected")

        // Fingerprint retained: identical post-reset broadcast is still deduped until state moves.
        assertNotNull(snapshot.lastBroadcastFingerprintForTest())
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

        snapshot.reset("disconnected")

        val fields = snapshot.snapshotFields(platform(tunnelOpen = false, activeMode = null))
        assertEquals("disconnected", fields.phase)
        assertNull(fields.warning)
        assertNull(fields.transport)
        assertEquals(0L, fields.downloadBytesPerSecond)
        assertNull(fields.exitCountryCode)
        assertEquals("notApplicable", fields.killSwitchState)
    }
}
