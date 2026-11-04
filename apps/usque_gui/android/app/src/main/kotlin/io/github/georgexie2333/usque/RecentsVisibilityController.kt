package io.github.georgexie2333.usque

/** UI-only policy. Task changes never finish an Activity or touch the VPN owner. */
internal class RecentsVisibilityController(
    private val preferences: Preferences,
    private val host: Host,
) {
    interface Preferences {
        fun read(): Boolean

        fun write(hidden: Boolean)
    }

    interface Host {
        val packageName: String
        val activityClassName: String
        val currentTaskId: Int

        fun tasks(): List<Task>
    }

    interface Task {
        val id: Int
        val rootPackage: String?
        val rootClassName: String?
        val excluded: Boolean

        fun setExcluded(hidden: Boolean)
    }

    @Synchronized
    fun restore(): Boolean {
        val hidden = preferences.read()
        updateTasks(hidden) {}
        return hidden
    }

    @Synchronized
    fun setHidden(hidden: Boolean) {
        // A corrupt/unreadable preference must not silently become the default.
        preferences.read()
        updateTasks(hidden) { preferences.write(hidden) }
    }

    private fun updateTasks(
        hidden: Boolean,
        persist: () -> Unit,
    ) {
        val tasks =
            host.tasks().filter {
                it.rootPackage == host.packageName && it.rootClassName == host.activityClassName
            }
        check(tasks.any { it.id == host.currentTaskId }) { "Current UI task unavailable" }
        val previous = tasks.map { it to it.excluded }
        try {
            for ((task, _) in previous) task.setExcluded(hidden)
            persist()
        } catch (error: Exception) {
            // Attempt every rollback even if one task has disappeared. Callers
            // must restore/read back before claiming a confirmed value.
            for ((task, excluded) in previous) runCatching { task.setExcluded(excluded) }
            throw error
        }
    }
}
