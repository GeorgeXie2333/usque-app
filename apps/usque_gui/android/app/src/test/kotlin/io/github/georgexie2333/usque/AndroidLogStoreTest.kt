package io.github.georgexie2333.usque

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files

class AndroidLogStoreTest {
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
