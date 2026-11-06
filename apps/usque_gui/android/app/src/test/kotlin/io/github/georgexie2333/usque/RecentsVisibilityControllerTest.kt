package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException
import java.nio.file.Files

class RecentsVisibilityControllerTest {
    private class Preferences : RecentsVisibilityController.Preferences {
        var hidden = false
        var failRead = false
        var failWrite = false

        override fun read(): Boolean {
            if (failRead) throw IOException("private path")
            return hidden
        }

        override fun write(hidden: Boolean) {
            if (failWrite) throw IOException("private path")
            this.hidden = hidden
        }
    }

    private class Task(
        override val id: Int,
        override val rootPackage: String? = "usque",
        override val rootClassName: String? = "usque.MainActivity",
        excluded: Boolean = false,
    ) : RecentsVisibilityController.Task {
        private var hidden = excluded
        override val excluded: Boolean get() = hidden
        var fail = false
        val changes = mutableListOf<Boolean>()

        override fun setExcluded(hidden: Boolean) {
            if (fail) throw IllegalStateException("task disappeared")
            changes += hidden
            this.hidden = hidden
        }
    }

    private class Host(
        val entries: MutableList<Task> = mutableListOf(Task(1)),
    ) : RecentsVisibilityController.Host {
        override val packageName = "usque"
        override val activityClassName = "usque.MainActivity"
        override val currentTaskId = 1

        override fun tasks(): List<RecentsVisibilityController.Task> = entries
    }

    @Test
    fun defaultToggleAndRestoreIncludeAllOwnMainTasksOnly() {
        val preferences = Preferences()
        val host = Host()
        val second = Task(2)
        val foreign = Task(3, rootPackage = "browser")
        val otherActivity = Task(4, rootClassName = "usque.OtherActivity")
        host.entries += listOf(second, foreign, otherActivity)
        val controller = RecentsVisibilityController(preferences, host)
        assertFalse(controller.restore())
        controller.setHidden(true)
        assertTrue(preferences.hidden)
        assertTrue(host.entries.first().excluded)
        assertTrue(second.excluded)
        assertTrue(foreign.changes.isEmpty())
        assertTrue(otherActivity.changes.isEmpty())
        // A newly created task picks up persisted policy even in a new owner.
        host.entries[0] = Task(1)
        val recreated = RecentsVisibilityController(preferences, host)
        assertTrue(recreated.restore())
        assertTrue(recreated.restore())
        assertTrue(host.entries.first().excluded)
        recreated.setHidden(false)
        assertFalse(preferences.hidden)
        assertFalse(host.entries.first().excluded)
        assertFalse(second.excluded)
    }

    @Test
    fun writeFailureRollsBackEachTasksOriginalFlag() {
        val preferences = Preferences().apply { failWrite = true }
        val host = Host().apply { entries += Task(2, excluded = true) }
        val controller = RecentsVisibilityController(preferences, host)
        assertThrows(IOException::class.java) { controller.setHidden(true) }
        assertFalse(preferences.hidden)
        assertFalse(host.entries[0].excluded)
        assertTrue(host.entries[1].excluded)
    }

    @Test
    fun taskFailureRollsBackEarlierTasksWithoutSaving() {
        val preferences = Preferences()
        val host = Host().apply { entries += Task(2).apply { fail = true } }
        val controller = RecentsVisibilityController(preferences, host)
        assertThrows(IllegalStateException::class.java) { controller.setHidden(true) }
        assertFalse(preferences.hidden)
        assertFalse(host.entries.first().excluded)
    }

    @Test
    fun missingCurrentTaskNeverSavesOrChangesUnrelatedTasks() {
        val preferences = Preferences()
        val host = Host(mutableListOf(Task(2)))
        assertThrows(IllegalStateException::class.java) {
            RecentsVisibilityController(preferences, host).setHidden(true)
        }
        assertFalse(preferences.hidden)
        assertTrue(
            host.entries
                .single()
                .changes
                .isEmpty(),
        )
    }

    @Test
    fun unreadablePolicyNeverSilentlyRestoresFalse() {
        val preferences = Preferences().apply { failRead = true }
        val host = Host(mutableListOf(Task(1, excluded = true)))
        val controller = RecentsVisibilityController(preferences, host)
        assertThrows(IOException::class.java) { controller.restore() }
        assertThrows(IOException::class.java) { controller.setHidden(false) }
        assertTrue(host.entries.single().excluded)
        assertTrue(
            host.entries
                .single()
                .changes
                .isEmpty(),
        )
    }

    @Test
    fun atomicPreferenceSurvivesReopenAndClearRestoresVisibleTask() {
        val root = Files.createTempDirectory("usque-recents-test").toFile()
        try {
            val file = java.io.File(root, "ui.json")

            fun preferences() =
                object : RecentsVisibilityController.Preferences {
                    override fun read(): Boolean = AtomicPolicyStore(file).getBoolean("hide_from_recents", false)

                    override fun write(hidden: Boolean) {
                        AtomicPolicyStore(file).edit { putBoolean("hide_from_recents", hidden) }
                    }
                }
            val host = Host()
            RecentsVisibilityController(preferences(), host).setHidden(true)
            assertTrue(RecentsVisibilityController(preferences(), host).restore())
            AtomicPolicyStore(file).edit { clear() }
            assertFalse(RecentsVisibilityController(preferences(), host).restore())
            assertFalse(host.entries.single().excluded)
            assertEquals(emptyMap<String, Any?>(), AtomicPolicyStore(file).snapshot())
        } finally {
            root.deleteRecursively()
        }
    }
}
