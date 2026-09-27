package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Test

class NativeSessionSnapshotTest {
    @Test
    fun replacementBetweenActiveCheckAndJniCannotRelabelTheOldError() {
        var current = 1L
        val snapshot =
            readNativeSessionSnapshot(
                generation = { current },
                active = {
                    // An observer sees active=true just as main suspends the
                    // old runtime and advances the replacement generation.
                    current = 2L
                    true
                },
                read = { "old H3_CONNECTION_CLOSED" },
            )!!
        assertEquals(1L, snapshot.generation)
        assertNotEquals(current, snapshot.generation)
    }

    @Test
    fun replacementDuringJniCannotPublishItsPredecessorsSnapshot() {
        var current = 3L
        val snapshot =
            readNativeSessionSnapshot(
                generation = { current },
                active = { true },
                read = {
                    current = 4L
                    "old disconnected"
                },
            )!!
        assertNotEquals(current, snapshot.generation)
        val replacement = readNativeSessionSnapshot({ current }, { true }, { "connected" })!!
        assertEquals(current, replacement.generation)
        assertEquals("connected", replacement.value)
    }

    @Test
    fun cleanupAndStartupNeverReadThePreviousEngineSlot() {
        val snapshot =
            readNativeSessionSnapshot<String>(
                generation = { 5L },
                active = { false },
                read = { error("JNI must not run until the replacement is active") },
            )
        assertNull(snapshot)
    }
}
