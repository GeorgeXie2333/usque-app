package io.github.georgexie2333.usque

import android.content.ServiceConnection
import android.os.Bundle
import android.os.IBinder
import io.flutter.plugin.common.MethodChannel
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.util.concurrent.CompletableFuture
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

class VpnControlClientTest {
    @Test
    fun oldWarpQueryCannotReleaseANewerGenerationJob() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val oldQuery = RecordingResult()
        client.requestVpnGate("""{"command":"warp_wireguard","warp_wireguard":{"action":"get"}}""", oldQuery)
        val oldId = endpoint.messages.last().requestId
        val generate = RecordingResult()
        client.requestVpnGate("""{"command":"warp_wireguard","warp_wireguard":{"action":"generate"}}""", generate)
        val jobId = "10000000-0000-4000-8000-000000000003"
        client.deliverVpnGateReply(
            endpoint.messages.last().requestId,
            """{"job":{"id":"$jobId","state":"running"},"error":null}""",
            null,
        )
        assertEquals(1, generate.completionCount)
        client.deliverVpnGateReply(oldId, """{"job":null,"error":null}""", null)
        assertEquals(1, oldQuery.completionCount)
        assertTrue(client.creatingBindingForTest())
        assertTrue(client.warpGenerationPending)
        scheduler.advanceBy(1_000)
        client.deliverVpnGateReply(
            endpoint.messages.last().requestId,
            """{"job":{"id":"$jobId","state":"completed"},"error":null}""",
            null,
        )
        assertFalse(client.creatingBindingForTest())
        assertEquals(0, scheduler.pendingTimeoutCount())
    }

    @Test
    fun historicProtonJobQueryCannotRetireTheCurrentVerificationJob() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        client.requestProtonVpn("""{"action":"login"}""", byteArrayOf(1), RecordingResult())
        val jobId = "10000000-0000-4000-8000-000000000004"
        client.deliverProtonReply(
            endpoint.messages.last().requestId,
            """{"job":{"id":"$jobId","state":"waiting_verification"},"profiles":[],"error":null}""".toByteArray(),
        )
        val historic = "10000000-0000-4000-8000-000000000005"
        client.requestProtonVpn("""{"action":"get","job_id":"$historic"}""", ByteArray(0), RecordingResult())
        client.deliverProtonReply(
            endpoint.messages.last().requestId,
            """{"job":{"id":"$historic","state":"completed"},"profiles":[],"error":null}""".toByteArray(),
        )
        assertTrue(client.creatingBindingForTest())
        scheduler.advanceBy(1_000)
        client.deliverProtonReply(
            endpoint.messages.last().requestId,
            """{"job":{"id":"$jobId","state":"cancelled"},"profiles":[],"error":null}""".toByteArray(),
        )
        assertFalse(client.creatingBindingForTest())
        assertEquals(0, scheduler.pendingTimeoutCount())
    }

    @Test
    fun oldServiceRepliesAndEventsAreRejectedBeforeDecodingAfterReplacement() {
        val connections = mutableListOf<ServiceConnection>()
        val receivers = mutableListOf<(Int, Int, Bundle) -> Boolean>()
        val endpoint = RecordingEndpoint()
        val live =
            VpnControlClient(scheduler, { connection, _ ->
                connections += connection
                true
            }, {}, { _, receiver ->
                receivers += receiver
                endpoint
            })

        fun binder(): IBinder =
            java.lang.reflect.Proxy.newProxyInstance(
                IBinder::class.java.classLoader,
                arrayOf(IBinder::class.java),
            ) { _, _, _ -> null } as IBinder
        live.setUiVisible(true)
        live.setEventsWanted(true)
        val firstBinder = binder()
        connections.single().onServiceConnected(null, firstBinder)
        val oldReceiver = receivers.last()
        // A second binding to the same service must share its reply Messenger.
        val owner = RecordingResult()
        live.beginControlOperation(owner)
        connections.last().onServiceConnected(null, firstBinder)
        assertSame(oldReceiver, receivers.last())
        connections.last().onServiceConnected(null, binder())
        val currentReceiver = receivers.last()
        assertFalse(oldReceiver === currentReceiver)
        val marker = mapOf("phase" to "connected", "instance_id" to "replacement")
        live.deliverEvent(marker)
        val result = RecordingResult()
        live.requestSnapshot(result)
        val requestId = endpoint.messages.last().requestId
        val empty = Bundle()
        assertTrue(oldReceiver(UsqueVpnService.MSG_EVENT, 0, empty))
        assertTrue(oldReceiver(UsqueVpnService.MSG_SNAPSHOT, requestId, empty))
        assertSame(marker, live.lastSnapshot)
        assertEquals(0, result.completionCount)
        assertEquals(1, live.pendingSnapshotCountForTest())
        assertFalse(currentReceiver(-1, 0, empty))
        live.deliverSnapshotReply(requestId, null, null, marker)
        assertEquals(1, result.completionCount)
        live.destroy()
    }

    @Test
    fun nullCreatingBindingKeepsTheObserverAndItsReplyEpochUsable() {
        val connections = mutableListOf<ServiceConnection>()
        val receivers = mutableListOf<(Int, Int, Bundle) -> Boolean>()
        val endpoint = RecordingEndpoint()
        val live =
            VpnControlClient(scheduler, { connection, _ ->
                connections += connection
                true
            }, {}, { _, receiver ->
                receivers += receiver
                endpoint
            })
        live.setUiVisible(true)
        val binder =
            java.lang.reflect.Proxy.newProxyInstance(
                IBinder::class.java.classLoader,
                arrayOf(IBinder::class.java),
            ) { _, _, _ -> null } as IBinder
        connections.single().onServiceConnected(null, binder)
        val receiver = receivers.single()
        val owner = RecordingResult()
        live.beginControlOperation(owner)
        connections.last().onNullBinding(null)
        assertTrue(live.hasEndpoint)
        assertFalse(receiver(-1, 0, Bundle()))
        assertEquals(1, owner.completionCount)
        live.destroy()
    }

    @Test
    fun aTimedOutGenerationAckDoesNotDiscardPossibleNativeWorkOrReplayGenerate() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()
        client.requestVpnGate("""{"command":"warp_wireguard","warp_wireguard":{"action":"generate"}}""", result)
        scheduler.advanceBy(50_000L)
        assertEquals("VPN_GATE_UNAVAILABLE", result.errorCode)
        assertTrue(client.creatingBindingForTest())
        assertEquals(2, endpoint.messages.size)
        val poll = endpoint.messages.last()
        assertEquals(
            "get",
            JSONObject(
                poll.extras?.get("vpn_gate_request") as String,
            ).getJSONObject("warp_wireguard").getString("action"),
        )
        client.deliverVpnGateReply(poll.requestId, """{"job":null,"error":null}""", null)
        assertFalse(client.creatingBindingForTest())
        assertEquals(1, result.completionCount)
        assertEquals(0, scheduler.pendingTimeoutCount())
    }

    @Test
    fun protonVerificationOwnsItsServiceAfterTheInitialRpcWhileTheUiIsHidden() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()
        client.requestProtonVpn("""{"action":"login"}""", byteArrayOf(1, 2), result)
        val id = "10000000-0000-4000-8000-000000000002"
        val metadata =
            """{"job":{"id":"$id","state":"waiting_verification"},"profiles":[],"error":null}"""
                .toByteArray()
        client.deliverProtonReply(endpoint.messages.single().requestId, metadata)
        assertTrue(metadata.all { it == 0.toByte() })
        assertEquals(1, result.completionCount)
        client.setUiVisible(false)
        assertTrue(client.creatingBindingForTest())
        scheduler.advanceBy(1_000L)
        val poll = endpoint.messages.last()
        assertEquals("get", JSONObject(poll.extras?.get("proton_command") as String).getString("action"))
        client.deliverProtonReply(
            poll.requestId,
            """{"job":{"id":"$id","state":"cancelled"},"profiles":[],"error":null}""".toByteArray(),
        )
        assertFalse(client.creatingBindingForTest())
        assertEquals(0, scheduler.pendingTimeoutCount())
    }

    @Test
    fun actualBinderDeathRetiresTheOldJobWithoutRestartOrLateCallbackRevival() {
        val connections = mutableListOf<ServiceConnection>()
        val flags = mutableListOf<Boolean>()
        val endpoint = RecordingEndpoint()
        val live =
            VpnControlClient(
                scheduler,
                { connection, creating ->
                    connections += connection
                    flags += creating
                    true
                },
                {},
                { _, _ -> endpoint },
            )
        val result = RecordingResult()
        live.requestVpnGate("""{"command":"warp_wireguard","warp_wireguard":{"action":"generate"}}""", result)
        val binder =
            java.lang.reflect.Proxy.newProxyInstance(
                android.os.IBinder::class.java.classLoader,
                arrayOf(android.os.IBinder::class.java),
            ) { _, _, _ -> null } as android.os.IBinder
        val connection = connections.single()
        connection.onServiceConnected(null, binder)
        live.deliverVpnGateReply(
            endpoint.messages.single().requestId,
            """{"job":{"id":"job","state":"running"}}""",
            null,
        )
        assertTrue(live.creatingBindingForTest())
        connection.onServiceDisconnected(null)
        assertFalse(live.creatingBindingForTest())
        scheduler.advanceBy(60_000L)
        connection.onServiceConnected(null, binder)
        assertEquals(listOf(true), flags)
        assertFalse(live.hasEndpoint)
        assertEquals(1, endpoint.messages.size)
        live.destroy()
    }

    @Test
    fun absentPassiveStatusAndSettingsNeverRequestServiceCreation() {
        val flags = mutableListOf<Boolean>()
        var unbound = 0
        val passive =
            VpnControlClient(
                scheduler,
                { _, creating ->
                    flags += creating
                    false
                },
                { unbound++ },
                { _, _ -> error("Absent service cannot connect") },
            )
        passive.setUiVisible(true)
        passive.setEventsWanted(true)
        passive.requestSnapshot(RecordingResult())
        passive.probeSnapshot { assertFalse(it.controlReachable) }
        val settings = RecordingResult()
        passive.requestNetworkSettings(null, settings)
        val scenes = RecordingResult()
        passive.requestScenarios("""{"action":"list"}""", scenes)
        assertEquals("CONTROL_SERVICE_INACTIVE", settings.errorCode)
        assertEquals("CONTROL_SERVICE_INACTIVE", scenes.errorCode)
        assertTrue(flags.isNotEmpty())
        assertTrue(flags.none { it })
        assertEquals(flags.size, unbound)
        passive.setUiVisible(false)
        assertFalse(passive.isBound)
    }

    @Test
    fun acceptedPassiveBindingWithoutACallbackAllowsPersistedSettingsFallback() {
        val flags = mutableListOf<Boolean>()
        val passive =
            VpnControlClient(
                scheduler,
                { _, creating ->
                    flags += creating
                    true
                },
                {},
                { _, _ -> error("No live service") },
            )
        val settings = RecordingResult()
        val scenes = RecordingResult()
        passive.requestNetworkSettings(null, settings)
        passive.requestScenarios("""{"action":"list"}""", scenes)
        assertEquals(0, settings.completionCount)
        assertEquals(0, scenes.completionCount)
        scheduler.advanceBy(10_000)
        for (result in listOf(settings, scenes)) {
            assertEquals("CONTROL_SERVICE_INACTIVE", result.errorCode)
            assertEquals(1, result.completionCount)
        }
        assertTrue(flags.none { it })
        assertFalse(passive.isBound)
        assertEquals(0, scheduler.pendingTimeoutCount())
    }

    @Test
    fun passiveDeadlineWithoutAServiceLetsLaterPassiveReadsFallBackImmediately() {
        val flags = mutableListOf<Boolean>()
        val passive =
            VpnControlClient(
                scheduler,
                { _, creating ->
                    flags += creating
                    true
                },
                {},
                { _, _ -> error("No live service") },
            )
        passive.setUiVisible(true)
        val first = RecordingResult()
        passive.requestNetworkSettings(null, first)
        scheduler.advanceBy(5_000)
        val waiting = RecordingResult()
        passive.requestScenarios("""{"action":"list"}""", waiting)
        assertEquals(0, waiting.completionCount)
        scheduler.advanceBy(5_000)
        // The first deadline also settles passive reads still waiting for it.
        for (result in listOf(first, waiting)) {
            assertEquals("CONTROL_SERVICE_INACTIVE", result.errorCode)
            assertEquals(1, result.completionCount)
        }
        val settings = RecordingResult()
        val scenes = RecordingResult()
        passive.requestNetworkSettings(null, settings)
        passive.requestScenarios("""{"action":"list"}""", scenes)
        for (result in listOf(settings, scenes)) {
            assertEquals("CONTROL_SERVICE_INACTIVE", result.errorCode)
            assertEquals(1, result.completionCount)
        }
        assertEquals(0, scheduler.pendingTimeoutCount())
        assertTrue(flags.none { it })
        assertTrue(passive.isBound)
        // A creating bind can still deliver a service, so reads wait for it.
        val save = RecordingResult()
        passive.requestScenarios("""{"action":"save"}""", save)
        assertTrue(passive.creatingBindingForTest())
        val during = RecordingResult()
        passive.requestNetworkSettings(null, during)
        scheduler.advanceBy(9_999)
        assertEquals(0, save.completionCount)
        assertEquals(0, during.completionCount)
        passive.destroy()
        assertEquals("NETWORK_SETTINGS_UNCONFIRMED", save.errorCode)
        assertEquals("NETWORK_SETTINGS_UNCONFIRMED", during.errorCode)
    }

    @Test
    fun aDeliveredEndpointOrHiddenUiRetiresKnownServiceInactivity() {
        val connections = mutableListOf<ServiceConnection>()
        val endpoint = RecordingEndpoint()
        val passive =
            VpnControlClient(
                scheduler,
                { connection, creating ->
                    assertFalse(creating)
                    connections += connection
                    true
                },
                {},
                { _, _ -> endpoint },
            )

        fun binder(): IBinder =
            java.lang.reflect.Proxy.newProxyInstance(
                IBinder::class.java.classLoader,
                arrayOf(IBinder::class.java),
            ) { _, _, _ -> null } as IBinder

        fun proveInactive() {
            val read = RecordingResult()
            passive.requestNetworkSettings(null, read)
            assertEquals(0, read.completionCount)
            scheduler.advanceBy(10_000)
            assertEquals("CONTROL_SERVICE_INACTIVE", read.errorCode)
        }
        passive.setUiVisible(true)
        proveInactive()
        // A service started elsewhere reaches the retained observation.
        connections.last().onServiceConnected(null, binder())
        val live = RecordingResult()
        passive.requestScenarios("""{"action":"list"}""", live)
        assertEquals(UsqueVpnService.MSG_SCENARIOS, endpoint.messages.last().what)
        assertEquals(0, live.completionCount)
        // After that service is lost, only a fresh deadline proves inactivity.
        connections.last().onServiceDisconnected(null)
        proveInactive()
        assertEquals("NETWORK_SETTINGS_UNCONFIRMED", live.errorCode)
        // A stopped UI cannot observe a service that starts in the background,
        // so neither a deadline nor earlier knowledge carries across it.
        passive.setUiVisible(false)
        proveInactive()
        val hidden = RecordingResult()
        passive.requestNetworkSettings(null, hidden)
        assertEquals(0, hidden.completionCount)
        passive.setUiVisible(true)
        assertEquals(0, hidden.completionCount)
        scheduler.advanceBy(10_000)
        assertEquals("CONTROL_SERVICE_INACTIVE", hidden.errorCode)
        val immediate = RecordingResult()
        passive.requestNetworkSettings(null, immediate)
        assertEquals("CONTROL_SERVICE_INACTIVE", immediate.errorCode)
        assertEquals(0, scheduler.pendingTimeoutCount())
        passive.destroy()
    }

    @Test
    fun absentControlProcessLetsPassiveReadsFallBackWithoutADeadline() {
        val connections = mutableListOf<ServiceConnection>()
        val flags = mutableListOf<Boolean>()
        val endpoint = RecordingEndpoint()
        var absent = true
        var checks = 0
        val passive =
            VpnControlClient(
                scheduler,
                { connection, creating ->
                    connections += connection
                    flags += creating
                    true
                },
                {},
                { _, _ -> endpoint },
                controlServiceProcessAbsent = {
                    checks++
                    absent
                },
            )
        passive.setUiVisible(true)
        val settings = RecordingResult()
        val scenes = RecordingResult()
        passive.requestNetworkSettings(null, settings)
        passive.requestScenarios("""{"action":"list"}""", scenes)
        for (result in listOf(settings, scenes)) {
            assertEquals("CONTROL_SERVICE_INACTIVE", result.errorCode)
            assertEquals(1, result.completionCount)
        }
        assertEquals(0, scheduler.pendingTimeoutCount())
        assertTrue(flags.none { it })
        // The observation stays bound for a service that starts later.
        assertTrue(passive.isBound)
        // Writes never use saved configuration, and a creating bind can still
        // deliver a service, so passive reads wait without consulting the process.
        val save = RecordingResult()
        passive.requestScenarios("""{"action":"save"}""", save)
        assertTrue(passive.creatingBindingForTest())
        val checked = checks
        val during = RecordingResult()
        passive.requestNetworkSettings(null, during)
        assertEquals(checked, checks)
        assertEquals(0, save.completionCount)
        assertEquals(0, during.completionCount)
        passive.destroy()
        // A present process keeps the ordinary deadline until the endpoint arrives.
        absent = false
        val retry =
            VpnControlClient(
                scheduler,
                { connection, _ ->
                    connections += connection
                    true
                },
                {},
                { _, _ -> endpoint },
                controlServiceProcessAbsent = { absent },
            )
        retry.setUiVisible(true)
        val waiting = RecordingResult()
        retry.requestNetworkSettings(null, waiting)
        assertEquals(0, waiting.completionCount)
        connections.last().onServiceConnected(
            null,
            java.lang.reflect.Proxy.newProxyInstance(
                IBinder::class.java.classLoader,
                arrayOf(IBinder::class.java),
            ) { _, _, _ -> null } as IBinder,
        )
        assertEquals(UsqueVpnService.MSG_GET_SETTINGS, endpoint.messages.last().what)
        assertEquals(0, waiting.completionCount)
        retry.destroy()
    }

    @Test
    fun anUnavailableProcessListKeepsTheBindDeadline() {
        val passive =
            VpnControlClient(
                scheduler,
                { _, _ -> true },
                {},
                { _, _ -> error("No live service") },
                controlServiceProcessAbsent = { throw SecurityException("Process list unavailable") },
            )
        passive.setUiVisible(true)
        val settings = RecordingResult()
        passive.requestNetworkSettings(null, settings)
        assertEquals(0, settings.completionCount)
        scheduler.advanceBy(10_000)
        assertEquals("CONTROL_SERVICE_INACTIVE", settings.errorCode)
        passive.destroy()
    }

    @Test
    fun sentSettingsReadsAndUnsentWritesNeverAllowPersistedFallbackOnTimeout() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val settings = RecordingResult()
        val scenes = RecordingResult()
        client.requestNetworkSettings(null, settings)
        client.requestScenarios("""{"action":"list"}""", scenes)
        scheduler.advanceBy(10_000)
        for (result in listOf(settings, scenes)) {
            assertEquals("NETWORK_SETTINGS_UNCONFIRMED", result.errorCode)
            assertEquals(1, result.completionCount)
        }
        val unsent = VpnControlClient(scheduler, { _, _ -> true }, {}, { _, _ -> null })
        val save = RecordingResult()
        unsent.requestScenarios("""{"action":"save"}""", save)
        scheduler.advanceBy(10_000)
        assertEquals("NETWORK_SETTINGS_UNCONFIRMED", save.errorCode)
        assertEquals(1, save.completionCount)
        unsent.destroy()
    }

    @Test
    fun sentReconfigureRetainsCreatingBindingWhileHiddenUntilItsReply() {
        val result = RecordingResult()
        client.requestReconfigure("{}", result)
        assertTrue(client.creatingBindingForTest())
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val request = endpoint.messages.single()
        assertEquals(UsqueVpnService.MSG_RECONFIGURE, request.what)
        client.setUiVisible(false)
        assertTrue(client.creatingBindingForTest())
        client.deliverSnapshotReply(request.requestId, null, null, mapOf("phase" to "connected"))
        assertEquals(1, result.completionCount)
        assertFalse(client.creatingBindingForTest())
        assertFalse(client.isBound)
    }

    @Test
    fun theFirstSettingsReplyCannotReleaseAnotherOutstandingCommand() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val first = RecordingResult()
        val second = RecordingResult()
        client.requestNetworkSettings("{}", first)
        client.requestNetworkSettings("{}", second)
        val requests = endpoint.messages.toList()
        client.deliverSettingsReply(requests[0].requestId, """{"source_epoch":"owner","sequence":1}""")
        assertTrue(client.creatingBindingForTest())
        client.deliverSettingsReply(requests[1].requestId, """{"source_epoch":"owner","sequence":2}""")
        assertFalse(client.creatingBindingForTest())
        assertEquals(1, first.completionCount)
        assertEquals(1, second.completionCount)
    }

    @Test
    fun timedOutUnsentRetryReleasesCreationAndIsNeverReplayed() {
        val result = RecordingResult()
        client.requestRetry(result)
        scheduler.advanceBy(VpnControlClient.SNAPSHOT_TIMEOUT_MILLIS - 1)
        assertTrue(client.creatingBindingForTest())
        scheduler.advanceBy(1)
        assertEquals("ENGINE_IPC_TIMEOUT", result.errorCode)
        assertFalse(client.creatingBindingForTest())
        val late = RecordingEndpoint()
        client.attachEndpointForTest(late)
        assertTrue(late.messages.isEmpty())
        assertEquals(1, result.completionCount)
    }

    @Test
    fun coldClearAllWaitsForBindingButItsAcknowledgedLocalWipeDoesNotRetainService() {
        val result = RecordingResult()
        var acknowledged: MethodChannel.Result? = null
        client.clearAllAcknowledgedListener = VpnControlClient.ClearAllAcknowledgedListener { acknowledged = it }
        assertTrue(client.requestClearAllData(result))
        assertTrue(client.creatingBindingForTest())
        assertEquals(0, result.completionCount)
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val request = endpoint.messages.single()
        assertEquals(UsqueVpnService.MSG_CLEAR_ALL_DATA, request.what)
        client.deliverSnapshotReply(request.requestId, null, null, mapOf("phase" to "disconnected"))
        assertFalse(client.creatingBindingForTest())
        assertTrue(client.claimInFlightClearAll(requireNotNull(acknowledged)))
        client.destroy()
        assertEquals(0, result.completionCount)
        assertTrue(client.takeClaimedClearAll(requireNotNull(acknowledged)))
        requireNotNull(acknowledged).success(null)
        assertEquals(1, result.completionCount)
    }

    @Test
    fun explicitOwnerSurvivesPassiveReadsAndReleasesIdempotently() {
        val acquired = RecordingResult()
        client.beginControlOperation(acquired)
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val token = (acquired.successValue as Map<*, *>)["owner_token"] as String
        assertTrue(client.creatingBindingForTest())
        val settings = RecordingResult()
        client.requestNetworkSettings(null, settings)
        client.deliverSettingsReply(
            endpoint.messages.single().requestId,
            """{"source_epoch":"live-owner","sequence":1}""",
        )
        assertTrue(client.creatingBindingForTest())
        client.endControlOperation(token)
        assertFalse(client.creatingBindingForTest())
        assertEquals(0, client.controlOwnerCountForTest())
        val releases = binder.unbindCount
        client.endControlOperation(token)
        assertEquals(releases, binder.unbindCount)
    }

    @Test
    fun runningGenerationJobOutlivesItsRpcAndReleasesAfterTerminalReadback() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()
        client.requestVpnGate("""{"command":"warp_wireguard","warp_wireguard":{"action":"generate"}}""", result)
        val id = "10000000-0000-4000-8000-000000000001"
        client.deliverVpnGateReply(
            endpoint.messages.single().requestId,
            """{"job":{"id":"$id","state":"running"},"error":null}""",
            null,
        )
        assertEquals(1, result.completionCount)
        assertTrue(client.creatingBindingForTest())
        client.setUiVisible(false)
        scheduler.advanceBy(1_000L)
        assertEquals(2, endpoint.messages.size)
        val poll = endpoint.messages.last()
        val command = JSONObject(poll.extras?.get("vpn_gate_request") as String)
        assertEquals("get", command.getJSONObject("warp_wireguard").getString("action"))
        client.deliverVpnGateReply(poll.requestId, """{"job":{"id":"$id","state":"completed"},"error":null}""", null)
        assertFalse(client.creatingBindingForTest())
        assertEquals(0, scheduler.pendingTimeoutCount())
    }

    @Test
    fun cancelledSessionWaitsForDelayedServiceCleanupReply() {
        val endpoint = RecordingEndpoint()
        val probeStarted = CountDownLatch(1)
        val cancellationSent = CountDownLatch(1)
        endpoint.onSend = { sent ->
            if (sent.what == UsqueVpnService.MSG_DIAGNOSTIC_PROBE) probeStarted.countDown()
            if (sent.what == UsqueVpnService.MSG_CANCEL_DIAGNOSTIC_PROBE) cancellationSent.countDown()
        }
        client.attachEndpointForTest(endpoint)
        val executor = Executors.newSingleThreadExecutor()
        try {
            val coordinator = AndroidDiagnosticsCoordinator(executor, networkProbe = client::runNetworkProbe)
            val started = coordinator.start("deep", mapOf("phase" to "disconnected"), true, true, true, true)
            assertTrue(probeStarted.await(2, TimeUnit.SECONDS))
            coordinator.cancel(started["session_id"] as String)
            assertTrue(cancellationSent.await(2, TimeUnit.SECONDS))
            val workerFinished = executor.submit {}
            assertThrows(TimeoutException::class.java) { workerFinished.get(150, TimeUnit.MILLISECONDS) }
            assertEquals("cancelling", coordinator.current()!!["state"])
            val id = endpoint.messages.first { it.what == UsqueVpnService.MSG_DIAGNOSTIC_PROBE }.requestId
            client.deliverNetworkProbeReply(id, "{\"code\":\"cancelled\"}")
            workerFinished.get(2, TimeUnit.SECONDS)
            assertEquals("cancelled", coordinator.current()!!["state"])
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun missingCleanupReplyFailsAndKeepsProbeOwnershipUntilLateActualReply() {
        val endpoint = RecordingEndpoint()
        val cancelled = AtomicBoolean()
        endpoint.onSend = { sent ->
            if (sent.what == UsqueVpnService.MSG_DIAGNOSTIC_PROBE) cancelled.set(true)
        }
        client.attachEndpointForTest(endpoint)
        val result = client.runNetworkProbe("transport.h3_path_validation_probe", cancelled::get)
        assertEquals("failed", result["status"])
        assertEquals(false, result["cleanup_confirmed"])
        assertEquals(listOf("probe_cleanup_unconfirmed"), result["sanitized_evidence"])
        val blocked = client.runNetworkProbe("dns.direct_encrypted_reachability") { false }
        assertEquals(false, blocked["cleanup_confirmed"])
        assertEquals(1, endpoint.messages.count { it.what == UsqueVpnService.MSG_DIAGNOSTIC_PROBE })
        val id = endpoint.messages.first { it.what == UsqueVpnService.MSG_DIAGNOSTIC_PROBE }.requestId
        client.deliverNetworkProbeReply(id + 1, "{\"code\":\"cancelled\"}")
        assertEquals(false, client.runNetworkProbe("dns.direct_encrypted_reachability") { false }["cleanup_confirmed"])
        client.deliverNetworkProbeReply(id, "{\"code\":\"cancelled\"}")
        endpoint.onSend = { sent ->
            if (sent.what == UsqueVpnService.MSG_DIAGNOSTIC_PROBE) {
                client.deliverNetworkProbeReply(sent.requestId, "{\"code\":\"passed\"}")
            }
        }
        assertEquals("passed", client.runNetworkProbe("dns.direct_encrypted_reachability") { false }["status"])
        assertEquals(2, endpoint.messages.count { it.what == UsqueVpnService.MSG_DIAGNOSTIC_PROBE })
    }

    @Test
    fun cleanupWaitCannotExtendExpiredFourSecondBudget() {
        val clock = AtomicLong()
        val boundedClient =
            VpnControlClient(
                scheduler,
                binder::bind,
                binder::unbind,
                { _, _ -> error("real Binder is unused") },
                nowNanos = clock::get,
            )
        val endpoint = RecordingEndpoint()
        endpoint.onSend = { sent ->
            if (sent.what == UsqueVpnService.MSG_DIAGNOSTIC_PROBE) clock.set(4_000_000_000L)
        }
        boundedClient.attachEndpointForTest(endpoint)
        val result =
            CompletableFuture
                .supplyAsync {
                    boundedClient.runNetworkProbe("transport.h3_path_validation_probe") { false }
                }.get(200, TimeUnit.MILLISECONDS)
        assertEquals(false, result["cleanup_confirmed"])
    }

    @Test
    fun losingBinderIsNotASyntheticCleanupAcknowledgement() {
        val endpoint = RecordingEndpoint()
        endpoint.onSend = { sent ->
            if (sent.what == UsqueVpnService.MSG_DIAGNOSTIC_PROBE) client.detachEndpointForTest()
        }
        client.attachEndpointForTest(endpoint)
        val result = client.runNetworkProbe("transport.h3_path_validation_probe") { false }
        assertEquals(false, result["cleanup_confirmed"])
        assertEquals("failed", result["status"])
        client.attachEndpointForTest(RecordingEndpoint())
        assertEquals(false, client.runNetworkProbe("transport.h3_path_validation_probe") { false }["cleanup_confirmed"])
    }

    @Test
    fun logCaptureIsBoundedAndLateTimeoutRepliesCannotCompleteTwice() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val replies = mutableListOf<AndroidLogStore.Snapshot?>()
        client.requestLogs(replies::add)
        val id = endpoint.messages.last().requestId
        assertEquals(UsqueVpnService.MSG_LOG_SNAPSHOT, endpoint.messages.last().what)
        scheduler.fireAllDelayed()
        assertEquals(listOf<AndroidLogStore.Snapshot?>(null), replies)
        client.deliverLogsReply(id, "{}")
        assertEquals(1, replies.size)
    }

    @Test
    fun logCaptureRevalidatesRecordsAndHealthAtTheMessengerBoundary() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        var snapshot: AndroidLogStore.Snapshot? = null
        client.requestLogs { snapshot = it }
        val id = endpoint.messages.last().requestId
        val line =
            JSONObject()
                .put("timestamp", "2026-09-30T00:00:00Z")
                .put("level", "WARN")
                .put("event", "CONNECTION_FAILED")
                .put("error_code", "ANDROID_RUNTIME_FAILED")
                .toString()
        client.deliverLogsReply(
            id,
            JSONObject(
                mapOf(
                    "lines" to "$line\n{\"token\":\"private-token\"}\n",
                    "health" to
                        mapOf("barrier_completed" to true, "written_count" to 1L, "private_path" to "private-file"),
                ),
            ).toString(),
        )
        assertEquals("$line\n", snapshot?.lines)
        assertEquals(1L, snapshot?.health?.get("omitted_line_count"))
        assertFalse(snapshot.toString().contains("private"))
    }

    @Test
    fun generationPendingFollowsCurrentJobUntilCompletion() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        client.requestVpnGate(
            """{"command":"warp_wireguard","warp_wireguard":{"action":"generate"}}""",
            RecordingResult(),
        )
        assertTrue(client.warpGenerationPending)
        client.deliverVpnGateReply(
            endpoint.messages.last().requestId,
            """{"job":{"id":"job","state":"running"}}""",
            null,
        )
        assertTrue(client.warpGenerationPending)
        client.requestVpnGate(
            """{"command":"warp_wireguard","warp_wireguard":{"action":"get","job_id":"job"}}""",
            RecordingResult(),
        )
        client.deliverVpnGateReply(
            endpoint.messages.last().requestId,
            """{"job":{"id":"job","state":"completed","profile_id":"saved"}}""",
            null,
        )
        assertFalse(client.warpGenerationPending)
    }

    @Test
    fun disconnectedWarpRequestsBindAndPreserveSafeNativeFailureCodes() {
        for ((nativeError, expected) in listOf(
            "identity_required" to "identity_required",
            "secret-token" to "unavailable",
        )) {
            client.detachEndpointForTest()
            val result = RecordingResult()
            client.requestVpnGate("""{"command":"warp_wireguard","warp_wireguard":{"action":"generate"}}""", result)
            assertEquals(0, result.completionCount)
            val endpoint = RecordingEndpoint()
            client.attachEndpointForTest(endpoint)
            val request = endpoint.messages.single()
            assertEquals(UsqueVpnService.MSG_VPN_GATE, request.what)
            client.deliverVpnGateReply(request.requestId, null, nativeError)
            assertEquals(expected, result.errorCode)
            assertEquals(1, result.completionCount)
        }
    }

    @Test
    fun retryWaitsForBindingAndDisconnectCancelsAnUnsentRetry() {
        val retry = RecordingResult()
        client.requestRetry(retry)
        assertEquals(0, retry.completionCount)
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        assertEquals(UsqueVpnService.MSG_RETRY, endpoint.messages.single().what)
        client.deliverSnapshotReply(endpoint.messages.single().requestId, null, null, mapOf("phase" to "preparing"))
        assertEquals(1, retry.completionCount)

        client.detachEndpointForTest()
        val cancelled = RecordingResult()
        client.requestRetry(cancelled)
        client.requestDisconnect(RecordingResult())
        assertEquals("ENGINE_REQUEST_CANCELLED", cancelled.errorCode)
        val next = RecordingEndpoint()
        client.attachEndpointForTest(next)
        assertEquals(UsqueVpnService.MSG_DISCONNECT, next.messages.single().what)
    }

    @Test
    fun anUnboundRetryTimesOutWithoutClaimingDisconnection() {
        val result = RecordingResult()
        client.requestRetry(result)
        scheduler.fireAllDelayed()
        assertEquals("ENGINE_IPC_TIMEOUT", result.errorCode)
        assertEquals(1, result.completionCount)
    }

    private lateinit var scheduler: FakeMainScheduler
    private lateinit var binder: RecordingServiceBinder
    private lateinit var client: VpnControlClient
    private val events = mutableListOf<Map<String, Any?>>()
    private val clearAllAcks = mutableListOf<MethodChannel.Result>()

    @Before
    fun setUp() {
        scheduler = FakeMainScheduler()
        binder = RecordingServiceBinder()
        client =
            VpnControlClient(
                scheduler = scheduler,
                serviceBinder = binder::bind,
                serviceUnbinder = binder::unbind,
                endpointFromBinder = { _, _ -> error("real binder not used in unit tests") },
                snapshotTimeoutMillis = 2_000L,
                clearAllTimeoutMillis = 45_000L,
            )
        client.eventListener = VpnControlClient.EventListener { events.add(it) }
        client.clearAllAcknowledgedListener =
            VpnControlClient.ClearAllAcknowledgedListener { clearAllAcks.add(it) }
    }

    @Test
    fun settingsTimeoutNeverReplaysAndLateReplyCannotReportSuccess() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()
        client.requestNetworkSettings("""{"operation_id":"op"}""", result)
        assertEquals(UsqueVpnService.MSG_SAVE_SETTINGS, endpoint.messages.single().what)
        val id = endpoint.messages.single().requestId
        scheduler.fireAllDelayed()
        assertEquals("NETWORK_SETTINGS_UNCONFIRMED", result.errorCode)
        client.attachEndpointForTest(endpoint)
        client.deliverSettingsReply(id, """{"source_epoch":"one","sequence":1,"persisted":true}""")
        assertEquals(1, result.completionCount)
        assertEquals(1, endpoint.messages.size)
    }

    @Test
    fun localeUpdateIsRetainedUntilTheVpnProcessIsReachable() {
        client.updateLocale("ja")
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)

        assertEquals(UsqueVpnService.MSG_UPDATE_LOCALE, endpoint.messages.single().what)
        assertEquals(
            "ja",
            endpoint.messages
                .single()
                .extras
                ?.get("catalog_id"),
        )

        endpoint.messages.clear()
        client.updateLocale("zh_TW")
        assertEquals(UsqueVpnService.MSG_UPDATE_LOCALE, endpoint.messages.single().what)
        assertEquals(
            "zh_TW",
            endpoint.messages
                .single()
                .extras
                ?.get("catalog_id"),
        )
    }

    @Test
    fun settingsReplyKeepsPersistenceSeparateFromRuntimeAndStripsPrivateTarget() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()
        client.requestNetworkSettings("{}", result)
        val id = endpoint.messages.single().requestId
        client.deliverSettingsReply(
            id,
            """{"source_epoch":"one","sequence":1,"persisted":true,"apply_status":"failed","target":{"mtu":1400}}""",
        )
        val state = result.successValue as Map<*, *>
        assertEquals(true, state["persisted"])
        assertEquals("failed", state["apply_status"])
        assertFalse(state.containsKey("target"))
        client.deliverSettingsReply(id, "{}")
        scheduler.fireAllDelayed()
        assertEquals(1, result.completionCount)
    }

    @Test
    fun routingConflictReplyReachesFlutterAsADefinitiveError() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()
        client.requestNetworkSettings("{}", result)
        val token = "ROUTING_RULE_CONFLICT:00112233-4455-4677-8899-aabbccddeeff:11223344-5566-4788-9900-aabbccddeeff"
        client.deliverSettingsReply(
            endpoint.messages.single().requestId,
            null,
            RoutingSettingsError.nativeFailure("JNI: $token"),
        )
        assertEquals("ROUTING_RULE_CONFLICT", result.errorCode)
        assertEquals(token, result.errorMessage)
        scheduler.fireAllDelayed()
        assertEquals(1, result.completionCount)
    }

    @Test
    fun timelineRequestIsSingleFlightTimesOutAndIgnoresLateReplies() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        var completed = 0
        client.requestTimeline {
            assertNull(it)
            completed++
        }
        val id = endpoint.messages.single().requestId
        client.requestTimeline {
            assertNull(it)
            completed++
        }
        assertEquals(1, endpoint.messages.size)
        scheduler.fireAllDelayed()
        assertEquals(2, completed)
        client.deliverTimelineReply(id, "{}")
        assertEquals(2, completed)
    }

    @Test
    fun timelineReplyIsDeliveredOnceAndDestroyCancelsPendingRead() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        var completed = 0
        client.requestTimeline {
            assertNotNull(it)
            completed++
        }
        val id = endpoint.messages.single().requestId
        client.deliverTimelineReply(id, """{"schema_version":1,"events":[],"metrics":{}}""")
        scheduler.fireAllDelayed()
        assertEquals(1, completed)
        client.requestTimeline {
            assertNull(it)
            completed++
        }
        client.destroy()
        assertEquals(2, completed)
        scheduler.fireAllDelayed()
        assertEquals(2, completed)
    }

    @Test
    fun requestSnapshotTimesOutExactlyOnce() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()

        client.requestSnapshot(result)

        assertEquals(1, endpoint.messages.size)
        assertEquals(UsqueVpnService.MSG_SNAPSHOT, endpoint.messages.single().what)
        assertEquals(1, client.pendingSnapshotCountForTest())

        scheduler.fireAllDelayed()

        assertEquals("ENGINE_IPC_TIMEOUT", result.errorCode)
        assertEquals(1, result.completionCount)
        assertEquals(0, client.pendingSnapshotCountForTest())

        // Firing again must not complete a second time.
        scheduler.fireAllDelayed()
        assertEquals(1, result.completionCount)
    }

    @Test
    fun snapshotReplyCancelsTimeoutAndCompletesOnce() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()

        client.requestSnapshot(result)
        val requestId = endpoint.messages.single().requestId
        client.deliverSnapshotReply(
            requestId,
            errorCode = null,
            errorMessage = null,
            snapshot = mapOf("phase" to "connected"),
        )

        assertEquals("connected", (result.successValue as Map<*, *>)["phase"])
        assertEquals(1, result.completionCount)
        assertEquals(0, client.pendingSnapshotCountForTest())

        scheduler.fireAllDelayed()
        assertEquals(1, result.completionCount)
    }

    @Test
    fun diagnosticProbePassesOnlyAfterABoundedSnapshotReply() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        var probe: VpnControlClient.SnapshotProbe? = null

        client.probeSnapshot { probe = it }

        assertNull(probe)
        assertEquals(1, client.pendingDiagnosticProbeCountForTest())
        val requestId = endpoint.messages.single().requestId
        client.deliverSnapshotReply(
            requestId,
            errorCode = null,
            errorMessage = null,
            snapshot = mapOf("phase" to "connected"),
        )

        assertEquals(true, probe?.controlReachable)
        assertEquals("connected", probe?.snapshot?.get("phase"))
        assertEquals(0, client.pendingDiagnosticProbeCountForTest())
        scheduler.fireAllDelayed()
        assertEquals(true, probe?.controlReachable)
    }

    @Test
    fun diagnosticProbeTimeoutFailsClosedWithTheLastSnapshot() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        var probe: VpnControlClient.SnapshotProbe? = null

        client.probeSnapshot { probe = it }
        scheduler.fireAllDelayed()

        assertEquals(false, probe?.controlReachable)
        assertEquals(0, client.pendingDiagnosticProbeCountForTest())
    }

    @Test
    fun destroyCompletesPendingResultsOnlyOnce() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val snapshotResult = RecordingResult()
        val clearResult = RecordingResult()
        val disconnectResult = RecordingResult()

        client.requestSnapshot(snapshotResult)
        assertTrue(client.requestClearAllData(clearResult))
        client.detachEndpointForTest()
        client.requestDisconnect(disconnectResult)

        client.destroy()

        assertEquals("ENGINE_IPC_CLOSED", snapshotResult.errorCode)
        assertEquals("CLEAR_ALL_CANCELLED", clearResult.errorCode)
        assertEquals("ENGINE_IPC_CLOSED", disconnectResult.errorCode)
        assertEquals(1, snapshotResult.completionCount)
        assertEquals(1, clearResult.completionCount)
        assertEquals(1, disconnectResult.completionCount)

        client.destroy()
        assertEquals(1, snapshotResult.completionCount)
        assertEquals(1, clearResult.completionCount)
        assertEquals(1, disconnectResult.completionCount)
    }

    @Test
    fun destroyCancelsOutstandingTimeouts() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()

        client.requestSnapshot(result)
        assertEquals(1, scheduler.pendingTimeoutCount())

        client.destroy()

        assertEquals(0, scheduler.pendingTimeoutCount())
        assertEquals(1, result.completionCount)
        scheduler.fireAllDelayed()
        assertEquals(1, result.completionCount)
    }

    @Test
    fun destroyDoesNotRecompleteAlreadyFinishedResults() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()

        client.requestSnapshot(result)
        val requestId = endpoint.messages.single().requestId
        client.deliverSnapshotReply(requestId, null, null, mapOf("phase" to "disconnected"))
        assertEquals(1, result.completionCount)

        client.destroy()
        assertEquals(1, result.completionCount)
    }

    @Test
    fun visibleBindingDiedRebindsWithoutCreatingTheService() {
        assertFalse(client.isBound)
        client.setUiVisible(true)
        assertTrue(client.isBound)
        assertEquals(1, binder.bindCount)

        client.notifyBindingDiedForTest()

        assertEquals(1, binder.unbindCount)
        assertEquals(2, binder.bindCount)
        assertTrue(client.isBound)
        assertTrue(binder.creatingFlags.none { it })
    }

    @Test
    fun bindingDiedDoesNotCompletePendingSnapshot() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        client.bind()
        val result = RecordingResult()

        client.requestSnapshot(result)
        val requestId = endpoint.messages.single().requestId
        val messagesBeforeReconnect = endpoint.messages.size

        client.notifyBindingDiedForTest()

        assertEquals(0, result.completionCount)
        assertEquals(1, client.pendingSnapshotCountForTest())
        // Reconnect must not re-send the in-flight snapshot.
        assertEquals(messagesBeforeReconnect, endpoint.messages.size)
        assertEquals(2, binder.bindCount)

        scheduler.fireAllDelayed()
        assertEquals("ENGINE_IPC_TIMEOUT", result.errorCode)
        assertEquals(1, result.completionCount)

        // Late reply after timeout must not double-complete.
        client.deliverSnapshotReply(requestId, null, null, mapOf("phase" to "connected"))
        assertEquals(1, result.completionCount)
    }

    @Test
    fun notifyApplyPerAppRetainsNewestRevisionUntilBound() {
        client.notifyApplyPerApp(2)
        client.notifyApplyPerApp(3)
        assertEquals(1, binder.bindCount)

        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        assertEquals(UsqueVpnService.MSG_APPLY_PER_APP, endpoint.messages.single().what)
        assertEquals(
            3L,
            endpoint.messages
                .single()
                .extras
                ?.get("revision"),
        )
        client.notifyApplyPerApp(4)
        assertEquals(
            4L,
            endpoint.messages
                .last()
                .extras
                ?.get("revision"),
        )
    }

    @Test
    fun disconnectWaitsForReconnectThenSends() {
        val result = RecordingResult()
        client.requestDisconnect(result)
        assertNotNull(client.pendingDisconnectForTest())
        assertEquals(1, binder.bindCount)

        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)

        assertNull(client.pendingDisconnectForTest())
        assertEquals(UsqueVpnService.MSG_DISCONNECT, endpoint.messages.single().what)
        assertEquals(0, result.completionCount)
    }

    @Test
    fun requestReconfigureUsesNativeAlignedTimeout() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()

        assertTrue(client.requestReconfigure("""{"id":"p1"}""", result))

        assertEquals(UsqueVpnService.MSG_RECONFIGURE, endpoint.messages.single().what)
        assertEquals(listOf(VpnControlClient.RECONFIGURE_TIMEOUT_MILLIS), scheduler.pendingDelays())
        assertEquals(1, client.pendingSnapshotCountForTest())

        scheduler.fireAllDelayed()

        assertEquals("ENGINE_IPC_TIMEOUT", result.errorCode)
        assertEquals(1, result.completionCount)
        assertEquals(0, client.pendingSnapshotCountForTest())
    }

    @Test
    fun reconfigureWaitsForReconnectThenSends() {
        val result = RecordingResult()
        val profileJson = """{"id":"p1","mode":"vpn"}"""

        assertTrue(client.requestReconfigure(profileJson, result))
        assertSame(result, client.pendingReconfigureForTest())
        assertEquals(profileJson, client.pendingReconfigureProfileForTest())
        assertEquals(1, binder.bindCount)
        assertEquals(listOf(VpnControlClient.RECONFIGURE_TIMEOUT_MILLIS), scheduler.pendingDelays())
        assertEquals(0, result.completionCount)

        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)

        assertNull(client.pendingReconfigureForTest())
        assertEquals(UsqueVpnService.MSG_RECONFIGURE, endpoint.messages.single().what)
        assertEquals(
            profileJson,
            endpoint.messages
                .single()
                .extras
                ?.get(UsqueVpnService.EXTRA_PROFILE_JSON),
        )
        assertEquals(0, result.completionCount)
        assertEquals(1, client.pendingSnapshotCountForTest())
    }

    @Test
    fun newestPendingAccountSelectionSurvivesRebinding() {
        val b = RecordingResult()
        val c = RecordingResult()
        assertTrue(client.requestReconfigure("""{"id":"b"}""", b, accountSelection = true))
        assertTrue(client.requestReconfigure("""{"id":"c"}""", c, accountSelection = true))
        assertEquals(1, b.completionCount)
        assertNull(b.errorCode)
        assertSame(c, client.pendingReconfigureForTest())
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        assertEquals(UsqueVpnService.MSG_RECONFIGURE, endpoint.messages.single().what)
        assertEquals(
            true,
            endpoint.messages
                .single()
                .extras
                ?.get("account_selection"),
        )
        assertEquals(
            """{"id":"c"}""",
            endpoint.messages
                .single()
                .extras
                ?.get(UsqueVpnService.EXTRA_PROFILE_JSON),
        )
        assertEquals(1, b.completionCount)
        assertEquals(0, c.completionCount)
    }

    @Test
    fun reconfigureRejectsConcurrentUnboundRequest() {
        val first = RecordingResult()
        val second = RecordingResult()

        assertTrue(client.requestReconfigure("""{"id":"p1"}""", first))
        assertTrue(client.requestReconfigure("""{"id":"p2"}""", second))

        assertEquals("RECONFIGURE_IN_PROGRESS", second.errorCode)
        assertEquals(1, second.completionCount)
        assertEquals(0, first.completionCount)
        assertSame(first, client.pendingReconfigureForTest())
        assertEquals("""{"id":"p1"}""", client.pendingReconfigureProfileForTest())
    }

    @Test
    fun destroyCompletesPendingReconfigureOnlyOnce() {
        val result = RecordingResult()
        assertTrue(client.requestReconfigure("""{"id":"p1"}""", result))
        assertEquals(1, scheduler.pendingTimeoutCount())

        client.destroy()

        assertEquals("ENGINE_IPC_CLOSED", result.errorCode)
        assertEquals(1, result.completionCount)
        assertNull(client.pendingReconfigureForTest())
        assertEquals(0, scheduler.pendingTimeoutCount())

        client.destroy()
        scheduler.fireAllDelayed()
        assertEquals(1, result.completionCount)
    }

    @Test
    fun clearAllAcknowledgementDelegatesToListener() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()

        assertTrue(client.requestClearAllData(result))
        val requestId = endpoint.messages.single().requestId
        assertEquals(UsqueVpnService.MSG_CLEAR_ALL_DATA, endpoint.messages.single().what)

        client.deliverSnapshotReply(requestId, null, null, mapOf("phase" to "disconnected"))

        assertEquals(1, clearAllAcks.size)
        assertSame(result, client.inFlightClearAllForTest())
        assertEquals(0, result.completionCount)
    }

    @Test
    fun clearAllAckThenDestroyCompletesOnceWithCancelled() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()

        assertTrue(client.requestClearAllData(result))
        val requestId = endpoint.messages.single().requestId
        client.deliverSnapshotReply(requestId, null, null, mapOf("phase" to "disconnected"))

        assertEquals(0, result.completionCount)
        assertNotNull(client.inFlightClearAllForTest())

        // Activity destroy before local wipe finishes.
        client.destroy()

        assertEquals("CLEAR_ALL_CANCELLED", result.errorCode)
        assertEquals(1, result.completionCount)
        assertNull(client.inFlightClearAllForTest())

        // A cancelled acknowledgement can no longer be claimed by a wipe worker.
        assertFalse(client.claimInFlightClearAll(result))
        client.destroy()
        assertEquals(1, result.completionCount)
    }

    @Test
    fun clearAllLocalWipeCompletesOnceAfterAck() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()

        assertTrue(client.requestClearAllData(result))
        client.deliverSnapshotReply(
            endpoint.messages.single().requestId,
            null,
            null,
            mapOf("phase" to "disconnected"),
        )
        assertTrue(client.claimInFlightClearAll(result))
        assertSame(result, client.claimedClearAllForTest())
        assertTrue(client.takeClaimedClearAll(result))
        result.success(null)

        assertEquals(1, result.completionCount)
        assertNull(client.inFlightClearAllForTest())
        assertNull(client.claimedClearAllForTest())
        client.destroy()
        assertEquals(1, result.completionCount)
    }

    @Test
    fun clearAllFailsClosedWhenListenerMissing() {
        client.clearAllAcknowledgedListener = null
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()

        assertTrue(client.requestClearAllData(result))
        client.deliverSnapshotReply(
            endpoint.messages.single().requestId,
            null,
            null,
            mapOf("phase" to "disconnected"),
        )

        assertEquals("CLEAR_ALL_FAILED", result.errorCode)
        assertEquals(1, result.completionCount)
        assertNull(client.inFlightClearAllForTest())
    }

    @Test
    fun clearAllRejectsConcurrentRequestWhileIpcPending() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val first = RecordingResult()
        val second = RecordingResult()

        assertTrue(client.requestClearAllData(first))
        assertFalse(client.requestClearAllData(second))

        assertEquals("CLEAR_ALL_IN_PROGRESS", second.errorCode)
        assertEquals(1, second.completionCount)
        assertEquals(0, first.completionCount)
        assertEquals(1, endpoint.messages.size)
    }

    @Test
    fun clearAllRejectsConcurrentRequestWhileLocalWipeClaimed() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val first = RecordingResult()
        val second = RecordingResult()

        assertTrue(client.requestClearAllData(first))
        client.deliverSnapshotReply(
            endpoint.messages.single().requestId,
            null,
            null,
            mapOf("phase" to "disconnected"),
        )
        assertTrue(client.claimInFlightClearAll(first))
        assertSame(first, client.claimedClearAllForTest())
        assertEquals(0, first.completionCount)

        assertFalse(client.requestClearAllData(second))
        assertEquals("CLEAR_ALL_IN_PROGRESS", second.errorCode)
        assertEquals(1, second.completionCount)
        assertSame(first, client.claimedClearAllForTest())
        assertEquals(0, first.completionCount)
    }

    @Test
    fun eventDeliveryUpdatesLastSnapshot() {
        client.setUiVisible(true)
        client.setEventsWanted(true)
        client.deliverEvent(mapOf("phase" to "connected", "transport" to "h3"))
        assertEquals("connected", client.lastSnapshot["phase"])
        assertEquals(1, events.size)
    }

    @Test
    fun eventStreamIsReachableOnlyAfterAnObservedSubscribedEvent() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)

        client.setUiVisible(true)
        client.setEventsWanted(true)
        assertFalse(client.eventStreamReachable)

        client.deliverEvent(mapOf("phase" to "connected"))
        assertTrue(client.eventStreamReachable)

        client.setEventsWanted(false)
        assertFalse(client.eventStreamReachable)
    }

    @Test
    fun backgroundReleasesIdleObservationAndResumeBindsWithoutCreating() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        client.setEventsWanted(true)
        assertTrue(endpoint.messages.isEmpty())
        assertEquals(0, scheduler.pendingTimeoutCount())

        client.setUiVisible(true)
        client.deliverEvent(mapOf("phase" to "connected"))
        assertTrue(client.eventStreamReachable)

        client.setUiVisible(false)
        assertFalse(client.eventStreamReachable)
        assertFalse(client.isBound)
        assertEquals(0, scheduler.pendingTimeoutCount())
        // Already queued events must not replace the last visible state.
        client.deliverEvent(mapOf("phase" to "preparing"))
        assertEquals("connected", client.lastSnapshot["phase"])
        assertEquals(1, events.size)
        scheduler.advanceBy(7_200_000L)

        // Flutter can also replace its subscription while the Activity is stopped.
        client.setEventsWanted(true)
        assertEquals(2, endpoint.messages.size)
        client.setUiVisible(true)
        assertEquals(
            listOf(
                UsqueVpnService.MSG_REGISTER_EVENTS,
                UsqueVpnService.MSG_UNREGISTER_EVENTS,
                UsqueVpnService.MSG_REGISTER_EVENTS,
            ),
            endpoint.messages.map { it.what },
        )
        assertEquals(2, binder.bindCount)
        assertEquals(1, binder.unbindCount)
        assertTrue(binder.creatingFlags.none { it })
        assertFalse(client.eventStreamReachable)
        client.deliverEvent(mapOf("phase" to "connected", "transport" to "h2"))
        assertEquals("h2", events.last()["transport"])
        assertTrue(client.eventStreamReachable)
    }

    @Test
    fun cancelledDartSubscriptionIsNotRestoredOnResume() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        client.setUiVisible(true)
        client.setEventsWanted(true)
        client.setUiVisible(false)
        client.setEventsWanted(false)
        val messagesBeforeResume = endpoint.messages.size

        client.setUiVisible(true)
        scheduler.advanceBy(20_000L)
        client.deliverEvent(mapOf("phase" to "connected"))
        assertEquals(messagesBeforeResume, endpoint.messages.size)
        assertTrue(events.isEmpty())
        assertEquals(0, scheduler.pendingTimeoutCount())
    }

    @Test
    fun serviceRebindingWhileHiddenWaitsForTheVisibleSubscriber() {
        val endpoint = RecordingEndpoint()
        client.setEventsWanted(true)
        client.attachEndpointForTest(endpoint)
        assertTrue(endpoint.messages.isEmpty())

        client.setUiVisible(true)
        assertEquals(UsqueVpnService.MSG_REGISTER_EVENTS, endpoint.messages.single().what)
    }

    @Test
    fun silentPushLossAfterReconnectRecoversTheUiWithoutReplayingCommands() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        client.setUiVisible(true)
        client.setEventsWanted(true)
        client.deliverEvent(mapOf("phase" to "connected"))

        // The disconnect request/reply remains usable after the event subscriber
        // disappears. A new connection is started by MainActivity's Intent path.
        val disconnected = RecordingResult()
        client.requestDisconnect(disconnected)
        client.deliverSnapshotReply(
            endpoint.messages.last().requestId,
            null,
            null,
            mapOf("phase" to "disconnected"),
        )
        assertEquals("disconnected", (disconnected.successValue as Map<*, *>)["phase"])
        var uiPhase = "preparing" // MainActivity's initial connect acknowledgement.
        client.eventListener = VpnControlClient.EventListener { uiPhase = it["phase"] as String }
        // Like MSG_REGISTER_EVENTS in the real service, re-registration returns
        // its current snapshot even when ordinary broadcasts were deduplicated.
        endpoint.onSend = { message ->
            if (message.what == UsqueVpnService.MSG_REGISTER_EVENTS) {
                client.deliverEvent(mapOf("phase" to "connected"))
            }
        }

        scheduler.advanceBy(VpnControlClient.EVENT_REFRESH_INTERVAL_MILLIS - 1)
        assertEquals("preparing", uiPhase)
        scheduler.advanceBy(1)
        assertEquals("connected", uiPhase)
        assertTrue(client.eventStreamReachable)
        assertEquals(
            listOf(
                UsqueVpnService.MSG_REGISTER_EVENTS,
                UsqueVpnService.MSG_DISCONNECT,
                UsqueVpnService.MSG_REGISTER_EVENTS,
            ),
            endpoint.messages.map { it.what },
        )
        assertEquals(0, client.pendingSnapshotCountForTest())
        assertEquals(1, scheduler.pendingTimeoutCount())
    }

    @Test
    fun liveSnapshotsPostponeTheSingleRefreshDeadline() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        client.setUiVisible(true)
        client.setEventsWanted(true)
        scheduler.advanceBy(VpnControlClient.EVENT_REFRESH_INTERVAL_MILLIS - 1)

        client.deliverEvent(mapOf("phase" to "connected"))
        scheduler.advanceBy(1)
        assertEquals(1, endpoint.messages.size)
        assertEquals(1, scheduler.pendingTimeoutCount())
        scheduler.advanceBy(VpnControlClient.EVENT_REFRESH_INTERVAL_MILLIS - 1)
        assertEquals(2, endpoint.messages.size)
        assertEquals(1, scheduler.pendingTimeoutCount())
    }

    @Test
    fun missingRefreshRepliesNeverInventConnectedStateOrAccumulateRequests() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        client.setUiVisible(true)
        client.setEventsWanted(true)
        client.deliverEvent(mapOf("phase" to "preparing"))

        scheduler.advanceBy(3 * VpnControlClient.EVENT_REFRESH_INTERVAL_MILLIS)
        assertEquals("preparing", client.lastSnapshot["phase"])
        assertEquals(1, events.size)
        assertFalse(client.eventStreamReachable)
        assertEquals(4, endpoint.messages.size)
        assertTrue(endpoint.messages.all { it.what == UsqueVpnService.MSG_REGISTER_EVENTS })
        assertEquals(0, client.pendingSnapshotCountForTest())
        assertEquals(1, scheduler.pendingTimeoutCount())
    }

    @Test
    fun failedEventSendRebindsAndRecoversWithoutReplayingTheDisconnect() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        client.setUiVisible(true)
        client.setEventsWanted(true)
        client.deliverEvent(mapOf("phase" to "connected"))

        val disconnect = RecordingResult()
        endpoint.succeeds = false
        client.requestDisconnect(disconnect)
        assertEquals("ENGINE_IPC_UNAVAILABLE", disconnect.errorCode)
        assertFalse(client.hasEndpoint)
        assertTrue(client.isBound)
        assertFalse(client.creatingBindingForTest())

        scheduler.advanceBy(VpnControlClient.EVENT_REFRESH_INTERVAL_MILLIS - 1)
        client.deliverEvent(mapOf("phase" to "connected"))
        scheduler.advanceBy(1)
        assertEquals(2, binder.unbindCount)
        assertEquals(3, binder.bindCount)
        assertEquals(listOf(false, true, false), binder.creatingFlags)
        val replacement = RecordingEndpoint()
        client.attachEndpointForTest(replacement)
        assertEquals(UsqueVpnService.MSG_REGISTER_EVENTS, replacement.messages.single().what)
        client.deliverEvent(mapOf("phase" to "disconnected"))
        assertEquals("disconnected", events.last()["phase"])
        assertTrue(client.eventStreamReachable)
        assertEquals(1, disconnect.completionCount)
    }

    @Test
    fun missingServiceConnectionCallbackIsRetriedOnlyWhileVisible() {
        client.setUiVisible(true)
        client.setEventsWanted(true)
        assertEquals(1, binder.bindCount)
        scheduler.advanceBy(VpnControlClient.EVENT_REFRESH_INTERVAL_MILLIS)
        assertEquals(2, binder.bindCount)
        assertEquals(1, binder.unbindCount)

        client.setUiVisible(false)
        scheduler.advanceBy(20_000L)
        assertEquals(2, binder.bindCount)
        assertEquals(0, scheduler.pendingTimeoutCount())
    }

    @Test
    fun stoppedCancelledAndDestroyedSubscribersRejectQueuedRefreshes() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        client.setUiVisible(true)
        client.setEventsWanted(true)
        val beforeStop = scheduler.pendingActions().single()
        client.setUiVisible(false)
        client.setUiVisible(true)
        val messagesAfterResume = endpoint.messages.size
        beforeStop()
        assertEquals(messagesAfterResume, endpoint.messages.size)

        val beforeCancel = scheduler.pendingActions().single()
        client.setEventsWanted(false)
        client.setEventsWanted(true)
        val messagesAfterListen = endpoint.messages.size
        beforeCancel()
        assertEquals(messagesAfterListen, endpoint.messages.size)

        val beforeDestroy = scheduler.pendingActions().single()
        client.destroy()
        val messagesAfterDestroy = endpoint.messages.size
        beforeDestroy()
        client.setUiVisible(true)
        client.setEventsWanted(true)
        scheduler.advanceBy(20_000L)
        assertEquals(messagesAfterDestroy, endpoint.messages.size)
        assertEquals(0, scheduler.pendingTimeoutCount())
    }

    @Test
    fun unavailableEndpointReturnsDisconnectedSnapshot() {
        val result = RecordingResult()
        client.requestSnapshot(result)
        val value = result.successValue as Map<*, *>
        assertEquals("disconnected", value["phase"])
        assertTrue(binder.bindCount >= 1)
    }

    private class RecordingServiceBinder {
        var bindCount = 0
        var unbindCount = 0

        val creatingFlags = mutableListOf<Boolean>()

        fun bind(
            connection: ServiceConnection,
            creating: Boolean,
        ): Boolean {
            creatingFlags += creating
            bindCount += 1
            return true
        }

        fun unbind(connection: ServiceConnection) {
            unbindCount += 1
        }
    }

    private class RecordingEndpoint : VpnControlClient.ControlEndpoint {
        data class Sent(
            val what: Int,
            val requestId: Int,
            val extras: Map<String, Any?>?,
        )

        val messages = mutableListOf<Sent>()
        var succeeds = true
        var onSend: ((Sent) -> Unit)? = null

        override fun send(
            what: Int,
            requestId: Int,
            extras: Map<String, Any?>?,
        ): Boolean {
            val message = Sent(what, requestId, extras)
            messages.add(message)
            if (succeeds) onSend?.invoke(message)
            return succeeds
        }
    }

    private class FakeMainScheduler : VpnControlClient.MainScheduler {
        private data class Delayed(
            val delayMillis: Long,
            val dueAtMillis: Long,
            val token: Any,
            val action: () -> Unit,
        )

        private val delayed = mutableListOf<Delayed>()
        private var nowMillis = 0L

        override fun post(action: () -> Unit) {
            action()
        }

        override fun postDelayed(
            delayMillis: Long,
            token: Any,
            action: () -> Unit,
        ) {
            cancel(token)
            delayed.add(Delayed(delayMillis, nowMillis + delayMillis, token, action))
        }

        override fun cancel(token: Any) {
            delayed.removeAll { it.token == token }
        }

        fun fireAllDelayed() {
            val snapshot = delayed.toList()
            delayed.clear()
            snapshot.forEach { it.action() }
        }

        fun pendingTimeoutCount(): Int = delayed.size

        fun pendingDelays(): List<Long> = delayed.map { it.delayMillis }

        fun pendingActions(): List<() -> Unit> = delayed.map { it.action }

        fun advanceBy(millis: Long) {
            val end = nowMillis + millis
            while (true) {
                val next = delayed.minByOrNull { it.dueAtMillis } ?: break
                if (next.dueAtMillis > end) break
                delayed.remove(next)
                nowMillis = next.dueAtMillis
                next.action()
            }
            nowMillis = end
        }
    }

    private class RecordingResult : MethodChannel.Result {
        var completionCount = 0
        var successValue: Any? = null
        var errorCode: String? = null
        var errorMessage: String? = null

        override fun success(result: Any?) {
            completionCount += 1
            successValue = result
        }

        override fun error(
            errorCode: String,
            errorMessage: String?,
            errorDetails: Any?,
        ) {
            completionCount += 1
            this.errorCode = errorCode
            this.errorMessage = errorMessage
        }

        override fun notImplemented() {
            completionCount += 1
        }
    }
}
