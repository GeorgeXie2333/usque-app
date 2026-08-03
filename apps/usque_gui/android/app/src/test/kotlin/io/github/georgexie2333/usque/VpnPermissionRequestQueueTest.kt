package io.github.georgexie2333.usque

import io.flutter.plugin.common.MethodChannel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class VpnPermissionRequestQueueTest {
    @Test
    fun permitsOnlyOnePendingConnection() {
        val queue = VpnPermissionRequestQueue()
        val first = RecordingResult()
        val second = RecordingResult()

        assertTrue(queue.offer("""{"mode":"vpn","id":"one"}""", first))
        assertFalse(queue.offer("""{"mode":"vpn","id":"two"}""", second))
        assertTrue(queue.hasPending)

        val pending = queue.take()
        assertEquals("""{"mode":"vpn","id":"one"}""", pending?.profileJson)
        assertSame(first, pending?.result)
        assertFalse(queue.hasPending)
        assertNull(queue.take())
    }

    @Test
    fun cancellationCompletesThePendingResultExactlyOnce() {
        val queue = VpnPermissionRequestQueue()
        val result = RecordingResult()
        assertTrue(queue.offer("""{"mode":"vpn"}""", result))

        assertTrue(queue.cancel("VPN_PERMISSION_CANCELLED", "cancelled"))
        assertEquals("VPN_PERMISSION_CANCELLED", result.errorCode)
        assertEquals("cancelled", result.errorMessage)
        assertFalse(queue.cancel("VPN_PERMISSION_CANCELLED", "again"))
        assertEquals(1, result.completionCount)
    }

    private class RecordingResult : MethodChannel.Result {
        var completionCount = 0
        var errorCode: String? = null
        var errorMessage: String? = null

        override fun success(result: Any?) {
            completionCount += 1
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
