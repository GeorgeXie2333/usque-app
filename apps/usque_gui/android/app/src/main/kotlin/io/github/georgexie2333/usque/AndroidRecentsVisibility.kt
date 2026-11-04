package io.github.georgexie2333.usque

import android.app.Activity
import android.app.ActivityManager
import android.content.Intent
import android.os.Build

internal fun recentsVisibilityController(activity: Activity): RecentsVisibilityController =
    RecentsVisibilityController(
        object : RecentsVisibilityController.Preferences {
            override fun read(): Boolean = AndroidPolicyStore.ui(activity).getBoolean("hide_from_recents", false)

            override fun write(hidden: Boolean) {
                AndroidPolicyStore.ui(activity).edit { putBoolean("hide_from_recents", hidden) }
            }
        },
        object : RecentsVisibilityController.Host {
            override val packageName: String get() = activity.packageName
            override val activityClassName: String get() = MainActivity::class.java.name
            override val currentTaskId: Int get() = activity.taskId

            override fun tasks(): List<RecentsVisibilityController.Task> =
                activity.getSystemService(ActivityManager::class.java).appTasks.map { task ->
                    val info = task.taskInfo
                    object : RecentsVisibilityController.Task {
                        @Suppress("DEPRECATION")
                        override val id: Int = if (Build.VERSION.SDK_INT >= 29) info.taskId else info.id
                        override val rootPackage: String? = info.baseActivity?.packageName
                        override val rootClassName: String? = info.baseActivity?.className
                        override val excluded: Boolean =
                            info.baseIntent.flags and Intent.FLAG_ACTIVITY_EXCLUDE_FROM_RECENTS != 0

                        override fun setExcluded(hidden: Boolean) {
                            task.setExcludeFromRecents(hidden)
                        }
                    }
                }
        },
    )
