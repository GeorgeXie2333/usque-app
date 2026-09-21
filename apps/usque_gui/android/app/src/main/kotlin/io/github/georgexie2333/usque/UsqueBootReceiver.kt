package io.github.georgexie2333.usque

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.Looper
import android.os.UserManager
import androidx.core.content.ContextCompat
import java.io.File
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

class UsqueBootReceiver : BroadcastReceiver() {
    override fun onReceive(
        context: Context,
        intent: Intent,
    ) {
        val userManager = context.getSystemService(UserManager::class.java)
        if (intent.action != Intent.ACTION_BOOT_COMPLETED || !userManager.isUserUnlocked) return
        val pending = goAsync()
        val completed = AtomicBoolean(false)
        val worker = Executors.newSingleThreadExecutor()
        val handler = Handler(Looper.getMainLooper())
        val timeout =
            Runnable {
                if (completed.compareAndSet(false, true)) {
                    worker.shutdownNow()
                    pending.finish()
                }
            }
        handler.postDelayed(timeout, 8_000L)
        worker.execute {
            try {
                val policy = AndroidPolicyStore.startup(context)
                if (!policy.getBoolean(UsqueVpnService.START_ON_BOOT, false)) return@execute
                val path = File(context.noBackupFilesDir, "usque_config/profiles-v2.json").absolutePath
                val catalog =
                    NativeEngine.applyProfileCommand(path, "{\"command\":\"list_profiles\"}") ?: return@execute
                val profile = BootConnectPolicy.profile(catalog) ?: return@execute
                handler.post {
                    if (completed.compareAndSet(false, true)) {
                        try {
                            if (policy.getBoolean(UsqueVpnService.START_ON_BOOT, false)) {
                                ContextCompat.startForegroundService(
                                    context,
                                    Intent(context, UsqueVpnService::class.java)
                                        .setAction(UsqueVpnService.ACTION_CONNECT)
                                        .putExtra(UsqueVpnService.EXTRA_PROFILE_JSON, profile),
                                )
                            }
                        } catch (_: RuntimeException) {
                            // OS policy can deny background start; never reuse a cached profile.
                        } finally {
                            handler.removeCallbacks(timeout)
                            pending.finish()
                        }
                    }
                }
            } catch (_: Exception) {
                // Missing/corrupt policy fails closed: boot never guesses a profile.
            } finally {
                worker.shutdown()
                handler.post {
                    if (completed.compareAndSet(false, true)) {
                        handler.removeCallbacks(timeout)
                        pending.finish()
                    }
                }
            }
        }
    }
}
