package io.github.georgexie2333.usque

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import android.os.RemoteException
import io.flutter.plugin.common.MethodChannel

/**
 * Binder control plane for [UsqueVpnService]: connection lifecycle, request IDs,
 * timeouts, pending Flutter results, and snapshot/event delivery.
 */
internal class VpnControlClient(
    private val scheduler: MainScheduler,
    private val serviceBinder: (ServiceConnection) -> Boolean,
    private val serviceUnbinder: (ServiceConnection) -> Unit,
    private val endpointFromBinder: (
        IBinder,
        replyHandler: (what: Int, arg1: Int, data: Bundle) -> Boolean,
    ) -> ControlEndpoint?,
    private val snapshotTimeoutMillis: Long = SNAPSHOT_TIMEOUT_MILLIS,
    private val clearAllTimeoutMillis: Long = CLEAR_ALL_TIMEOUT_MILLIS,
) {
    companion object {
        const val SNAPSHOT_TIMEOUT_MILLIS = 2_000L
        const val CLEAR_ALL_TIMEOUT_MILLIS = 45_000L

        fun create(
            context: Context,
            looper: Looper = Looper.getMainLooper(),
        ): VpnControlClient {
            val handler = Handler(looper)
            return VpnControlClient(
                scheduler = HandlerMainScheduler(handler),
                serviceBinder = { connection ->
                    context.bindService(
                        Intent(context, UsqueVpnService::class.java)
                            .setAction(UsqueVpnService.ACTION_CONTROL),
                        connection,
                        Context.BIND_AUTO_CREATE,
                    )
                },
                serviceUnbinder = { connection ->
                    context.unbindService(connection)
                },
                endpointFromBinder = { binder, replyHandler ->
                    val replyMessenger =
                        Messenger(
                            Handler(looper) { message ->
                                replyHandler(message.what, message.arg1, message.data)
                            },
                        )
                    MessengerControlEndpoint(Messenger(binder), replyMessenger)
                },
            )
        }
    }

    interface ControlEndpoint {
        fun send(
            what: Int,
            requestId: Int = 0,
            extras: Map<String, Any?>? = null,
        ): Boolean
    }

    interface MainScheduler {
        fun post(action: () -> Unit)

        fun postDelayed(
            delayMillis: Long,
            token: Any,
            action: () -> Unit,
        )

        fun cancel(token: Any)
    }

    fun interface EventListener {
        fun onEvent(snapshot: Map<String, Any?>)
    }

    fun interface ClearAllAcknowledgedListener {
        fun onClearAllAcknowledged(result: MethodChannel.Result)
    }

    var eventListener: EventListener? = null
    var clearAllAcknowledgedListener: ClearAllAcknowledgedListener? = null

    private val pendingSnapshots = mutableMapOf<Int, MethodChannel.Result>()
    private val pendingClearAll = mutableMapOf<Int, MethodChannel.Result>()
    private var nextSnapshotId = 1
    private var endpoint: ControlEndpoint? = null
    private var controlBound = false
    private var eventsWanted = false
    private var pendingDisconnectResult: MethodChannel.Result? = null

    /** Guards the acknowledgement-to-local-wipe ownership transition across threads. */
    private val clearAllStateLock = Any()

    /** Clear-all result acknowledged by the service but not yet claimed by the wipe worker. */
    private var inFlightClearAll: MethodChannel.Result? = null

    /** Clear-all result owned by a running wipe. Once claimed, destroy must not report cancellation. */
    private var claimedClearAll: MethodChannel.Result? = null
    private var destroyed = false

    var lastSnapshot: Map<String, Any?> = disconnectedSnapshot()
        private set

    val isBound: Boolean
        get() = controlBound

    val hasEndpoint: Boolean
        get() = endpoint != null

    val controlConnection: ServiceConnection =
        object : ServiceConnection {
            override fun onServiceConnected(
                name: ComponentName?,
                binder: IBinder?,
            ) {
                if (destroyed) return
                endpoint =
                    if (binder == null) {
                        null
                    } else {
                        endpointFromBinder(binder, ::onReply)
                    }
                if (eventsWanted) {
                    registerForEvents()
                }
                pendingDisconnectResult?.let { result ->
                    pendingDisconnectResult = null
                    scheduler.cancel(disconnectPendingToken(result))
                    requestDisconnect(result)
                }
            }

            override fun onServiceDisconnected(name: ComponentName?) {
                endpoint = null
            }

            override fun onBindingDied(name: ComponentName?) {
                endpoint = null
                if (controlBound) {
                    runCatching { serviceUnbinder(this) }
                    controlBound = false
                }
                if (!destroyed) {
                    bind()
                }
            }

            override fun onNullBinding(name: ComponentName?) {
                endpoint = null
            }
        }

    fun bind() {
        if (destroyed || controlBound) return
        controlBound = serviceBinder(controlConnection)
    }

    fun unbind() {
        if (!controlBound) return
        runCatching { serviceUnbinder(controlConnection) }
        controlBound = false
        endpoint = null
    }

    fun setEventsWanted(wanted: Boolean) {
        if (destroyed) return
        eventsWanted = wanted
        if (wanted) {
            registerForEvents()
        } else {
            unregisterForEvents()
        }
    }

    fun requestSnapshot(result: MethodChannel.Result) {
        if (destroyed) {
            result.error(
                "ENGINE_IPC_CLOSED",
                "The Android UI closed before the VPN process replied.",
                null,
            )
            return
        }
        val service = endpoint
        if (service == null) {
            bind()
            result.success(disconnectedSnapshot())
            return
        }

        val requestId = allocateRequestId()
        pendingSnapshots[requestId] = result
        if (!service.send(UsqueVpnService.MSG_SNAPSHOT, requestId)) {
            pendingSnapshots.remove(requestId)
            endpoint = null
            result.error(
                "ENGINE_IPC_UNAVAILABLE",
                "The Android VPN process could not receive the status request.",
                null,
            )
            return
        }

        scheduler.postDelayed(snapshotTimeoutMillis, snapshotTimeoutToken(requestId)) {
            pendingSnapshots.remove(requestId)?.error(
                "ENGINE_IPC_TIMEOUT",
                "The Android VPN process did not reply in time.",
                null,
            )
        }
    }

    fun requestRetry(result: MethodChannel.Result) {
        if (destroyed) {
            result.error(
                "ENGINE_IPC_CLOSED",
                "The Android UI closed before the connection could be retried.",
                null,
            )
            return
        }
        val service = endpoint
        if (service == null) {
            bind()
            result.success(disconnectedSnapshot())
            return
        }
        val requestId = allocateRequestId()
        pendingSnapshots[requestId] = result
        if (!service.send(UsqueVpnService.MSG_RETRY, requestId)) {
            pendingSnapshots.remove(requestId)
            endpoint = null
            result.error(
                "ENGINE_IPC_UNAVAILABLE",
                "The Android VPN process could not receive the retry request.",
                null,
            )
            return
        }
        scheduler.postDelayed(snapshotTimeoutMillis, snapshotTimeoutToken(requestId)) {
            pendingSnapshots.remove(requestId)?.error(
                "ENGINE_IPC_TIMEOUT",
                "The Android VPN process did not retry in time.",
                null,
            )
        }
    }

    fun requestReconfigure(
        profileJson: String,
        result: MethodChannel.Result,
    ): Boolean {
        if (destroyed) {
            result.error(
                "ENGINE_IPC_CLOSED",
                "The Android UI closed before the session could be reconfigured.",
                null,
            )
            return true
        }
        val service = endpoint
        if (service == null) {
            bind()
            return false
        }
        val requestId = allocateRequestId()
        pendingSnapshots[requestId] = result
        if (!service.send(
                UsqueVpnService.MSG_RECONFIGURE,
                requestId,
                mapOf(UsqueVpnService.EXTRA_PROFILE_JSON to profileJson),
            )
        ) {
            pendingSnapshots.remove(requestId)
            endpoint = null
            result.error(
                "ENGINE_IPC_UNAVAILABLE",
                "The Android VPN process could not receive the reconfigure request.",
                null,
            )
            return true
        }
        scheduler.postDelayed(snapshotTimeoutMillis, snapshotTimeoutToken(requestId)) {
            pendingSnapshots.remove(requestId)?.error(
                "ENGINE_IPC_TIMEOUT",
                "The Android VPN process did not reconfigure in time.",
                null,
            )
        }
        return true
    }

    fun requestDisconnect(result: MethodChannel.Result) {
        if (destroyed) {
            result.error(
                "ENGINE_IPC_CLOSED",
                "The Android UI closed before the connection could be stopped.",
                null,
            )
            return
        }
        val service = endpoint
        if (service == null) {
            if (pendingDisconnectResult != null) {
                result.error(
                    "DISCONNECT_IN_PROGRESS",
                    "A disconnect request is already in progress.",
                    null,
                )
                return
            }
            pendingDisconnectResult = result
            bind()
            val token = disconnectPendingToken(result)
            scheduler.postDelayed(snapshotTimeoutMillis, token) {
                if (pendingDisconnectResult === result) {
                    pendingDisconnectResult = null
                    result.error(
                        "ENGINE_IPC_TIMEOUT",
                        "The Android VPN process did not accept the disconnect request in time.",
                        null,
                    )
                }
            }
            return
        }

        val requestId = allocateRequestId()
        pendingSnapshots[requestId] = result
        if (!service.send(UsqueVpnService.MSG_DISCONNECT, requestId)) {
            pendingSnapshots.remove(requestId)
            endpoint = null
            result.error(
                "ENGINE_IPC_UNAVAILABLE",
                "The Android VPN process could not receive the disconnect request.",
                null,
            )
            return
        }

        scheduler.postDelayed(snapshotTimeoutMillis, snapshotTimeoutToken(requestId)) {
            pendingSnapshots.remove(requestId)?.error(
                "ENGINE_IPC_TIMEOUT",
                "The Android VPN process did not disconnect in time.",
                null,
            )
        }
    }

    /**
     * Sends MSG_CLEAR_ALL_DATA and holds [result] until the service acknowledges.
     * @return false when the control endpoint is unavailable (caller already received the error).
     */
    fun requestClearAllData(result: MethodChannel.Result): Boolean {
        if (destroyed) {
            result.error(
                "CLEAR_ALL_CANCELLED",
                "The Android UI closed before local data could be cleared.",
                null,
            )
            return false
        }
        // Single-slot local wipe tracking cannot own two results; reject overlap.
        val localWipeActive =
            synchronized(clearAllStateLock) {
                inFlightClearAll != null || claimedClearAll != null
            }
        if (pendingClearAll.isNotEmpty() || localWipeActive) {
            result.error(
                "CLEAR_ALL_IN_PROGRESS",
                "Another clear-all operation is already in progress.",
                null,
            )
            return false
        }
        val service = endpoint
        if (service == null) {
            bind()
            result.error(
                "ENGINE_IPC_UNAVAILABLE",
                "The Android network process is not ready. Try again.",
                null,
            )
            return false
        }
        val requestId = allocateRequestId()
        pendingClearAll[requestId] = result
        val extras = mapOf("confirmed" to true)
        if (!service.send(UsqueVpnService.MSG_CLEAR_ALL_DATA, requestId, extras)) {
            pendingClearAll.remove(requestId)
            endpoint = null
            result.error(
                "ENGINE_IPC_UNAVAILABLE",
                "The Android network process could not receive the clear request.",
                null,
            )
            return false
        }
        scheduler.postDelayed(clearAllTimeoutMillis, clearAllTimeoutToken(requestId)) {
            pendingClearAll.remove(requestId)?.error(
                "ENGINE_IPC_TIMEOUT",
                "The Android network process did not disconnect in time.",
                null,
            )
        }
        return true
    }

    fun destroy() {
        val acknowledgedClearAllToCancel =
            synchronized(clearAllStateLock) {
                if (destroyed) return
                destroyed = true
                inFlightClearAll.also { inFlightClearAll = null }
            }

        pendingSnapshots.keys.toList().forEach { requestId ->
            scheduler.cancel(snapshotTimeoutToken(requestId))
        }
        pendingSnapshots.values.forEach { result ->
            result.error(
                "ENGINE_IPC_CLOSED",
                "The Android UI closed before the VPN process replied.",
                null,
            )
        }
        pendingSnapshots.clear()

        pendingClearAll.keys.toList().forEach { requestId ->
            scheduler.cancel(clearAllTimeoutToken(requestId))
        }
        pendingClearAll.values.forEach { result ->
            result.error(
                "CLEAR_ALL_CANCELLED",
                "The Android UI closed before local data could be cleared.",
                null,
            )
        }
        pendingClearAll.clear()

        // Only an acknowledged-but-unclaimed wipe is still cancellable. A claimed wipe owns
        // the destructive operation and will report its real success/failure exactly once.
        acknowledgedClearAllToCancel?.error(
            "CLEAR_ALL_CANCELLED",
            "The Android UI closed before local data could be cleared.",
            null,
        )

        pendingDisconnectResult?.let { result ->
            scheduler.cancel(disconnectPendingToken(result))
            result.error(
                "ENGINE_IPC_CLOSED",
                "The Android UI closed before the connection could be stopped.",
                null,
            )
        }
        pendingDisconnectResult = null

        eventsWanted = false
        unregisterForEvents()
        unbind()
        eventListener = null
        clearAllAcknowledgedListener = null
    }

    /**
     * Atomically claims an acknowledged clear-all result for the wipe worker. Returns false
     * when destroy already cancelled it or another worker owns the destructive operation.
     */
    fun claimInFlightClearAll(result: MethodChannel.Result): Boolean =
        synchronized(clearAllStateLock) {
            if (destroyed || inFlightClearAll !== result || claimedClearAll != null) {
                return@synchronized false
            }
            inFlightClearAll = null
            claimedClearAll = result
            true
        }

    /** Releases a worker-owned clear-all result for its one terminal completion. */
    fun takeClaimedClearAll(result: MethodChannel.Result): Boolean =
        synchronized(clearAllStateLock) {
            if (claimedClearAll !== result) return@synchronized false
            claimedClearAll = null
            true
        }

    /** Test and reply-path entry: complete a pending snapshot/disconnect/pause request. */
    fun deliverSnapshotReply(
        requestId: Int,
        errorCode: String?,
        errorMessage: String?,
        snapshot: Map<String, Any?>?,
    ) {
        if (destroyed) return

        val clearResult = pendingClearAll.remove(requestId)
        if (clearResult != null) {
            scheduler.cancel(clearAllTimeoutToken(requestId))
            if (errorCode != null) {
                clearResult.error(
                    errorCode,
                    errorMessage ?: "The Android VPN process rejected the operation.",
                    null,
                )
            } else {
                val listener = clearAllAcknowledgedListener
                if (listener == null) {
                    clearResult.error(
                        "CLEAR_ALL_FAILED",
                        "Clear-all acknowledgement handler is not configured.",
                        null,
                    )
                } else {
                    // Track as cancellable until the background worker atomically claims it.
                    synchronized(clearAllStateLock) {
                        inFlightClearAll = clearResult
                    }
                    listener.onClearAllAcknowledged(clearResult)
                }
            }
            return
        }

        val result = pendingSnapshots.remove(requestId) ?: return
        scheduler.cancel(snapshotTimeoutToken(requestId))
        if (errorCode != null) {
            result.error(
                errorCode,
                errorMessage ?: "The Android VPN process rejected the operation.",
                null,
            )
        } else {
            val payload = snapshot ?: disconnectedSnapshot()
            lastSnapshot = payload
            result.success(payload)
        }
    }

    fun deliverEvent(snapshot: Map<String, Any?>) {
        lastSnapshot = snapshot
        eventListener?.onEvent(snapshot)
    }

    /** Simulates [ServiceConnection.onServiceConnected] for JVM tests. */
    fun attachEndpointForTest(testEndpoint: ControlEndpoint) {
        endpoint = testEndpoint
        if (eventsWanted) {
            registerForEvents()
        }
        pendingDisconnectResult?.let { result ->
            pendingDisconnectResult = null
            scheduler.cancel(disconnectPendingToken(result))
            requestDisconnect(result)
        }
    }

    fun detachEndpointForTest() {
        endpoint = null
    }

    fun notifyBindingDiedForTest() {
        controlConnection.onBindingDied(null)
    }

    fun pendingSnapshotCountForTest(): Int = pendingSnapshots.size

    fun pendingClearAllCountForTest(): Int = pendingClearAll.size

    fun pendingDisconnectForTest(): MethodChannel.Result? = pendingDisconnectResult

    fun inFlightClearAllForTest(): MethodChannel.Result? = synchronized(clearAllStateLock) { inFlightClearAll }

    fun claimedClearAllForTest(): MethodChannel.Result? = synchronized(clearAllStateLock) { claimedClearAll }

    private fun onReply(
        what: Int,
        arg1: Int,
        data: Bundle,
    ): Boolean =
        when (what) {
            UsqueVpnService.MSG_SNAPSHOT -> {
                val errorCode = data.getString("control_error_code")
                if (errorCode != null) {
                    deliverSnapshotReply(
                        arg1,
                        errorCode,
                        data.getString("control_error_message"),
                        null,
                    )
                } else {
                    deliverSnapshotReply(arg1, null, null, snapshotFromBundle(data))
                }
                true
            }

            UsqueVpnService.MSG_EVENT -> {
                deliverEvent(snapshotFromBundle(data))
                true
            }

            else -> {
                false
            }
        }

    private fun registerForEvents() {
        sendEventControlMessage(UsqueVpnService.MSG_REGISTER_EVENTS)
    }

    private fun unregisterForEvents() {
        sendEventControlMessage(UsqueVpnService.MSG_UNREGISTER_EVENTS)
    }

    private fun sendEventControlMessage(what: Int) {
        val service = endpoint ?: return
        if (!service.send(what)) {
            endpoint = null
        }
    }

    private fun allocateRequestId(): Int {
        val requestId = nextSnapshotId
        nextSnapshotId = if (nextSnapshotId == Int.MAX_VALUE) 1 else nextSnapshotId + 1
        return requestId
    }

    private fun snapshotTimeoutToken(requestId: Int): Any = "snapshot-timeout-$requestId"

    private fun clearAllTimeoutToken(requestId: Int): Any = "clear-all-timeout-$requestId"

    private fun disconnectPendingToken(result: MethodChannel.Result): Any =
        "disconnect-pending-${System.identityHashCode(result)}"

    private fun snapshotFromBundle(bundle: Bundle): Map<String, Any?> {
        val snapshot =
            mapOf(
                "phase" to (bundle.getString("phase") ?: "error"),
                "warning" to bundle.getString("warning"),
                "error_code" to bundle.getString("error_code"),
                "transport" to bundle.getString("transport"),
                "address_family" to bundle.getString("address_family"),
                "connected_at" to bundle.getString("connected_at"),
                "download_bytes_per_second" to bundle.getLong("download_bytes_per_second"),
                "upload_bytes_per_second" to bundle.getLong("upload_bytes_per_second"),
                "downloaded_bytes" to bundle.getLong("downloaded_bytes"),
                "uploaded_bytes" to bundle.getLong("uploaded_bytes"),
                "reconnect_count" to bundle.getInt("reconnect_count"),
                "active_listeners" to
                    (bundle.getStringArrayList("active_listeners") ?: arrayListOf<String>()),
                "kill_switch_state" to bundle.getString("kill_switch_state"),
                "platform_lockdown" to bundle.getBoolean("platform_lockdown"),
                "always_on" to bundle.getBoolean("always_on"),
                "exit_ipv4" to bundle.getString("exit_ipv4"),
                "exit_ipv6" to bundle.getString("exit_ipv6"),
                "exit_city" to bundle.getString("exit_city"),
                "exit_country" to bundle.getString("exit_country"),
                "exit_country_code" to bundle.getString("exit_country_code"),
                "exit_flag_svg" to bundle.getString("exit_flag_svg"),
            )
        lastSnapshot = snapshot
        return snapshot
    }

    fun disconnectedSnapshot(): Map<String, Any> =
        mapOf(
            "phase" to "disconnected",
            "download_bytes_per_second" to 0,
            "upload_bytes_per_second" to 0,
            "downloaded_bytes" to 0,
            "uploaded_bytes" to 0,
        )

    private class MessengerControlEndpoint(
        private val service: Messenger,
        private val replyTo: Messenger,
    ) : ControlEndpoint {
        override fun send(
            what: Int,
            requestId: Int,
            extras: Map<String, Any?>?,
        ): Boolean =
            try {
                service.send(
                    Message.obtain(null, what).apply {
                        arg1 = requestId
                        replyTo = this@MessengerControlEndpoint.replyTo
                        if (extras != null) {
                            data = extrasToBundle(extras)
                        }
                    },
                )
                true
            } catch (_: RemoteException) {
                false
            }

        private fun extrasToBundle(extras: Map<String, Any?>): Bundle {
            val bundle = Bundle()
            for ((key, value) in extras) {
                when (value) {
                    null -> bundle.putString(key, null)
                    is Boolean -> bundle.putBoolean(key, value)
                    is Int -> bundle.putInt(key, value)
                    is Long -> bundle.putLong(key, value)
                    is String -> bundle.putString(key, value)
                    else -> bundle.putString(key, value.toString())
                }
            }
            return bundle
        }
    }

    internal class HandlerMainScheduler(
        private val handler: Handler,
    ) : MainScheduler {
        private val runnables = mutableMapOf<Any, Runnable>()

        override fun post(action: () -> Unit) {
            handler.post(action)
        }

        override fun postDelayed(
            delayMillis: Long,
            token: Any,
            action: () -> Unit,
        ) {
            cancel(token)
            val runnable =
                Runnable {
                    runnables.remove(token)
                    action()
                }
            runnables[token] = runnable
            handler.postDelayed(runnable, delayMillis)
        }

        override fun cancel(token: Any) {
            runnables.remove(token)?.let { handler.removeCallbacks(it) }
        }
    }
}
