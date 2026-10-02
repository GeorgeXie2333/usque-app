package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ExecutionException
import java.util.concurrent.RejectedExecutionException

class GenerationOwnerDispatcherTest {
    private class Harness {
        var generation = 1L
        var ownedFd: Int? = null
        var killSwitchEnabled = false
        val queued = ArrayDeque<() -> Unit>()
        val dispatcher =
            GenerationOwnerDispatcher(
                isCurrent = { it == generation },
                dispatch = { queued.add(it) },
            )

        fun establish(
            owner: Long = generation,
            descriptor: Int = 42,
        ): CompletableFuture<Int> =
            dispatcher.submit(owner) {
                ownedFd = descriptor
                killSwitchEnabled = true
                descriptor
            }

        fun flush() {
            while (queued.isNotEmpty()) queued.removeFirst()()
        }
    }

    private fun failure(future: CompletableFuture<*>): Throwable {
        try {
            future.get()
            fail("Expected exceptional completion")
        } catch (error: ExecutionException) {
            return requireNotNull(error.cause)
        }
        error("Expected exceptional completion")
    }

    @Test
    fun disconnectBeforeDispatchPreventsEstablishment() {
        val h = Harness()
        val handoff = h.establish()
        h.generation++
        h.flush()
        assertEquals(null, h.ownedFd)
        assertFalse(h.killSwitchEnabled)
        val superseded = failure(handoff)
        assertTrue(superseded is GenerationSupersededException)
        assertEquals(1L, (superseded as GenerationSupersededException).generation)
    }

    @Test
    fun timeoutCancellationBeforeDispatchPreventsPlatformMutation() {
        val h = Harness()
        val handoff = h.establish()
        assertTrue(handoff.cancel(false))
        h.flush()
        assertTrue(handoff.isCancelled)
        assertEquals(null, h.ownedFd)
        assertFalse(h.killSwitchEnabled)
    }

    @Test
    fun recoverySupersedesQueuedHandoffBeforePublishingItsOwnDescriptor() {
        val h = Harness()
        val superseded = h.establish(descriptor = 41)
        h.generation++
        val replacement = h.establish(descriptor = 43)
        h.flush()
        assertTrue(failure(superseded) is GenerationSupersededException)
        assertEquals(43, replacement.get())
        assertEquals(43, h.ownedFd)
        assertTrue(h.killSwitchEnabled)
    }

    @Test
    fun ownerUpdatesDescriptorAndKillSwitchBeforeCompletingFuture() {
        val h = Harness()
        val handoff = h.establish()
        assertFalse(handoff.isDone)
        assertEquals(null, h.ownedFd)
        assertFalse(h.killSwitchEnabled)
        h.flush()
        assertEquals(42, handoff.get())
        assertEquals(42, h.ownedFd)
        assertTrue(h.killSwitchEnabled)
        assertFalse(handoff.cancel(false))
        assertEquals(42, h.ownedFd)
        assertTrue(h.killSwitchEnabled)
    }

    @Test
    fun actionExceptionCompletesFutureWithOriginalCause() {
        val h = Harness()
        val cause = IllegalStateException("Establishment failed")
        val handoff = h.dispatcher.submit<Int>(h.generation) { throw cause }
        h.flush()
        assertSame(cause, failure(handoff))
        assertEquals(null, h.ownedFd)
    }

    @Test
    fun cancellationAfterActionStartsDoesNotUndoPublishedProtection() {
        val h = Harness()
        lateinit var handoff: CompletableFuture<Int>
        handoff =
            h.dispatcher.submit(h.generation) {
                h.ownedFd = 42
                h.killSwitchEnabled = true
                // Model a waiting engine thread timing out after owner execution began.
                assertTrue(handoff.cancel(false))
                42
            }
        h.flush()
        assertTrue(handoff.isCancelled)
        assertEquals(42, h.ownedFd)
        assertTrue(h.killSwitchEnabled)
    }

    @Test
    fun rejectedDispatchCompletesFutureWithOriginalCause() {
        val cause = RejectedExecutionException("Owner dispatcher is closed")
        val dispatcher = GenerationOwnerDispatcher(isCurrent = { true }, dispatch = { throw cause })
        var invoked = false
        val handoff = dispatcher.submit(1L) { invoked = true }
        assertSame(cause, failure(handoff))
        assertFalse(invoked)
    }
}
