package io.github.georgexie2333.usque

import io.flutter.plugin.common.BinaryMessenger
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.AbstractExecutorService
import java.util.concurrent.ExecutorService
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit

class TrackedPreferencesMessengerTest {
    private class ControlledExecutor : AbstractExecutorService() {
        val pending = ArrayDeque<Runnable>()
        var stopped = false
        private var running = false

        override fun execute(command: Runnable) {
            if (stopped) throw RejectedExecutionException()
            pending.addLast(command)
        }

        fun runNext() {
            val next = pending.removeFirst()
            running = true
            try {
                next.run()
            } finally {
                running = false
            }
        }

        override fun shutdown() {
            stopped = true
        }

        override fun shutdownNow(): MutableList<Runnable> =
            pending.toMutableList().also {
                stopped = true
                pending.clear()
            }

        override fun isShutdown(): Boolean = stopped

        override fun isTerminated(): Boolean = stopped && pending.isEmpty() && !running

        override fun awaitTermination(
            timeout: Long,
            unit: TimeUnit,
        ): Boolean = isTerminated
    }

    private class Messenger : BinaryMessenger {
        val handlers = mutableMapOf<String, BinaryMessenger.BinaryMessageHandler>()

        override fun setMessageHandler(
            channel: String,
            handler: BinaryMessenger.BinaryMessageHandler?,
        ) {
            if (handler == null) handlers.remove(channel) else handlers[channel] = handler
        }

        override fun send(
            channel: String,
            message: ByteBuffer?,
        ) = Unit

        override fun send(
            channel: String,
            message: ByteBuffer?,
            callback: BinaryMessenger.BinaryReply?,
        ) = Unit
    }

    private class Harness {
        val delegate = Messenger()
        val executors = mutableListOf<ControlledExecutor>()
        val retired = mutableListOf<ExecutorService>()
        var leases = 0
        var released = 0
        val replies = mutableListOf<ByteBuffer?>()
        val messenger =
            TrackedPreferencesMessenger(
                delegate = delegate,
                holdWork = {
                    leases++
                    AutoCloseable {
                        leases--
                        released++
                    }
                },
                awaitCleanup = { retired.addAll(it) },
                createExecutor = { ControlledExecutor().also(executors::add) },
            )

        fun register(handler: BinaryMessenger.BinaryMessageHandler) {
            messenger.setMessageHandler("preferences", handler, messenger.makeBackgroundTaskQueue())
        }

        fun receive(message: ByteBuffer? = null) {
            checkNotNull(delegate.handlers["preferences"]).onMessage(message) { replies.add(it) }
        }
    }

    @Test
    fun queuedWritesAcquireLeasesBeforeTheirHandlersStart() {
        val h = Harness()
        var writes = 0
        h.register { _, reply ->
            writes++
            reply.reply(null)
        }
        h.receive()
        h.receive()
        assertEquals(2, h.leases)
        assertEquals(0, writes)
        h.executors.single().runNext()
        assertEquals(1, h.leases)
        h.executors.single().runNext()
        assertEquals(0, h.leases)
        assertEquals(2, writes)
        assertEquals(2, h.released)
    }

    @Test
    fun copiesOnlyRemainingInputBeforeNativeBufferCanBeReleased() {
        val h = Harness()
        val original = ByteBuffer.allocateDirect(4).order(ByteOrder.LITTLE_ENDIAN)
        original.put(byteArrayOf(9, 1, 2, 8)).position(1).limit(3)
        var received: ByteBuffer? = null
        h.register { message, _ -> received = message }
        h.receive(original)
        assertEquals(1, original.position())
        assertEquals(3, original.limit())
        original.put(1, 99).put(2, 98).limit(0)
        h.executors.single().runNext()
        val copy = checkNotNull(received)
        assertNotSame(original, copy)
        assertTrue(copy.isDirect)
        assertEquals(ByteOrder.LITTLE_ENDIAN, copy.order())
        assertEquals(2, copy.remaining())
        assertEquals(1, copy.get().toInt())
        assertEquals(2, copy.get().toInt())
    }

    @Test
    fun detachUnregistersEveryChannelAndDrainsAcceptedWrites() {
        val h = Harness()
        var writes = 0
        h.register { _, _ -> writes++ }
        val oldHandler = checkNotNull(h.delegate.handlers["preferences"])
        h.messenger.setMessageHandler("legacy", { _, _ -> writes++ }, h.messenger.makeBackgroundTaskQueue())
        h.receive()
        h.messenger.stopAccepting()
        // Upstream detach also creates queues while unregistering its APIs.
        h.messenger.makeBackgroundTaskQueue()
        h.messenger.finishCleanup()
        assertTrue(h.delegate.handlers.isEmpty())
        assertEquals(3, h.retired.size)
        assertFalse(h.retired.all { it.isTerminated })
        oldHandler.onMessage(null) { h.replies.add(it) }
        assertEquals(1, h.leases)
        assertEquals(0, writes)
        h.executors.first().runNext()
        assertEquals(1, writes)
        assertEquals(0, h.leases)
        assertTrue(h.retired.all { it.isTerminated })
        assertEquals(2, h.replies.size)
    }

    @Test
    fun rejectedExecutionRepliesOnceAndReleasesLease() {
        val h = Harness()
        h.register { _, _ -> throw AssertionError("Rejected work must not execute") }
        h.executors.single().shutdown()
        h.receive()
        assertEquals(0, h.leases)
        assertEquals(1, h.released)
        assertEquals(1, h.replies.size)
        assertNull(h.replies.single())
    }

    @Test
    fun handlerFailureRepliesOnceAndReleasesLease() {
        val h = Harness()
        h.register { _, _ -> throw IllegalStateException("Invalid input") }
        h.receive()
        h.executors.single().runNext()
        assertEquals(0, h.leases)
        assertEquals(1, h.released)
        assertEquals(1, h.replies.size)
        assertNull(h.replies.single())
    }

    @Test
    fun failureAfterSuccessfulReplyCannotReplyAgain() {
        val h = Harness()
        val response = ByteBuffer.allocateDirect(1).put(7)
        h.register { _, reply ->
            reply.reply(response)
            throw IllegalStateException("After reply")
        }
        h.receive()
        h.executors.single().runNext()
        assertEquals(1, h.replies.size)
        assertSame(response, h.replies.single())
        assertEquals(1, h.released)
    }

    @Test
    fun platformThreadHandlersReleaseExactlyOnceOnFailure() {
        val h = Harness()
        h.messenger.setMessageHandler("preferences") { _, _ -> throw IllegalStateException("Invalid input") }
        h.receive()
        assertEquals(0, h.leases)
        assertEquals(1, h.released)
        assertEquals(1, h.replies.size)
    }

    @Test
    fun nullMessagesAndRepeatedCleanupDoNotLeakWork() {
        val h = Harness()
        h.register { message, reply ->
            assertNull(message)
            reply.reply(null)
        }
        h.receive()
        h.messenger.finishCleanup()
        h.messenger.finishCleanup()
        h.executors.single().runNext()
        assertEquals(1, h.retired.size)
        assertEquals(0, h.leases)
        assertEquals(1, h.released)
        assertEquals(1, h.replies.size)
    }
}
