package io.github.georgexie2333.usque

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent

/**
 * Owns the foreground VPN notification channel and builder. Status copy is supplied by
 * [ServiceSnapshotState.notificationText] so wording stays unit-testable.
 */
internal class VpnNotificationController(
    private val context: Context,
) {
    companion object {
        const val CHANNEL_ID = "usque_vpn"
        const val NOTIFICATION_ID = 1048
    }

    fun createChannel() {
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_ID,
                "Usque network service",
                NotificationManager.IMPORTANCE_LOW,
            ).apply {
                description = "Persistent status for the active Usque VPN or local proxy"
                setShowBadge(false)
            },
        )
    }

    fun build(status: String): Notification {
        val launchIntent = context.packageManager.getLaunchIntentForPackage(context.packageName)
        val contentIntent =
            PendingIntent.getActivity(
                context,
                0,
                launchIntent,
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        val disconnectIntent =
            PendingIntent.getService(
                context,
                1,
                Intent(context, UsqueVpnService::class.java)
                    .setAction(UsqueVpnService.ACTION_DISCONNECT),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        return Notification
            .Builder(context, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_usque)
            .setContentTitle("Usque")
            .setContentText(status)
            .setContentIntent(contentIntent)
            .setCategory(Notification.CATEGORY_SERVICE)
            .setOngoing(true)
            .addAction(
                Notification.Action
                    .Builder(
                        null,
                        "Disconnect",
                        disconnectIntent,
                    ).build(),
            ).build()
    }

    fun update(status: String) {
        context
            .getSystemService(NotificationManager::class.java)
            .notify(NOTIFICATION_ID, build(status))
    }
}
