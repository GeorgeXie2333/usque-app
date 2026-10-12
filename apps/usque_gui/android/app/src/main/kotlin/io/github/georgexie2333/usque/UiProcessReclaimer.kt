package io.github.georgexie2333.usque

import android.app.Activity
import android.app.ActivityManager
import android.app.Application
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.Process
import java.util.concurrent.ExecutorService

internal object UiProcessReclaimer {
    @Volatile
    private var coordinator: UiProcessExitCoordinator? = null

    fun install(application: Application) {
        if (coordinator != null || !isUiProcess(application.packageName, currentProcessName(application))) return
        val handler = Handler(Looper.getMainLooper())
        val installed =
            UiProcessExitCoordinator(
                schedule = { delay, callback ->
                    val task = Runnable { callback() }
                    handler.postDelayed(task, delay)
                    UiExitCancellation { handler.removeCallbacks(task) }
                },
                // Only the verified default UI process installs this coordinator.
                // In particular, this never targets the separate :vpn process.
                terminate = { Process.killProcess(Process.myPid()) },
            )
        coordinator = installed
        application.registerActivityLifecycleCallbacks(
            object : Application.ActivityLifecycleCallbacks {
                override fun onActivityCreated(
                    activity: Activity,
                    savedInstanceState: Bundle?,
                ) {
                    installed.activityCreated(activity)
                }

                override fun onActivityStarted(activity: Activity) {
                    installed.activityStarted(activity)
                }

                override fun onActivityDestroyed(activity: Activity) {
                    installed.activityDestroyed(
                        activity,
                        isMainActivity = activity is MainActivity,
                        isFinishing = activity.isFinishing,
                        isChangingConfigurations = activity.isChangingConfigurations,
                    )
                }

                override fun onActivityResumed(activity: Activity) = Unit

                override fun onActivityPaused(activity: Activity) = Unit

                override fun onActivityStopped(activity: Activity) = Unit

                override fun onActivitySaveInstanceState(
                    activity: Activity,
                    outState: Bundle,
                ) = Unit
            },
        )
    }

    fun awaitCleanup(executors: List<ExecutorService>) {
        val retired = executors.toList()
        coordinator?.awaitCleanup { retired.all { it.isTerminated } }
    }

    fun holdBackgroundWork(): AutoCloseable = coordinator?.holdBackgroundWork() ?: AutoCloseable { }

    private fun currentProcessName(application: Application): String? =
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                Application.getProcessName()
            } else {
                application
                    .getSystemService(ActivityManager::class.java)
                    ?.runningAppProcesses
                    ?.firstOrNull { it.pid == Process.myPid() }
                    ?.processName
            }
        } catch (_: RuntimeException) {
            null
        }
}
