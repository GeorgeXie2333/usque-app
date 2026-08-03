package io.github.georgexie2333.usque

import android.content.ServiceConnection
import io.flutter.plugin.common.MethodChannel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class VpnControlClientTest {
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
    fun bindingDiedRebindsControlService() {
        assertFalse(client.isBound)
        client.bind()
        assertTrue(client.isBound)
        assertEquals(1, binder.bindCount)

        client.notifyBindingDiedForTest()

        assertEquals(1, binder.unbindCount)
        assertEquals(2, binder.bindCount)
        assertTrue(client.isBound)
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
    fun clearAllAcknowledgementDelegatesToListener() {
        val endpoint = RecordingEndpoint()
        client.attachEndpointForTest(endpoint)
        val result = RecordingResult()

        assertTrue(client.requestClearAllData(result))
        val requestId = endpoint.messages.single().requestId
        assertEquals(UsqueVpnService.MSG_CLEAR_ALL_DATA, endpoint.messages.single().what)

        client.deliverSnapshotReply(requestId, null, null, mapOf("phase" to "disconnected"))

        assertEquals(1, clearAllAcks.size)
        assertEquals(0, result.completionCount)
    }

    @Test
    fun eventDeliveryUpdatesLastSnapshot() {
        client.deliverEvent(mapOf("phase" to "connected", "transport" to "h3"))
        assertEquals("connected", client.lastSnapshot["phase"])
        assertEquals(1, events.size)
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

        fun bind(connection: ServiceConnection): Boolean {
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

        override fun send(
            what: Int,
            requestId: Int,
            extras: Map<String, Any?>?,
        ): Boolean {
            messages.add(Sent(what, requestId, extras))
            return true
        }
    }

    private class FakeMainScheduler : VpnControlClient.MainScheduler {
        private data class Delayed(
            val delayMillis: Long,
            val token: Any,
            val action: () -> Unit,
        )

        private val delayed = mutableListOf<Delayed>()

        override fun post(action: () -> Unit) {
            action()
        }

        override fun postDelayed(
            delayMillis: Long,
            token: Any,
            action: () -> Unit,
        ) {
            cancel(token)
            delayed.add(Delayed(delayMillis, token, action))
        }

        override fun cancel(token: Any) {
            delayed.removeAll { it.token == token }
        }

        fun fireAllDelayed() {
            val snapshot = delayed.toList()
            delayed.clear()
            snapshot.forEach { it.action() }
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
