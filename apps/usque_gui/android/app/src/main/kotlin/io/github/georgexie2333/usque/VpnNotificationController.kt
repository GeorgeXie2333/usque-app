package io.github.georgexie2333.usque

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import java.util.Locale

/**
 * Owns the foreground VPN notification channel and builder. English phase copy
 * stays on [ServiceSnapshotState.notificationText] for JVM unit tests; this
 * controller resolves the same phases through Android string resources.
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
                AndroidLocaleController.getString(context, R.string.vpn_channel_name),
                NotificationManager.IMPORTANCE_LOW,
            ).apply {
                description = AndroidLocaleController.getString(context, R.string.vpn_channel_description)
                setShowBadge(false)
            },
        )
    }

    fun copyFor(snapshot: ServiceSnapshotState): String = copyFor(snapshot.phase, snapshot.transport)

    fun copyFor(
        phase: String,
        transport: String? = null,
    ): String =
        when (phase) {
            "preparing" -> {
                AndroidLocaleController.getString(context, R.string.vpn_notif_preparing)
            }

            "connectingH3" -> {
                AndroidLocaleController.getString(context, R.string.vpn_notif_connecting_h3)
            }

            "connectingH2" -> {
                AndroidLocaleController.getString(context, R.string.vpn_notif_connecting_h2)
            }

            "connected" -> {
                transport?.let {
                    AndroidLocaleController.getString(
                        context,
                        R.string.vpn_notif_connected_via,
                        it.uppercase(Locale.US),
                    )
                } ?: AndroidLocaleController.getString(context, R.string.vpn_notif_connected)
            }

            "degraded" -> {
                AndroidLocaleController.getString(context, R.string.vpn_notif_degraded)
            }

            "reconnecting" -> {
                AndroidLocaleController.getString(context, R.string.vpn_notif_reconnecting)
            }

            "error" -> {
                AndroidLocaleController.getString(context, R.string.vpn_notif_error)
            }

            "disconnecting" -> {
                AndroidLocaleController.getString(context, R.string.vpn_notif_disconnecting)
            }

            else -> {
                AndroidLocaleController.getString(context, R.string.vpn_notif_idle)
            }
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
                        AndroidLocaleController.getString(context, R.string.vpn_notif_disconnect),
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
