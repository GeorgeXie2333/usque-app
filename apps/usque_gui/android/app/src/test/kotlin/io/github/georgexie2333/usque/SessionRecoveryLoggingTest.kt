package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.util.concurrent.Executor

class SessionRecoveryLoggingTest {
    private class Harness(
        directory: File,
    ) {
        val snapshot = ServiceSnapshotState()
        var connectionGeneration = 4L
        var networkGeneration = 8L
        var restarts = 0
        var cleanupFailures = 0
        val stops = ArrayDeque<(Boolean) -> Unit>()
        val scheduled = ArrayDeque<() -> Unit>()
        val nativeStops = NativeStopTracker()
        val store = AndroidLogStore(directory, Executor(Runnable::run))
        val recovery =
            loggedSessionNetworkRecovery(
                captureLogContext = {
                    val decoded = NetworkQualityFields.decode(snapshot.networkQualityJson)
                    ServiceLogContext(
                        decoded?.get("connection_instance_id") as? String,
                        connectionGeneration,
                        networkGeneration,
                    )
                },
                suspendSession = {
                    connectionGeneration++
                    val killSwitch = snapshot.killSwitchEnabled
                    snapshot.reset("reconnecting")
                    snapshot.killSwitchEnabled = killSwitch
                },
                stop = { context, completed ->
                    val ticket = nativeStops.begin()
                    record(AndroidLogStore.Event.NATIVE_STOP_REQUESTED, context, ticket)
                    stops.add { confirmed ->
                        nativeStops.complete(ticket, confirmed)
                        record(
                            if (confirmed) {
                                AndroidLogStore.Event.NATIVE_STOP_COMPLETED
                            } else {
                                AndroidLogStore.Event.NATIVE_STOP_UNCONFIRMED
                            },
                            context,
                            ticket,
                        )
                        completed(confirmed)
                    }
                },
                schedule = { _, action -> scheduled.add(action) },
                restart = { restarts++ },
                cleanupFailed = { cleanupFailures++ },
            )

        init {
            installRuntime(OLD_ID, 4, 8)
        }

        fun installRuntime(
            id: String,
            connection: Long,
            network: Long,
        ) {
            connectionGeneration = connection
            networkGeneration = network
            snapshot.reset("connected")
            snapshot.killSwitchEnabled = true
            snapshot.networkQualityJson = JSONObject().put("connection_instance_id", id).toString()
        }

        private fun record(
            event: AndroidLogStore.Event,
            context: ServiceLogContext,
            ticket: Long,
        ) {
            store.record(
                event,
                connectionInstanceId = context.instanceId,
                connectionGeneration = context.connectionGeneration,
                networkGeneration = context.networkGeneration,
                stopTicket = ticket,
            )
        }

        fun exportedEntries(): List<JSONObject> =
            requireNotNull(AndroidLogStore.fromMap(store.capture().get().toMap()))
                .lines
                .lineSequence()
                .filter(String::isNotBlank)
                .map(::JSONObject)
                .toList()
    }

    @Test
    fun bothRecoveryTriggersKeepTheRetiringScopeThroughDelayedCleanup() {
        for (physicalChange in listOf(false, true)) {
            for (confirmed in listOf(false, true)) {
                val directory = Files.createTempDirectory("usque-recovery-log-test").toFile()
                try {
                    val h = Harness(directory)
                    val admitted =
                        if (physicalChange) {
                            h.recovery.networkChanged(8, true, restartEstablishedSession = true)
                        } else {
                            h.recovery.failed(true, 8, true)
                        }
                    assertTrue(admitted)
                    assertEquals(5L, h.connectionGeneration)
                    assertNull(h.snapshot.networkQualityJson)
                    assertTrue(h.snapshot.killSwitchEnabled)
                    assertTrue(h.nativeStops.pendingCleanup())

                    // Manual replacement can change every live source before old cleanup replies.
                    h.recovery.cancel()
                    h.installRuntime(NEW_ID, 6, 9)
                    h.stops.removeFirst()(confirmed)

                    val entries = h.exportedEntries()
                    assertEquals(2, entries.size)
                    for (entry in entries) assertScope(entry, OLD_ID, 4, 8, 1)
                    assertEquals(
                        if (confirmed) "NATIVE_STOP_COMPLETED" else "NATIVE_STOP_UNCONFIRMED",
                        entries.last().getString("event"),
                    )
                    assertEquals(0, h.restarts)
                    assertEquals(0, h.cleanupFailures)
                } finally {
                    directory.deleteRecursively()
                }
            }
        }
    }

    @Test
    fun anotherRecoveryCannotReplaceAnOlderStopScopeOrCleanupResult() {
        val directory = Files.createTempDirectory("usque-recovery-log-overlap-test").toFile()
        try {
            val h = Harness(directory)
            h.recovery.failed(true, 8, true)
            val oldCompletion = h.stops.removeFirst()
            h.recovery.cancel()
            h.installRuntime(NEW_ID, 6, 9)
            h.recovery.networkChanged(9, true, restartEstablishedSession = true)
            val newCompletion = h.stops.removeFirst()

            oldCompletion(false)
            assertEquals(0, h.cleanupFailures)
            assertTrue(h.recovery.active)
            assertTrue(h.nativeStops.pendingCleanup())
            newCompletion(true)
            assertFalse(h.nativeStops.pendingCleanup())
            assertEquals(1, h.scheduled.size)

            val entries = h.exportedEntries()
            assertEquals(4, entries.size)
            for (index in listOf(0, 2)) assertScope(entries[index], OLD_ID, 4, 8, 1)
            for (index in listOf(1, 3)) assertScope(entries[index], NEW_ID, 6, 9, 2)
        } finally {
            directory.deleteRecursively()
        }
    }

    private fun assertScope(
        entry: JSONObject,
        id: String,
        connection: Long,
        network: Long,
        ticket: Long,
    ) {
        assertEquals(id, entry.getString("connection_instance_id"))
        assertEquals(connection, entry.getLong("connection_generation"))
        assertEquals(network, entry.getLong("network_generation"))
        assertEquals(ticket, entry.getLong("stop_ticket"))
    }

    private companion object {
        const val OLD_ID = "123e4567-e89b-42d3-a456-426614174000"
        const val NEW_ID = "223e4567-e89b-42d3-a456-426614174000"
    }
}
