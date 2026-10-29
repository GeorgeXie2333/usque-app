package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ServiceSnapshotPublisherTest {
    @Test
    fun nativeSnapshotObservesOnceBeforePublishingWithOrWithoutClients() {
        for (clients in listOf(0, 1)) {
            val service = FakeService(clients)

            repeat(3) {
                val publishObservedSnapshot = service.publisher.observeNativeSnapshot()
                assertEquals(it + 1, service.settingsCommands.size)
                publishObservedSnapshot()
                assertEquals(it + 1, service.settingsCommands.size)
            }

            assertEquals(listOf(7L, 7L, 7L), service.settingsCommands)
            assertEquals(3 * clients, service.deliveries.size)
            assertEquals(List(3) { listOf("observe", "publish") }.flatten(), service.order)
        }
    }

    @Test
    fun nativeFailureIsObservedBeforeRecoveryRetiresItsGenerationWithoutPublishing() {
        val service = FakeService(0)
        val token = service.settingsApplication.begin(service.generation)
        assertTrue(service.settingsApplication.committed(token, service.generation, "save"))
        service.settingsApplication.runtimeReplied(token, service.generation)

        // The service returns into recovery instead of invoking the publication callback.
        service.publisher.observeNativeSnapshot()
        service.generation++
        service.settingsApplication.cancelIfStale(service.generation)

        assertEquals(listOf(7L), service.settingsCommands)
        assertEquals(listOf("observe"), service.order)
        assertTrue(service.deliveries.isEmpty())
    }

    @Test
    fun pendingSaveGuardsRemainEffectiveAndCompletionDoesNotCauseASecondObservation() {
        val service = FakeService(1)
        val token = service.settingsApplication.begin(service.generation)

        service.publisher.observeNativeSnapshot().invoke()
        assertTrue(service.settingsCommands.isEmpty())
        assertTrue(service.settingsApplication.committed(token, service.generation, "save"))
        service.publisher.observeNativeSnapshot().invoke()
        assertTrue(service.settingsCommands.isEmpty())

        service.settingsApplication.runtimeReplied(token, service.generation)
        val publishObservedSnapshot = service.publisher.observeNativeSnapshot()
        assertEquals(NetworkSettingsApplicationTracker.Phase.IDLE, service.settingsApplication.phase)
        publishObservedSnapshot()

        assertEquals(listOf(7L), service.settingsCommands)
        assertEquals(listOf(7L, 7L, 7L), service.deliveries)
    }

    @Test
    fun platformBroadcastStillObservesSettingsWithoutClients() {
        val service = FakeService(0)

        service.publisher.broadcast()

        assertEquals(listOf(7L), service.settingsCommands)
        assertEquals(listOf("observe", "publish"), service.order)
        assertTrue(service.deliveries.isEmpty())
    }

    private class FakeService(
        clients: Int,
    ) {
        var generation = 7L
        val settingsCommands = mutableListOf<Long>()
        val deliveries = mutableListOf<Long>()
        val order = mutableListOf<String>()
        val settingsApplication = NetworkSettingsApplicationTracker()
        val publisher =
            ServiceSnapshotPublisher(
                observeSettings = {
                    order += "observe"
                    if (settingsApplication.allowsObservation(generation)) {
                        settingsCommands += generation
                        settingsApplication.current(generation)?.let {
                            settingsApplication.finish(it.token, generation)
                        }
                    }
                },
                publishSnapshot = {
                    order += "publish"
                    repeat(clients) { deliveries += generation }
                },
            )
    }
}
