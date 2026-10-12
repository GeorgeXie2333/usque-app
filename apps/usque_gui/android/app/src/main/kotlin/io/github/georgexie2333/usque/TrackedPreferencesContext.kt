package io.github.georgexie2333.usque

import android.content.Context
import android.content.ContextWrapper
import android.content.SharedPreferences

/** Only the preferences plugin sees this context; its handlers already run off the UI thread. */
internal class TrackedPreferencesContext(
    context: Context,
) : ContextWrapper(context) {
    override fun getSharedPreferences(
        name: String,
        mode: Int,
    ): SharedPreferences = CommittingPreferences(super.getSharedPreferences(name, mode))

    private class CommittingPreferences(
        private val delegate: SharedPreferences,
    ) : SharedPreferences by delegate {
        override fun edit(): SharedPreferences.Editor = TrackedPreferencesEditor(delegate.edit())
    }
}

/** Include legacy migration and the optional apply-based backend in the work lease. */
internal class TrackedPreferencesEditor(
    private val delegate: SharedPreferences.Editor,
) : SharedPreferences.Editor {
    override fun putString(
        key: String,
        value: String?,
    ): SharedPreferences.Editor = apply { delegate.putString(key, value) }

    override fun putStringSet(
        key: String,
        values: Set<String>?,
    ): SharedPreferences.Editor = apply { delegate.putStringSet(key, values) }

    override fun putInt(
        key: String,
        value: Int,
    ): SharedPreferences.Editor = apply { delegate.putInt(key, value) }

    override fun putLong(
        key: String,
        value: Long,
    ): SharedPreferences.Editor = apply { delegate.putLong(key, value) }

    override fun putFloat(
        key: String,
        value: Float,
    ): SharedPreferences.Editor = apply { delegate.putFloat(key, value) }

    override fun putBoolean(
        key: String,
        value: Boolean,
    ): SharedPreferences.Editor = apply { delegate.putBoolean(key, value) }

    override fun remove(key: String): SharedPreferences.Editor = apply { delegate.remove(key) }

    override fun clear(): SharedPreferences.Editor = apply { delegate.clear() }

    override fun commit(): Boolean = delegate.commit()

    override fun apply() {
        // The plugin's synchronous handler must not release its lease while
        // SharedPreferences still has an untracked disk write queued.
        delegate.commit()
    }
}
