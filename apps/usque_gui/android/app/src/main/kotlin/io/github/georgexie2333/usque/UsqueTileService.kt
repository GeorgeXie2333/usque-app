@file:Suppress("DEPRECATION")

package io.github.georgexie2333.usque

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import android.os.RemoteException
import android.os.SystemClock
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService

/**
 * Native Quick Settings control for the VPN frontend.
 *
 * The tile never starts Flutter and never infers a transition from cross-process
 * SharedPreferences alone. It asks the authoritative `:vpn` process for both
 * snapshots and toggles, so stopping a running foreground service does not go
 * through a new `startForegroundService()` contract.
 */
class UsqueTileService : TileService() {
    companion object {
        private const val CONTROL_TIMEOUT_MILLIS = 3_000L
        private const val CLICK_DEBOUNCE_MILLIS = 1_000L
    }

    private val mainHandler = Handler(Looper.getMainLooper())
    private var activeConnection: ServiceConnection? = null
    private var activeReplyMessenger: Messenger? = null
    private var activeTimeout: Runnable? = null
    private var nextRequestId = 1
    private var lastClickElapsed = 0L

    override fun onStartListening() {
        super.onStartListening()
        // Active tiles get one authoritative update per listening cycle. A
        // durable fallback is used only when the VPN process cannot be queried.
        sendControl(UsqueVpnService.MSG_SNAPSHOT, openAppOnFailure = false)
    }

    override fun onClick() {
        super.onClick()
        val now = SystemClock.elapsedRealtime()
        if (lastClickElapsed != 0L && now - lastClickElapsed < CLICK_DEBOUNCE_MILLIS) return
        lastClickElapsed = now

        render(QuickSettingsTileState.pending("Working"))
        sendControl(UsqueVpnService.MSG_TILE_TOGGLE, openAppOnFailure = true)
    }

    override fun onDestroy() {
        cancelActiveRequest()
        super.onDestroy()
    }

    private fun sendControl(
        what: Int,
        openAppOnFailure: Boolean,
    ) {
        cancelActiveRequest()
        val requestId = nextRequestId++
        val connection =
            object : ServiceConnection {
                override fun onServiceConnected(
                    name: ComponentName?,
                    binder: IBinder?,
                ) {
                    if (activeConnection !== this || binder == null) {
                        if (binder == null) onControlFailure(this, openAppOnFailure)
                        return
                    }
                    val replyMessenger =
                        Messenger(
                            Handler(Looper.getMainLooper()) { reply ->
                                if (
                                    activeConnection !== this ||
                                    reply.what != UsqueVpnService.MSG_SNAPSHOT ||
                                    reply.arg1 != requestId
                                ) {
                                    return@Handler false
                                }
                                val snapshot = Bundle(reply.data)
                                finishRequest(this)
                                handleSnapshot(snapshot, openAppOnFailure)
                                true
                            },
                        )
                    activeReplyMessenger = replyMessenger
                    try {
                        Messenger(binder).send(
                            Message.obtain(null, what).apply {
                                arg1 = requestId
                                replyTo = replyMessenger
                            },
                        )
                    } catch (_: RemoteException) {
                        onControlFailure(this, openAppOnFailure)
                    }
                }

                override fun onServiceDisconnected(name: ComponentName?) {
                    onControlFailure(this, openAppOnFailure)
                }

                override fun onBindingDied(name: ComponentName?) {
                    onControlFailure(this, openAppOnFailure)
                }

                override fun onNullBinding(name: ComponentName?) {
                    onControlFailure(this, openAppOnFailure)
                }
            }
        activeConnection = connection

        val bound =
            runCatching {
                bindService(
                    Intent(this, UsqueVpnService::class.java)
                        .setAction(UsqueVpnService.ACTION_CONTROL),
                    connection,
                    Context.BIND_AUTO_CREATE,
                )
            }.getOrDefault(false)
        if (!bound) {
            onControlFailure(connection, openAppOnFailure)
            return
        }
        if (activeConnection !== connection) return

        val timeout =
            Runnable {
                if (activeConnection === connection) {
                    onControlFailure(connection, openAppOnFailure)
                }
            }
        activeTimeout = timeout
        mainHandler.postDelayed(timeout, CONTROL_TIMEOUT_MILLIS)
    }

    private fun handleSnapshot(
        snapshot: Bundle,
        openAppOnFailure: Boolean,
    ) {
        val controlError = snapshot.getString("control_error_code")
        if (controlError != null) {
            render(QuickSettingsTileState.inactive("Open Usque"))
            if (openAppOnFailure) openApp()
            if (openAppOnFailure) requestAuthoritativeRefresh()
            return
        }
        render(
            QuickSettingsTileState.fromSnapshot(
                snapshot.getString(ServiceSnapshotState.WireKeys.PHASE),
                snapshot.getBoolean(UsqueVpnService.TILE_VPN_ACTIVE),
            ),
        )
        if (openAppOnFailure) requestAuthoritativeRefresh()
    }

    private fun onControlFailure(
        connection: ServiceConnection,
        openAppOnFailure: Boolean,
    ) {
        if (activeConnection !== connection) return
        finishRequest(connection)
        if (openAppOnFailure) {
            render(QuickSettingsTileState.inactive("Open Usque"))
            openApp()
            requestAuthoritativeRefresh()
        } else {
            render(cachedPresentation())
        }
    }

    private fun finishRequest(connection: ServiceConnection) {
        if (activeConnection !== connection) return
        activeTimeout?.let(mainHandler::removeCallbacks)
        activeTimeout = null
        runCatching { unbindService(connection) }
        activeConnection = null
        activeReplyMessenger = null
    }

    private fun cancelActiveRequest() {
        activeConnection?.let(::finishRequest)
    }

    private fun render(presentation: QuickSettingsTileState.Presentation) {
        qsTile?.apply {
            state =
                when (presentation.state) {
                    QuickSettingsTileState.State.ACTIVE -> Tile.STATE_ACTIVE
                    QuickSettingsTileState.State.INACTIVE -> Tile.STATE_INACTIVE
                    QuickSettingsTileState.State.UNAVAILABLE -> Tile.STATE_UNAVAILABLE
                }
            label = "Usque"
            contentDescription =
                if (presentation.subtitle == null) {
                    "Usque"
                } else {
                    "Usque, ${presentation.subtitle}"
                }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                subtitle = presentation.subtitle
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                stateDescription = presentation.subtitle
            }
            updateTile()
        }
    }

    private fun cachedPresentation(): QuickSettingsTileState.Presentation {
        val recovery =
            createDeviceProtectedStorageContext().getSharedPreferences(
                UsqueVpnService.RECOVERY_PREFERENCES,
                MODE_PRIVATE,
            )
        return if (recovery.contains(UsqueVpnService.RECOVERY_PROFILE)) {
            QuickSettingsTileState.active()
        } else {
            QuickSettingsTileState.inactive()
        }
    }

    private fun requestAuthoritativeRefresh() {
        requestListeningState(this, ComponentName(this, UsqueTileService::class.java))
    }

    @SuppressLint("StartActivityAndCollapseDeprecated")
    private fun openApp() {
        val launch =
            packageManager.getLaunchIntentForPackage(packageName)?.apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            } ?: return
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            val pending =
                PendingIntent.getActivity(
                    this,
                    0,
                    launch,
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                )
            startActivityAndCollapse(pending)
        } else {
            startActivityAndCollapse(launch)
        }
    }
}
