package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.ArrayDeque
import java.util.concurrent.Executor

class AndroidDiagnosticsCoordinatorTest {
    private class QueuedExecutor : Executor {
        private val tasks = ArrayDeque<Runnable>()

        override fun execute(command: Runnable) {
            tasks.addLast(command)
        }

        fun runAll() {
            while (tasks.isNotEmpty()) tasks.removeFirst().run()
        }
    }

    private fun snapshot(): Map<String, Any?> =
        mapOf(
            "phase" to "connected",
            "transport" to "h3",
            "address_family" to "dual",
            "underlying_network_present" to true,
            "underlying_family_mask" to 3,
            "network_generation" to 4L,
            "dns_server_count" to 2,
            "tun_fd_valid" to true,
            "active_frontends" to listOf("socks5", "http"),
            "tunnel_ipv4_available" to true,
            "tunnel_ipv6_available" to true,
            "kill_switch_state" to "active",
            "platform_state_observed" to true,
            "downloaded_bytes" to 4L,
            "uploaded_bytes" to 2L,
            "reconnect_count" to 1,
        )

    @Test
    fun duplicateStartIsRejectedAndCancelWinsTheCompletionRace() {
        val executor = QueuedExecutor()
        val published = mutableListOf<Map<String, Any?>>()
        val coordinator =
            AndroidDiagnosticsCoordinator(
                executor = executor,
                publish = published::add,
                nowMillis = { 10L },
                newSessionId = { "session-one" },
            )

        val started = coordinator.start("standard", snapshot(), true, true, true, true)
        assertEquals("running", started["state"])
        val duplicate =
            assertThrows(AndroidDiagnosticsCoordinator.DiagnosticsException::class.java) {
                coordinator.start("standard", snapshot(), true, true, true, true)
            }
        assertEquals("DIAGNOSTICS_ALREADY_RUNNING", duplicate.code)

        val cancelled = coordinator.cancel("session-one")
        assertEquals("cancelling", cancelled["state"])
        assertThrows(AndroidDiagnosticsCoordinator.DiagnosticsException::class.java) {
            coordinator.start("deep", snapshot(), true, true, true, true)
        }
        executor.runAll()
        assertEquals("cancelled", coordinator.current()?.get("state"))
        assertTrue(published.isNotEmpty())
    }

    @Test
    fun completedSessionUsesTheFullStableCatalogWithoutPrivateSnapshotFields() {
        val executor = QueuedExecutor()
        val published = mutableListOf<Map<String, Any?>>()
        val coordinator =
            AndroidDiagnosticsCoordinator(
                executor = executor,
                publish = published::add,
                nowMillis = { 20L },
                newSessionId = { "session-two" },
            )
        val hostile =
            snapshot() +
                mapOf(
                    "ssid" to "private-network-name",
                    "dns_servers" to listOf("192.0.2.53"),
                    "installed_apps" to listOf("com.example.private"),
                )

        coordinator.start("deep", hostile, true, true, true, true)
        executor.runAll()
        val completed = requireNotNull(coordinator.current())
        assertEquals("completed", completed["state"])
        assertEquals(39, (completed["findings"] as List<*>).size)
        val exported = completed.toString()
        assertFalse(exported.contains("private-network-name"))
        assertFalse(exported.contains("192.0.2.53"))
        assertFalse(exported.contains("com.example.private"))
        val progress =
            published.mapNotNull { event ->
                @Suppress("UNCHECKED_CAST")
                val session = event["diagnostic_session"] as? Map<String, Any?>
                (session?.get("progress_percent") as? Number)?.toInt()
            }
        assertTrue(progress.size >= 80)
        assertEquals(progress.sorted(), progress)
        assertEquals(100, progress.last())
    }

    @Test
    fun connectionTimelineIsBoundedAndOmitsRawNetworkValues() {
        val coordinator = AndroidDiagnosticsCoordinator(executor = Executor(Runnable::run))
        repeat(300) { index ->
            coordinator.observeSnapshot(
                snapshot() +
                    mapOf(
                        "phase" to if (index % 2 == 0) "reconnecting" else "connected",
                        "network_generation" to index.toLong(),
                        "ssid" to "never-export-me",
                    ),
            )
        }
        val timeline = coordinator.timeline()
        assertEquals(256, (timeline["events"] as List<*>).size)
        assertEquals(44L, timeline["dropped_event_count"])
        @Suppress("UNCHECKED_CAST")
        val metrics = timeline["metrics"] as Map<String, Any?>
        assertEquals(299L, metrics["network_change_count"])
        assertFalse(timeline.toString().contains("never-export-me"))
    }

    @Test
    fun structuredFailureSurvivesTheReadOnlyTimelineWithoutPrivateDetail() {
        val coordinator = AndroidDiagnosticsCoordinator(executor = Executor(Runnable::run))
        coordinator.observeSnapshot(
            snapshot() +
                mapOf(
                    "phase" to "reconnecting",
                    "error_code" to "H3_HANDSHAKE_TIMEOUT",
                    "failure" to
                        mapOf(
                            "code" to "H3_HANDSHAKE_TIMEOUT",
                            "stage" to "quic_handshake",
                            "transport" to "h3",
                            "address_family" to "ipv6",
                            "retryable" to true,
                            "fallback_allowed" to true,
                            "severity" to "warning",
                            "remediation_key" to "try_http2",
                            "sanitized_detail" to "private.example",
                        ),
                ),
        )

        @Suppress("UNCHECKED_CAST")
        val event = (coordinator.timeline()["events"] as List<Map<String, Any?>>).single()

        @Suppress("UNCHECKED_CAST")
        val failure = event["failure"] as Map<String, Any?>
        assertEquals("quic_handshake", event["stage"])
        assertEquals(true, failure["retryable"])
        assertEquals(true, failure["fallback_allowed"])
        assertFalse(failure.containsKey("sanitized_detail"))
    }

    @Test
    fun unreachableControlAndUnknownPlatformStateNeverProduceFalsePasses() {
        val executor = QueuedExecutor()
        val coordinator =
            AndroidDiagnosticsCoordinator(
                executor = executor,
                newSessionId = { "session-unreachable" },
            )
        val stale = snapshot() + ("platform_state_observed" to false)

        coordinator.start("standard", stale, false, false, true, true)
        executor.runAll()

        @Suppress("UNCHECKED_CAST")
        val findings = requireNotNull(coordinator.current())["findings"] as List<Map<String, Any?>>
        val byId = findings.associateBy { finding -> finding["check_id"] as String }
        assertEquals("failed", byId.getValue("engine.control_channel")["status"])
        assertEquals(
            "ENGINE_UNAVAILABLE",
            (byId.getValue("engine.control_channel")["failure"] as Map<*, *>)["code"],
        )
        val unknownPlatformChecks =
            listOf(
                "physical.network_present",
                "physical.ipv4_route",
                "physical.ipv6_route",
                "physical.dns_available",
                "physical.network_generation",
                "transport.endpoint_pin",
                "transport.fallback_policy",
                "tunnel.address_assignment",
                "tunnel.routes",
                "tunnel.dns",
                "tunnel.first_packet",
                "protection.kill_switch",
                "protection.recovery_journal",
            )
        for (checkId in unknownPlatformChecks) {
            assertEquals("skipped", byId.getValue(checkId)["status"])
        }
    }

    @Test
    fun frontendChecksUseTypedRuntimeKindsInsteadOfListenerAddresses() {
        val executor = QueuedExecutor()
        val coordinator =
            AndroidDiagnosticsCoordinator(
                executor = executor,
                newSessionId = { "session-frontends" },
            )

        coordinator.start("standard", snapshot(), true, true, true, true)
        executor.runAll()

        @Suppress("UNCHECKED_CAST")
        val findings = requireNotNull(coordinator.current())["findings"] as List<Map<String, Any?>>
        val byId = findings.associateBy { finding -> finding["check_id"] as String }
        assertEquals("passed", byId.getValue("frontend.socks_port")["status"])
        assertEquals("passed", byId.getValue("frontend.http_port")["status"])
        assertEquals("warning", byId.getValue("tunnel.routes")["status"])
        assertEquals("warning", byId.getValue("tunnel.dns")["status"])
    }
}
