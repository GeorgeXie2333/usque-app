package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import java.nio.file.Files
import java.util.concurrent.Executor

class ServiceLogContextTest {
    @Test
    fun replacementRequestAndDelayedStopKeepSeparateRuntimeScopes() {
        val oldId = "123e4567-e89b-42d3-a456-426614174000"
        val previous = ServiceLogContext(oldId, 4, 8)
        val requested = previous.replacementRequest(5, 9)
        val directory = Files.createTempDirectory("usque-replacement-log-test").toFile()
        try {
            val store = AndroidLogStore(directory, Executor(Runnable::run))

            fun record(
                event: AndroidLogStore.Event,
                context: ServiceLogContext,
                stopTicket: Long? = null,
            ) {
                store.record(
                    event,
                    connectionInstanceId = context.instanceId,
                    connectionGeneration = context.connectionGeneration,
                    networkGeneration = context.networkGeneration,
                    stopTicket = stopTicket,
                )
            }
            record(AndroidLogStore.Event.CONNECTION_REQUESTED, requested)
            record(AndroidLogStore.Event.NATIVE_STOP_REQUESTED, previous, 11)
            // A replacement is already running by the time the older stop finishes.
            record(
                AndroidLogStore.Event.CONNECTION_PHASE_CHANGED,
                ServiceLogContext("223e4567-e89b-42d3-a456-426614174000", 5, 9),
            )
            record(AndroidLogStore.Event.NATIVE_STOP_COMPLETED, previous, 11)
            val entries =
                store
                    .capture()
                    .get()
                    .lines
                    .lineSequence()
                    .filter { it.isNotBlank() }
                    .map(::JSONObject)
                    .toList()
            assertFalse(entries[0].has("connection_instance_id"))
            assertEquals(5L, entries[0].getLong("connection_generation"))
            assertEquals(9L, entries[0].getLong("network_generation"))
            for (entry in listOf(entries[1], entries[3])) {
                assertEquals(oldId, entry.getString("connection_instance_id"))
                assertEquals(4L, entry.getLong("connection_generation"))
                assertEquals(8L, entry.getLong("network_generation"))
                assertEquals(11L, entry.getLong("stop_ticket"))
            }
        } finally {
            directory.deleteRecursively()
        }
    }
}
