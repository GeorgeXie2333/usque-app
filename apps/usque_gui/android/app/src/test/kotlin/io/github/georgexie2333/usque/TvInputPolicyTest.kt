package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TvInputPolicyTest {
    private class Harness : TvInputPolicy.Host {
        val events = mutableListOf<Pair<String, String>>()
        var registered: String? = null
        var hides = 0

        override fun registerBack(sessionId: String?) {
            registered = sessionId
        }

        override fun hideKeyboard() {
            hides++
        }

        override fun editingEnded(
            sessionId: String,
            reason: String,
        ) {
            events += sessionId to reason
        }
    }

    @Test
    fun onlyTelevisionFactsEnableThePolicy() {
        assertFalse(TvInputPolicy.isTelevision(false, false, false))
        assertTrue(TvInputPolicy.isTelevision(true, false, false))
        assertTrue(TvInputPolicy.isTelevision(false, true, false))
        assertTrue(TvInputPolicy.isTelevision(false, false, true))
        val h = Harness()
        val policy = TvInputPolicy(false, h)
        assertFalse(policy.begin("phone"))
        assertFalse(policy.backKey(true, 1))
        assertEquals(null, h.registered)
        assertEquals(0, h.hides)
    }

    @Test
    fun legacyBackConsumesRepeatsAndReleaseEvenAfterImeHides() {
        val h = Harness()
        val policy = TvInputPolicy(true, h)
        policy.begin("a")
        assertTrue(policy.backKey(true, 1))
        policy.imeVisibility(true)
        policy.imeVisibility(false)
        assertTrue(policy.backKey(true, 1))
        assertTrue(policy.backKey(false, 1))
        assertEquals(listOf("a" to "imeHidden"), h.events)
        assertFalse(policy.backKey(true, 2))
        assertEquals(1, h.hides)
    }

    @Test
    fun zeroInsetsAndCancelledBackKeepEditingUntilARealBack() {
        val h = Harness()
        val policy = TvInputPolicy(true, h)
        policy.begin("a")
        repeat(3) { policy.imeVisibility(false) }
        assertTrue(policy.backKey(true, 1))
        assertTrue(policy.backKey(false, 1, cancelled = true))
        assertEquals("a", h.registered)
        assertTrue(policy.backKey(true, 2))
        assertTrue(policy.backKey(false, 2))
        assertEquals(listOf("a" to "back"), h.events)
        assertEquals(null, h.registered)
    }

    @Test
    fun modernAndStaleCallbacksCannotCloseTheNextSession() {
        val h = Harness()
        val policy = TvInputPolicy(true, h)
        policy.begin("a")
        policy.systemBack("a")
        policy.begin("b")
        policy.systemBack("a")
        policy.end("a")
        policy.imeVisibility(false)
        assertEquals("b", h.registered)
        policy.systemBack("b")
        policy.systemBack("b")
        assertEquals(listOf("a" to "back", "b" to "back"), h.events)
    }

    @Test
    fun oldBackReleaseCannotCloseAReplacementSession() {
        val h = Harness()
        val policy = TvInputPolicy(true, h)
        policy.begin("a")
        policy.backKey(true, 1)
        policy.imeVisibility(true)
        policy.imeVisibility(false)
        policy.begin("b")
        assertTrue(policy.backKey(false, 1))
        assertEquals("b", h.registered)
        assertEquals(listOf("a" to "imeHidden"), h.events)
    }

    @Test
    fun replacementAndLifecycleUnregisterAndReleaseTheKeyboard() {
        val h = Harness()
        val policy = TvInputPolicy(true, h)
        policy.begin("a")
        policy.begin("b")
        policy.backKey(true, 1)
        policy.stopped()
        policy.stopped()
        assertFalse(policy.backKey(false, 1))
        assertEquals(listOf("a" to "replaced", "b" to "lifecycle"), h.events)
        assertEquals(null, h.registered)
        assertEquals(2, h.hides)
    }
}
