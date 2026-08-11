package io.github.georgexie2333.usque

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.UserManager
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import org.json.JSONObject

class UsqueBootReceiver : BroadcastReceiver() {
    override fun onReceive(
        context: Context,
        intent: Intent,
    ) {
        val userManager = context.getSystemService(UserManager::class.java)
        if (intent.action != Intent.ACTION_BOOT_COMPLETED || !userManager.isUserUnlocked) return
        val deviceContext = context.createDeviceProtectedStorageContext()
        val preferences =
            deviceContext.getSharedPreferences(
                UsqueVpnService.RECOVERY_PREFERENCES,
                Context.MODE_PRIVATE,
            )
        if (!preferences.getBoolean(UsqueVpnService.START_ON_BOOT, false)) return
        val profile = preferences.getString(UsqueVpnService.LAST_PROFILE, null) ?: return
        if (!runCatching { JSONObject(profile).optBoolean("auto_connect", false) }.getOrDefault(false)) {
            return
        }
        try {
            ContextCompat.startForegroundService(
                context,
                Intent(context, UsqueVpnService::class.java)
                    .setAction(UsqueVpnService.ACTION_CONNECT_LAST),
            )
        } catch (_: RuntimeException) {
            preferences.edit { putBoolean("boot_connect_pending", true) }
        }
    }
}
