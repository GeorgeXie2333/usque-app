package io.github.georgexie2333.usque

import android.content.Context
import android.content.Intent
import android.content.pm.ShortcutInfo
import android.content.pm.ShortcutManager
import android.graphics.drawable.Icon

internal object AndroidShortcutController {
    fun sync(context: Context) {
        val manager = context.getSystemService(ShortcutManager::class.java)
        val icon = Icon.createWithResource(context, R.drawable.ic_stat_usque)
        manager.dynamicShortcuts =
            listOf(
                shortcut(
                    context,
                    id = "connect",
                    label = "Connect",
                    action = MainActivity.ACTION_SHORTCUT_CONNECT,
                    icon = icon,
                ),
                shortcut(
                    context,
                    id = "disconnect",
                    label = "Disconnect",
                    action = MainActivity.ACTION_SHORTCUT_DISCONNECT,
                    icon = icon,
                ),
                shortcut(
                    context,
                    id = "profiles",
                    label = "Profiles",
                    action = MainActivity.ACTION_SHORTCUT_PROFILES,
                    icon = icon,
                ),
            )
    }

    private fun shortcut(
        context: Context,
        id: String,
        label: String,
        action: String,
        icon: Icon,
    ): ShortcutInfo =
        ShortcutInfo
            .Builder(context, id)
            .setShortLabel(label)
            .setLongLabel("$label Usque")
            .setIcon(icon)
            .setIntent(
                Intent(context, MainActivity::class.java)
                    .setAction(action)
                    .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            ).build()
}
