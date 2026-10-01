package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException

class UnderlyingSocketBindingTest {
    private class NativeErrno(
        val errno: Int,
        cause: Throwable? = null,
    ) : Exception(cause)

    private class Fixture {
        var generation = 7L
        var network: Long? = 91L
        var destroyed = false
        var protectResult = true
        var protectAction: () -> Unit = {}
        var duplicateAction: () -> Unit = {}
        var bindAction: () -> Unit = {}
        var closeAction: () -> Unit = {}
        val calls = mutableListOf<String>()
        val binding =
            UnderlyingSocketBinding<Long, Int>(
                destroyed = { destroyed },
                currentGeneration = { generation },
                networkForGeneration = { network },
                networkHandle = { it },
                protect = {
                    calls += "protect"
                    protectAction()
                    protectResult
                },
                duplicate = {
                    calls += "duplicate"
                    duplicateAction()
                    it + 100
                },
                bindToNetwork = { selected, copy ->
                    assertEquals(91L, selected)
                    assertEquals(142, copy)
                    calls += "bind"
                    bindAction()
                },
                close = {
                    assertEquals(142, it)
                    calls += "close"
                    closeAction()
                },
                isNetworkGoneException = { hasNetworkBindingErrno(it, 64) { cause -> (cause as? NativeErrno)?.errno } },
            )

        fun bind(protected: Boolean = true): Int = binding.bind(42, 7L, protected)
    }

    @Test
    fun successfulBindProtectsFirstAndClosesOnlyTheDuplicate() {
        val fixture = Fixture()
        assertEquals(UnderlyingSocketBindingResult.BOUND, fixture.bind())
        assertEquals(listOf("protect", "duplicate", "bind", "close"), fixture.calls)

        val proxy = Fixture()
        assertEquals(UnderlyingSocketBindingResult.BOUND, proxy.bind(protected = false))
        assertEquals(listOf("duplicate", "bind", "close"), proxy.calls)
    }

    @Test
    fun invalidStoppedStaleAndAbsentNetworksCannotReachProtection() {
        val fixture = Fixture()
        assertEquals(UnderlyingSocketBindingResult.REJECTED, fixture.binding.bind(-1, 7L, true))
        assertEquals(UnderlyingSocketBindingResult.REJECTED, fixture.binding.bind(42, -1L, true))
        fixture.destroyed = true
        assertEquals(UnderlyingSocketBindingResult.REJECTED, fixture.bind())
        fixture.destroyed = false
        fixture.generation++
        assertEquals(UnderlyingSocketBindingResult.STALE, fixture.bind())
        fixture.generation = 7L
        fixture.network = null
        assertEquals(UnderlyingSocketBindingResult.STALE, fixture.bind())
        assertTrue(fixture.calls.isEmpty())
    }

    @Test
    fun protectionFailuresRemainRejectedEvenWhenTheGenerationChanges() {
        val fixture = Fixture()
        fixture.protectAction = { fixture.generation++ }
        fixture.protectResult = false
        assertEquals(UnderlyingSocketBindingResult.REJECTED, fixture.bind())
        assertEquals(listOf("protect"), fixture.calls)

        val exception = Fixture()
        exception.protectAction = { throw IOException(NativeErrno(64)) }
        assertEquals(UnderlyingSocketBindingResult.REJECTED, exception.bind())
        assertEquals(listOf("protect"), exception.calls)
    }

    @Test
    fun aGenerationChangeDuringProtectionOrDuplicationPreventsBinding() {
        val protecting = Fixture()
        protecting.protectAction = { protecting.generation++ }
        assertEquals(UnderlyingSocketBindingResult.STALE, protecting.bind())
        assertEquals(listOf("protect"), protecting.calls)

        val duplicating = Fixture()
        duplicating.duplicateAction = { duplicating.generation++ }
        assertEquals(UnderlyingSocketBindingResult.STALE, duplicating.bind())
        assertEquals(listOf("protect", "duplicate", "close"), duplicating.calls)
    }

    @Test
    fun generationAndNetworkIdentityAreCheckedAfterSuccessfulBinding() {
        val generation = Fixture()
        generation.bindAction = { generation.generation++ }
        assertEquals(UnderlyingSocketBindingResult.STALE, generation.bind())
        assertEquals("close", generation.calls.last())

        val identity = Fixture()
        identity.bindAction = { identity.network = 92L }
        assertEquals(UnderlyingSocketBindingResult.STALE, identity.bind())
        assertEquals("close", identity.calls.last())
    }

    @Test
    fun generationChangeAtTheBindingExceptionBoundaryIsRetryable() {
        val fixture = Fixture()
        fixture.bindAction = {
            fixture.generation++
            throw IOException()
        }
        assertEquals(UnderlyingSocketBindingResult.STALE, fixture.bind())
        assertEquals(listOf("protect", "duplicate", "bind", "close"), fixture.calls)

        val lost = Fixture()
        lost.bindAction = {
            lost.network = null
            throw IOException()
        }
        assertEquals(UnderlyingSocketBindingResult.STALE, lost.bind())
        assertEquals("close", lost.calls.last())
    }

    @Test
    fun exactNetworkGoneErrnoIsRetryableBeforeTheLossCallbackArrives() {
        val fixture = Fixture()
        fixture.bindAction = { throw IOException(IOException(NativeErrno(64))) }
        assertEquals(UnderlyingSocketBindingResult.STALE, fixture.bind())
        assertEquals(7L, fixture.generation)
        assertEquals(listOf("protect", "duplicate", "bind", "close"), fixture.calls)
    }

    @Test
    fun permissionInvalidDescriptorAndUnclassifiedIoFailuresRemainRejected() {
        val errors =
            listOf(
                IOException(NativeErrno(1)),
                IOException(NativeErrno(13)),
                IOException(NativeErrno(9)),
                IOException("ENONET"),
            )
        for (error in errors) {
            val fixture = Fixture()
            fixture.bindAction = { throw error }
            assertEquals(UnderlyingSocketBindingResult.REJECTED, fixture.bind())
            assertEquals("close", fixture.calls.last())
        }
    }

    @Test
    fun stoppingDuringBindingOverridesNetworkGoneRecovery() {
        val fixture = Fixture()
        fixture.bindAction = {
            fixture.destroyed = true
            throw IOException(NativeErrno(64))
        }
        assertEquals(UnderlyingSocketBindingResult.REJECTED, fixture.bind())
        assertEquals("close", fixture.calls.last())
    }

    @Test
    fun duplicationAndCleanupFailuresRemainRejected() {
        val duplicating = Fixture()
        duplicating.duplicateAction = { throw IOException(NativeErrno(64)) }
        assertEquals(UnderlyingSocketBindingResult.REJECTED, duplicating.bind())
        assertEquals(listOf("protect", "duplicate"), duplicating.calls)

        val closing = Fixture()
        closing.closeAction = { throw IOException() }
        assertEquals(UnderlyingSocketBindingResult.REJECTED, closing.bind())
        assertEquals(listOf("protect", "duplicate", "bind", "close"), closing.calls)
    }

    @Test
    fun cleanupCannotHideAGenerationChangeOrServiceStop() {
        val generation = Fixture()
        generation.closeAction = { generation.generation++ }
        assertEquals(UnderlyingSocketBindingResult.STALE, generation.bind())
        assertEquals("close", generation.calls.last())

        val stopped = Fixture()
        stopped.closeAction = { stopped.destroyed = true }
        assertEquals(UnderlyingSocketBindingResult.REJECTED, stopped.bind())
        assertEquals("close", stopped.calls.last())
    }

    @Test
    fun errnoClassificationUsesTypedCausesAndTerminatesForCauseCycles() {
        val errnoOf: (Throwable) -> Int? = { (it as? NativeErrno)?.errno }
        assertTrue(hasNetworkBindingErrno(IOException(NativeErrno(64)), 64, errnoOf))
        assertFalse(hasNetworkBindingErrno(IOException(NativeErrno(13)), 64, errnoOf))
        assertFalse(hasNetworkBindingErrno(IOException(NativeErrno(13, NativeErrno(64))), 64, errnoOf))
        assertFalse(hasNetworkBindingErrno(IOException("ENONET"), 64, errnoOf))
        val first = IOException()
        val second = IOException(first)
        first.initCause(second)
        assertFalse(hasNetworkBindingErrno(first, 64, errnoOf))
    }
}
