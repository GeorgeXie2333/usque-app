package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException
import java.nio.file.Files
import java.util.ArrayDeque
import java.util.concurrent.Executor

class AndroidLogStoreTest {
    @Test
    fun failedPartialAppendDoesNotCorruptTheFollowingRecord() {
        val directory = Files.createTempDirectory("usque-log-partial-test").toFile()
        try {
            var writes = 0
            val store =
                AndroidLogStore(directory, Executor(Runnable::run), appendRecord = { file, bytes ->
                    writes++
                    if (writes == 1) {
                        file.appendBytes(bytes.copyOfRange(0, bytes.size / 2))
                        throw IOException("disk full")
                    }
                    file.appendBytes(bytes)
                })
            store.record(AndroidLogStore.Event.CONNECTION_REQUESTED)
            store.record(AndroidLogStore.Event.CONNECTION_FAILED, errorType = "ANDROID_RUNTIME_FAILED")
            val snapshot = store.capture().get()
            assertEquals(1L, snapshot.health["write_failure_count"])
            assertEquals(1, snapshot.lines.lineSequence().count { it.isNotBlank() })
            assertTrue(snapshot.lines.contains("CONNECTION_FAILED"))
        } finally {
            directory.deleteRecursively()
        }
    }

    private class QueuedExecutor : Executor {
        val tasks = ArrayDeque<Runnable>()

        override fun execute(command: Runnable) {
            tasks.addLast(command)
        }

        fun runAll() {
            while (tasks.isNotEmpty()) tasks.removeFirst().run()
        }
    }

    @Test
    fun producersNeverTouchDiskAndQueueDropsAreObservable() {
        val parent = Files.createTempDirectory("usque-log-queue-test").toFile()
        try {
            val directory = parent.resolve("logs")
            val executor = QueuedExecutor()
            val store = AndroidLogStore(directory, executor, queueCapacity = 2)
            repeat(3) { store.record(AndroidLogStore.Event.CONNECTION_REQUESTED) }
            assertFalse(directory.exists())
            assertEquals(1, executor.tasks.size)
            val snapshot = store.capture()
            assertFalse(snapshot.isDone)
            executor.runAll()
            assertEquals(2L, snapshot.get().health["written_count"])
            assertEquals(1L, snapshot.get().health["dropped_event_count"])
            assertEquals(
                2,
                snapshot
                    .get()
                    .lines
                    .lineSequence()
                    .count { it.isNotBlank() },
            )
        } finally {
            parent.deleteRecursively()
        }
    }

    @Test
    fun diskFailureCannotEscapeProducerAndSnapshotReportsFailure() {
        val directory = Files.createTempDirectory("usque-log-failure-test").toFile()
        try {
            val executor = QueuedExecutor()
            val store =
                AndroidLogStore(directory, executor, appendRecord = {
                    _,
                    _,
                    ->
                    throw IOException("private-path")
                })
            store.record(AndroidLogStore.Event.CONNECTION_FAILED, errorType = "ANDROID_RUNTIME_FAILED")
            val snapshot = store.capture()
            executor.runAll()
            assertEquals(1L, snapshot.get().health["write_failure_count"])
            assertEquals(0L, snapshot.get().health["written_count"])
            assertFalse(snapshot.get().toString().contains("private-path"))
        } finally {
            directory.deleteRecursively()
        }
    }

    @Test
    fun clearBarrierRemovesQueuedHistoryAndPreventsLateProducerWrites() {
        val directory = Files.createTempDirectory("usque-log-clear-test").toFile()
        try {
            val executor = QueuedExecutor()
            val store = AndroidLogStore(directory, executor)
            store.record(AndroidLogStore.Event.CONNECTION_REQUESTED)
            val cleared = store.clearAndPause()
            store.record(AndroidLogStore.Event.NATIVE_STOP_COMPLETED)
            val empty = store.capture()
            executor.runAll()
            assertTrue(cleared.get())
            assertEquals("", empty.get().lines)
            store.resume()
            store.record(AndroidLogStore.Event.SERVICE_CREATED)
            val fresh = store.capture()
            executor.runAll()
            assertTrue(fresh.get().lines.contains("SERVICE_CREATED"))
            assertFalse(fresh.get().lines.contains("NATIVE_STOP_COMPLETED"))
        } finally {
            directory.deleteRecursively()
        }
    }

    @Test
    fun rotatedExportSelectsNewestWholeRecordsInChronologicalOrder() {
        val directory = Files.createTempDirectory("usque-log-tail-test").toFile()
        try {
            val store = AndroidLogStore(directory, Executor(Runnable::run), rotateBytes = 1)
            repeat(
                12,
            ) { index -> store.record(AndroidLogStore.Event.NETWORK_CHANGED, connectionGeneration = index.toLong()) }
            val all =
                store
                    .capture()
                    .get()
                    .lines
                    .lineSequence()
                    .filter { it.isNotBlank() }
                    .toList()
            assertEquals((0L..11L).toList(), all.map { JSONObject(it).getLong("connection_generation") })
            val limit = all.takeLast(2).sumOf { (it + "\n").toByteArray(Charsets.UTF_8).size }
            val tail = store.capture(limit).get()
            assertEquals(
                listOf(10L, 11L),
                tail.lines
                    .lineSequence()
                    .filter {
                        it.isNotBlank()
                    }.map { JSONObject(it).getLong("connection_generation") }
                    .toList(),
            )
            assertEquals(true, tail.health["truncated"])
        } finally {
            directory.deleteRecursively()
        }
    }

    @Test
    fun correlationAcceptsOnlyRuntimeUuidFixedCodesAndNonnegativeNumbers() {
        val directory = Files.createTempDirectory("usque-log-context-test").toFile()
        try {
            val store = AndroidLogStore(directory, Executor(Runnable::run))
            store.record(
                AndroidLogStore.Event.CONNECTION_FAILED,
                errorType = "ANDROID_RUNTIME_FAILED",
                connectionInstanceId = "123e4567-e89b-42d3-a456-426614174000",
                connectionGeneration = 4,
                networkGeneration = 8,
                stopTicket = 9,
            )
            store.record(
                AndroidLogStore.Event.CONNECTION_FAILED,
                errorType = "Bearer_private_token",
                connectionInstanceId = "private-account",
                networkGeneration = -1,
            )
            val snapshot = store.capture().get()
            val entries =
                snapshot.lines
                    .lineSequence()
                    .filter { it.isNotBlank() }
                    .map(::JSONObject)
                    .toList()
            assertEquals("ANDROID_RUNTIME_FAILED", entries.first().getString("error_code"))
            assertEquals(9L, entries.first().getLong("stop_ticket"))
            assertFalse(entries.last().has("error_code"))
            assertFalse(entries.last().has("connection_instance_id"))
            assertFalse(entries.last().has("network_generation"))
            assertFalse(snapshot.toString().contains("private"))
            val hostile =
                AndroidLogStore.fromMap(
                    mapOf(
                        "lines" to snapshot.lines + "{\"event\":\"CONNECTION_FAILED\",\"token\":\"private\"}\n",
                        "health" to snapshot.health,
                    ),
                )!!
            assertEquals(1L, hostile.health["omitted_line_count"])
            assertFalse(hostile.lines.contains("private"))
        } finally {
            directory.deleteRecursively()
        }
    }

    @Test
    fun nativeStopCompletionAndUnconfirmedEventsSurviveRedaction() {
        val directory = Files.createTempDirectory("usque-stop-log-test").toFile()
        try {
            val store = AndroidLogStore(directory)
            store.record(AndroidLogStore.Event.NATIVE_STOP_REQUESTED)
            store.record(AndroidLogStore.Event.NATIVE_STOP_UNCONFIRMED)
            store.record(AndroidLogStore.Event.NATIVE_STOP_COMPLETED)
            val diagnostic = store.diagnosticSnapshot()
            assertTrue(diagnostic.contains("NATIVE_STOP_REQUESTED"))
            assertTrue(diagnostic.contains("NATIVE_STOP_UNCONFIRMED"))
            assertTrue(diagnostic.contains("NATIVE_STOP_COMPLETED"))
            assertTrue(diagnostic.contains("\"level\":\"WARN\""))
        } finally {
            directory.deleteRecursively()
        }
    }

    @Test
    fun diagnosticLogContainsOnlyWhitelistedStateTokens() {
        val directory = Files.createTempDirectory("usque-log-test").toFile()
        try {
            val store = AndroidLogStore(directory)
            store.record(
                AndroidLogStore.Event.CONNECTION_REQUESTED,
                phase = "preparing",
                mode = "socks5",
                transport = "h3",
                errorType = "192.0.2.1",
            )

            val diagnostic = store.diagnosticSnapshot()
            assertTrue(diagnostic.contains("\"event\":\"CONNECTION_REQUESTED\""))
            assertTrue(diagnostic.contains("\"mode\":\"socks5\""))
            assertFalse(diagnostic.contains("192.0.2.1"))
        } finally {
            directory.deleteRecursively()
        }
    }

    @Test
    fun malformedOrUnknownLinesAreExcludedFromDiagnostics() {
        val directory = Files.createTempDirectory("usque-log-test").toFile()
        try {
            val store = AndroidLogStore(directory)
            store.record(AndroidLogStore.Event.CONNECTION_STOPPED, phase = "disconnecting")
            store.diagnosticSnapshot()
            directory.resolve("android-engine.jsonl").appendText(
                """
                {"event":"CONNECTION_STOPPED","secret":"must-not-leak"}
                not-json
                """.trimIndent(),
            )

            val diagnostic = store.diagnosticSnapshot()
            assertTrue(diagnostic.contains("\"event\":\"CONNECTION_STOPPED\""))
            assertFalse(diagnostic.contains("must-not-leak"))
            assertFalse(diagnostic.contains("not-json"))
        } finally {
            directory.deleteRecursively()
        }
    }
}
