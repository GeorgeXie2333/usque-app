package io.github.georgexie2333.usque

import android.content.SharedPreferences
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class TrackedPreferencesEditorTest {
    private class Editor : SharedPreferences.Editor {
        val pending = mutableMapOf<String, Any?>()
        val persisted = mutableMapOf<String, Any?>()
        var commits = 0
        var applies = 0
        var commitResult = true

        override fun putString(
            key: String,
            value: String?,
        ): SharedPreferences.Editor = apply { pending[key] = value }

        override fun putStringSet(
            key: String,
            values: Set<String>?,
        ): SharedPreferences.Editor = apply { pending[key] = values }

        override fun putInt(
            key: String,
            value: Int,
        ): SharedPreferences.Editor = apply { pending[key] = value }

        override fun putLong(
            key: String,
            value: Long,
        ): SharedPreferences.Editor = apply { pending[key] = value }

        override fun putFloat(
            key: String,
            value: Float,
        ): SharedPreferences.Editor = apply { pending[key] = value }

        override fun putBoolean(
            key: String,
            value: Boolean,
        ): SharedPreferences.Editor = apply { pending[key] = value }

        override fun remove(key: String): SharedPreferences.Editor = apply { pending.remove(key) }

        override fun clear(): SharedPreferences.Editor = apply { pending.clear() }

        override fun commit(): Boolean {
            commits++
            if (commitResult) {
                persisted.clear()
                persisted.putAll(pending)
            }
            return commitResult
        }

        override fun apply() {
            applies++
        }
    }

    @Test
    fun everyFluentMutationKeepsTheWrapper() {
        val delegate = Editor()
        val editor = TrackedPreferencesEditor(delegate)
        assertSame(editor, editor.putString("string", "saved"))
        assertSame(editor, editor.putStringSet("set", setOf("saved")))
        assertSame(editor, editor.putInt("int", 1))
        assertSame(editor, editor.putLong("long", 2L))
        assertSame(editor, editor.putFloat("float", 3f))
        assertSame(editor, editor.putBoolean("bool", true))
        assertSame(editor, editor.remove("int"))
        assertEquals(5, delegate.pending.size)
        assertSame(editor, editor.clear())
        assertTrue(delegate.pending.isEmpty())
    }

    @Test
    fun chainedApplyPersistsBeforeItReturns() {
        val delegate = Editor()
        TrackedPreferencesEditor(delegate)
            .putString("theme", "dark")
            .putBoolean("onboarding_complete", true)
            .remove("unused")
            .apply()
        assertEquals("dark", delegate.persisted["theme"])
        assertEquals(true, delegate.persisted["onboarding_complete"])
        assertEquals(1, delegate.commits)
        assertEquals(0, delegate.applies)
    }

    @Test
    fun clearThenApplyAlsoCommitsSynchronously() {
        val delegate = Editor()
        delegate.putString("old", "value").commit()
        TrackedPreferencesEditor(delegate).clear().putString("new", "value").apply()
        assertEquals(mapOf("new" to "value"), delegate.persisted)
        assertEquals(2, delegate.commits)
        assertEquals(0, delegate.applies)
    }

    @Test
    fun commitPreservesTheStorageResult() {
        val delegate = Editor()
        val editor = TrackedPreferencesEditor(delegate)
        assertTrue(editor.putString("theme", "dark").commit())
        delegate.commitResult = false
        assertFalse(editor.putString("theme", "light").commit())
        assertEquals("dark", delegate.persisted["theme"])
        assertEquals(2, delegate.commits)
    }
}
