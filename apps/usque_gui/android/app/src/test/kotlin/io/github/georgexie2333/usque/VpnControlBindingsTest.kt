package io.github.georgexie2333.usque

import android.content.ServiceConnection
import android.os.IBinder
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Proxy

class VpnControlBindingsTest {
    @Test
    fun delayedFirstCallbackFromAnotherBindingCannotReviveARetiredActor() {
        val calls = mutableListOf<ServiceConnection>()
        val endpoints = mutableListOf<IBinder>()
        val bindings =
            VpnControlBindings({ connection, _ ->
                calls += connection
                true
            }, {}, endpoints::add, { _, _ -> })
        val old = binder()
        val replacement = binder()
        bindings.retainService()
        calls[0].onServiceConnected(null, old)
        bindings.observe()
        // Different ServiceDispatchers have separate Binder callback nodes. The
        // observer's first callback can lag the creator's disconnect/reconnect.
        calls[0].onServiceDisconnected(null)
        calls[0].onServiceConnected(null, replacement)
        calls[1].onServiceConnected(null, old)
        assertSame(replacement, endpoints.last())
        assertEquals(2, endpoints.size)

        // Full unbinding ends that callback generation. A deliberate new bind
        // to the same surviving service instance remains legal.
        bindings.close()
        bindings.observe()
        calls[2].onServiceConnected(null, old)
        assertSame(old, endpoints.last())
    }

    @Test
    fun oneBindingNullOrDeadCannotDiscardTheOtherValidEndpoint() {
        for (creatingLost in listOf(false, true)) {
            for (died in listOf(false, true)) {
                val calls = mutableListOf<Pair<ServiceConnection, Boolean>>()
                val notices = mutableListOf<Pair<Boolean, Boolean>>()
                val bindings =
                    VpnControlBindings(
                        { connection, creating ->
                            calls += connection to creating
                            true
                        },
                        {},
                        {},
                        { unexpected, retry -> notices += unexpected to retry },
                    )
                bindings.observe()
                bindings.retainService()
                val endpoint = binder()
                calls.forEach { it.first.onServiceConnected(null, endpoint) }
                val failed = calls.single { it.second == creatingLost }.first
                if (died) failed.onBindingDied(null) else failed.onNullBinding(null)
                assertTrue(bindings.hasEndpoint)
                assertTrue(bindings.isBound)
                assertEquals(!creatingLost, bindings.createsService)
                assertTrue(notices.none { it.first })
                assertEquals(if (died) listOf(false to true) else emptyList(), notices)
            }
        }
    }

    @Test
    fun replacementServiceCannotFallBackToTheOldActor() {
        val calls = mutableListOf<ServiceConnection>()
        val bindings =
            VpnControlBindings({ connection, _ ->
                calls += connection
                true
            }, {}, {}, { _, _ -> })
        bindings.observe()
        calls[0].onServiceConnected(null, binder())
        bindings.retainService()
        calls[1].onServiceConnected(null, binder())
        bindings.releaseService()
        assertTrue(bindings.isBound)
        assertFalse(bindings.hasEndpoint)
    }

    private fun binder(): IBinder =
        Proxy.newProxyInstance(
            IBinder::class.java.classLoader,
            arrayOf(IBinder::class.java),
        ) { _, _, _ -> null } as IBinder

    @Test
    fun observationNeverCreatesAndRejectedBindingStillReleasesTracking() {
        val flags = mutableListOf<Boolean>()
        val releases = mutableListOf<ServiceConnection>()
        val bindings =
            VpnControlBindings({ _, creating ->
                flags += creating
                false
            }, releases::add, {}, { _, _ -> })
        bindings.observe()
        assertEquals(listOf(false), flags)
        assertEquals(1, releases.size)
        assertFalse(bindings.isBound)
    }

    @Test
    fun operationBindingCanEndWithoutDroppingAnExistingObserverEndpoint() {
        val calls = mutableListOf<Pair<ServiceConnection, Boolean>>()
        var available = 0
        var lost = 0
        val releases = mutableListOf<ServiceConnection>()
        val bindings =
            VpnControlBindings({ connection, creating ->
                calls += connection to creating
                true
            }, releases::add, { available++ }, { _, _ -> lost++ })
        bindings.observe()
        val endpoint = binder()
        calls[0].first.onServiceConnected(null, endpoint)
        bindings.retainService()
        calls[1].first.onServiceConnected(null, endpoint)
        bindings.releaseService()
        assertEquals(listOf(false, true), calls.map { it.second })
        assertFalse(bindings.createsService)
        assertTrue(bindings.isBound)
        assertEquals(2, available)
        assertEquals(0, lost)
        bindings.releaseObservation()
        assertEquals(1, lost)
        assertEquals(2, releases.size)
    }

    @Test
    fun lateCallbackFromReleasedCreatingBindingCannotResurrectControl() {
        val calls = mutableListOf<ServiceConnection>()
        var available = 0
        val bindings =
            VpnControlBindings({ connection, _ ->
                calls += connection
                true
            }, {}, { available++ }, { _, _ -> })
        bindings.retainService()
        bindings.releaseService()
        calls.single().onServiceConnected(null, binder())
        assertFalse(bindings.isBound)
        assertEquals(0, available)
    }
}
