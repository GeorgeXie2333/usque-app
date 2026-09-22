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
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException

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
    private val reconfigureTimeoutMillis: Long = RECONFIGURE_TIMEOUT_MILLIS,
) {
    companion object {
        const val SNAPSHOT_TIMEOUT_MILLIS = 2_000L
        const val CLEAR_ALL_TIMEOUT_MILLIS = 45_000L
        const val EVENT_REFRESH_INTERVAL_MILLIS = 5_000L

        // Native connection work shares one 180s deadline through final attachment.
        // Binder retains cleanup margin; quick acceptance and snapshots stay short.
        const val RECONFIGURE_TIMEOUT_MILLIS = 200_000L

        fun create(
            context: Context,
            looper: Looper = Looper.getMainLooper(),
        ): VpnControlClient {
            val handler = Handler(looper)
            var replies: Messenger? = null
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
                    // A retry may bind the same live service. Reuse the callback
                    // Binder so its subscriber list cannot accumulate old listeners.
                    val replyMessenger =
                        replies ?: Messenger(
                            Handler(looper) { message ->
                                replyHandler(message.what, message.arg1, message.data)
                            },
                        ).also { replies = it }
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

    private data class SettingsRequest(
        val json: String?,
        val result: MethodChannel.Result,
        var sent: Boolean = false,
    )

    private val pendingSettings = mutableMapOf<Int, SettingsRequest>()
    private val pendingVpnGate = mutableMapOf<Int, SettingsRequest>()
    var vpnGateRefreshPending = false
        private set

    fun requestVpnGate(
        json: String,
        result: MethodChannel.Result,
    ) {
        if (destroyed) {
            result.error("VPN_GATE_UNAVAILABLE", "The catalogue service is unavailable.", null)
            return
        }
        val id = allocateRequestId()
        val request = org.json.JSONObject(json)
        if ((request.optString("command") == "refresh" && !request.optBoolean("cancel")) ||
            (
                request.optString("command") == "node" &&
                    request.optString("action") in setOf("prepare", "favorite", "update_favorite")
            )
        ) {
            vpnGateRefreshPending =
                true
        }
        pendingVpnGate[id] = SettingsRequest(json, result)
        scheduler.postDelayed(50_000L, "vpn-gate-$id") {
            pendingVpnGate
                .remove(
                    id,
                )?.result
                ?.error("VPN_GATE_UNAVAILABLE", "The catalogue service did not respond.", null)
        }
        bind()
        flushVpnGate()
    }

    private fun flushVpnGate() {
        val service = endpoint ?: return
        pendingVpnGate.toMap().forEach { (id, request) ->
            if (!request.sent) {
                request.sent = true
                if (!service.send(UsqueVpnService.MSG_VPN_GATE, id, mapOf("vpn_gate_request" to request.json))) {
                    scheduler.cancel("vpn-gate-$id")
                    pendingVpnGate
                        .remove(
                            id,
                        )?.result
                        ?.error("VPN_GATE_UNAVAILABLE", "The catalogue service is unavailable.", null)
                }
            }
        }
    }

    internal fun deliverVpnGateReply(
        id: Int,
        json: String?,
        error: String?,
    ) {
        scheduler.cancel("vpn-gate-$id")
        val request = pendingVpnGate.remove(id) ?: return
        if (request.json?.let { org.json.JSONObject(it).optString("command") } == "chain_profile") {
            val value = json?.let { runCatching { ChainProfileFields.response(it) }.getOrNull() }
            if (value == null) {
                request.result.error("CHAIN_PROFILE_UNAVAILABLE", "Chain profile request failed.", null)
            } else {
                request.result.success(value)
            }
            return
        }
        val parsed = json?.let { runCatching { VpnGateFields.directory(it) }.getOrNull() }
        if (parsed == null) {
            request.result.error(error ?: "VPN_GATE_UNAVAILABLE", "The catalogue request failed.", null)
        } else {
            val nodeRunning = (parsed["node_progress"] as? Map<*, *>)?.get("stage") == "preparing"
            if ((!nodeRunning && parsed["refresh_stage"] in setOf("complete", "failed", "cancelled")) ||
                org.json.JSONObject(request.json.orEmpty()).optBoolean("cancel")
            ) {
                vpnGateRefreshPending = false
            }
            request.result.success(parsed)
        }
    }

    fun requestNetworkSettings(
        json: String?,
        result: MethodChannel.Result,
    ) {
        if (destroyed) {
            result.error("ENGINE_IPC_CLOSED", "The settings service is unavailable.", null)
            return
        }
        val id = allocateRequestId()
        pendingSettings[id] = SettingsRequest(json, result)
        scheduler.postDelayed(10_000L, "settings-$id") {
            pendingSettings.remove(id)?.result?.error(
                "NETWORK_SETTINGS_UNCONFIRMED",
                "The settings result is not confirmed.",
                null,
            )
        }
        bind()
        flushSettings()
    }

    private fun flushSettings() {
        val service = endpoint ?: return
        pendingSettings.toMap().forEach { (id, request) ->
            if (!request.sent) {
                request.sent = true
                val what =
                    if (request.json ==
                        null
                    ) {
                        UsqueVpnService.MSG_GET_SETTINGS
                    } else {
                        UsqueVpnService.MSG_SAVE_SETTINGS
                    }
                if (!service.send(what, id, mapOf("settings_request" to request.json))) {
                    scheduler.cancel("settings-$id")
                    pendingSettings.remove(id)?.result?.error(
                        "NETWORK_SETTINGS_UNCONFIRMED",
                        "The settings result is not confirmed.",
                        null,
                    )
                }
            }
        }
    }

    internal fun deliverSettingsReply(
        id: Int,
        json: String?,
        error: String? = null,
    ) {
        scheduler.cancel("settings-$id")
        pendingSettings.remove(id)?.result?.let { result ->
            val parsed = json?.let { runCatching { NetworkSettingsFields.decode(it) }.getOrNull() }
            if (parsed == null) {
                result.error(error ?: "NETWORK_SETTINGS_UNCONFIRMED", "Network settings could not be confirmed.", null)
            } else {
                result.success(parsed)
            }
        }
    }

    private val pendingDiagnosticProbes = mutableMapOf<Int, (SnapshotProbe) -> Unit>()
    private var pendingNetworkProbe: Pair<Int, CompletableFuture<String?>>? = null
    private var pendingTimeline: Pair<Int, (Map<String, Any?>?) -> Unit>? = null
    private val pendingClearAll = mutableMapOf<Int, MethodChannel.Result>()
    private var nextSnapshotId = 1
    private var endpoint: ControlEndpoint? = null
    private var controlBound = false
    private var eventsWanted = false
    private var uiVisible = false
    private var eventSubscriptionReachable = false
    private val eventRefreshToken = Any()
    private var eventRefreshGeneration = 0L
    private var pendingDisconnectResult: MethodChannel.Result? = null
    private var pendingRetryResult: MethodChannel.Result? = null
    private var pendingReconfigure: PendingReconfigure? = null
    private var desiredLocaleCatalog: String? = null
    private var pendingPerAppRevision: Long? = null

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

    val isClosed: Boolean
        get() = destroyed

    val hasEndpoint: Boolean
        get() = endpoint != null

    val eventStreamReachable: Boolean
        get() = eventDeliveryWanted && eventSubscriptionReachable && endpoint != null

    private val eventDeliveryWanted: Boolean
        get() = !destroyed && eventsWanted && uiVisible

    data class SnapshotProbe(
        val snapshot: Map<String, Any?>,
        val controlReachable: Boolean,
    )

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
                if (eventDeliveryWanted) {
                    registerForEvents()
                }
                pendingDisconnectResult?.let { result ->
                    pendingDisconnectResult = null
                    scheduler.cancel(disconnectPendingToken(result))
                    requestDisconnect(result)
                }
                flushPendingRetry()
                flushPendingReconfigure()
                flushSettings()
                flushVpnGate()
                flushLocale()
                flushPerApp()
            }

            override fun onServiceDisconnected(name: ComponentName?) {
                endpoint = null
                eventSubscriptionReachable = false
            }

            override fun onBindingDied(name: ComponentName?) {
                endpoint = null
                eventSubscriptionReachable = false
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
                eventSubscriptionReachable = false
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
        eventSubscriptionReachable = false
    }

    fun setEventsWanted(wanted: Boolean) {
        if (destroyed) return
        eventsWanted = wanted
        if (eventDeliveryWanted) {
            bind()
            registerForEvents()
        } else if (!wanted) {
            unregisterForEvents()
        }
    }

    /** Keep the Dart subscription intent across Activity stops, without queuing background status IPC. */
    fun setUiVisible(visible: Boolean) {
        if (destroyed || uiVisible == visible) return
        uiVisible = visible
        if (eventDeliveryWanted) {
            bind()
            // The service may have dropped this subscriber while the UI was frozen,
            // even though its Binder connection is still alive. Registration is
            // idempotent and immediately returns the authoritative snapshot.
            registerForEvents()
        } else if (!visible) {
            unregisterForEvents()
        }
    }

    fun updateLocale(catalogId: String) {
        if (destroyed) return
        desiredLocaleCatalog = catalogId
        flushLocale()
    }

    private fun flushLocale() {
        val service = endpoint ?: return
        val catalogId = desiredLocaleCatalog ?: return
        if (!service.send(UsqueVpnService.MSG_UPDATE_LOCALE, extras = mapOf("catalog_id" to catalogId))) {
            endpoint = null
            eventSubscriptionReachable = false
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

    /** Performs one bounded, read-only snapshot round trip for diagnostics. */
    fun probeSnapshot(callback: (SnapshotProbe) -> Unit) {
        if (destroyed) {
            callback(SnapshotProbe(lastSnapshot.toMap(), false))
            return
        }
        val service = endpoint
        if (service == null) {
            bind()
            callback(SnapshotProbe(lastSnapshot.toMap(), false))
            return
        }
        val requestId = allocateRequestId()
        pendingDiagnosticProbes[requestId] = callback
        if (!service.send(UsqueVpnService.MSG_SNAPSHOT, requestId)) {
            pendingDiagnosticProbes.remove(requestId)
            endpoint = null
            eventSubscriptionReachable = false
            callback(SnapshotProbe(lastSnapshot.toMap(), false))
            return
        }
        scheduler.postDelayed(minOf(snapshotTimeoutMillis, 750L), snapshotTimeoutToken(requestId)) {
            pendingDiagnosticProbes.remove(requestId)?.invoke(
                SnapshotProbe(lastSnapshot.toMap(), false),
            )
        }
    }

    /** One on-demand read, one callback, 750 ms; old native/service versions return no timeline. */
    fun requestTimeline(callback: (Map<String, Any?>?) -> Unit) {
        val service = endpoint
        if (destroyed || service == null || pendingTimeline != null) {
            callback(null)
            return
        }
        val id = allocateRequestId()
        pendingTimeline = id to callback
        if (!service.send(UsqueVpnService.MSG_CONNECTION_TIMELINE, id)) {
            deliverTimelineReply(id, null)
            return
        }
        scheduler.postDelayed(750L, snapshotTimeoutToken(id)) { deliverTimelineReply(id, null) }
    }

    internal fun deliverTimelineReply(
        id: Int,
        raw: String?,
    ) {
        val pending = pendingTimeline?.takeIf { it.first == id } ?: return
        pendingTimeline = null
        scheduler.cancel(snapshotTimeoutToken(id))
        pending.second(NativeTimelineFields.decode(raw))
    }

    /** Called only on the existing diagnostic worker, never on the UI thread. */
    fun runNetworkProbe(
        checkId: String,
        cancelled: () -> Boolean,
    ): Map<String, Any?> {
        val response = CompletableFuture<String?>()
        val started = System.nanoTime()
        scheduler.post {
            val service = endpoint
            if (destroyed || cancelled() || service == null || pendingNetworkProbe != null) {
                response.complete(null)
            } else {
                val id = allocateRequestId()
                pendingNetworkProbe = id to response
                val kind = if (checkId == "transport.h3_path_validation_probe") "h3" else "dns"
                if (!service.send(UsqueVpnService.MSG_DIAGNOSTIC_PROBE, id, mapOf("probe_kind" to kind))) {
                    pendingNetworkProbe = null
                    response.complete(null)
                }
            }
        }

        fun cancelPending() {
            scheduler.post {
                pendingNetworkProbe?.takeIf { it.second === response }?.let { (id, _) ->
                    endpoint?.send(UsqueVpnService.MSG_CANCEL_DIAGNOSTIC_PROBE, id)
                }
            }
        }
        try {
            while (System.nanoTime() - started < 3_850_000_000L && !cancelled()) {
                try {
                    return NetworkDiagnosticChecks.probe(checkId, response.get(50, TimeUnit.MILLISECONDS))
                } catch (
                    _: TimeoutException,
                ) {
                    // bounded wait; recheck session cancellation
                }
            }
            cancelPending()
            try {
                response.get(100, TimeUnit.MILLISECONDS)
            } catch (
                _: Exception,
            ) {
                // native deadline remains authoritative
            }
            return NetworkDiagnosticChecks.probe(
                checkId,
                if (cancelled()) "{\"code\":\"cancelled\"}" else "{\"code\":\"timeout\"}",
            )
        } catch (_: Exception) {
            cancelPending()
            return NetworkDiagnosticChecks.probe(checkId, "{\"code\":\"failed\"}")
        } finally {
            scheduler.post { if (pendingNetworkProbe?.second === response) pendingNetworkProbe = null }
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
            pendingRetryResult?.let { previous ->
                scheduler.cancel(previous)
                previous.error("ENGINE_REQUEST_CANCELLED", "A newer retry superseded this request.", null)
            }
            pendingRetryResult = result
            bind()
            scheduler.postDelayed(snapshotTimeoutMillis, result) {
                if (pendingRetryResult === result) {
                    pendingRetryResult = null
                    result.error("ENGINE_IPC_TIMEOUT", "The VPN process did not accept retry in time.", null)
                }
            }
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

    private fun flushPendingRetry() {
        val result = pendingRetryResult ?: return
        if (endpoint == null) return
        pendingRetryResult = null
        scheduler.cancel(result)
        requestRetry(result)
    }

    private fun cancelPendingConnections(code: String = "ENGINE_REQUEST_CANCELLED") {
        pendingRetryResult?.let {
            pendingRetryResult = null
            scheduler.cancel(it)
            it.error(code, "The connection request was cancelled.", null)
        }
        pendingReconfigure?.let {
            pendingReconfigure = null
            scheduler.cancel(reconfigurePendingToken(it.result))
            it.result.error(code, "The reconfigure request was cancelled.", null)
        }
    }

    fun requestReconfigure(
        profileJson: String,
        result: MethodChannel.Result,
        authOnly: Boolean = false,
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
            if (pendingReconfigure != null) {
                result.error(
                    "RECONFIGURE_IN_PROGRESS",
                    "A reconfigure request is already in progress.",
                    null,
                )
                return true
            }
            pendingReconfigure = PendingReconfigure(profileJson, result, authOnly)
            bind()
            val token = reconfigurePendingToken(result)
            scheduler.postDelayed(reconfigureTimeoutMillis, token) {
                if (pendingReconfigure?.result === result) {
                    pendingReconfigure = null
                    result.error(
                        "ENGINE_IPC_TIMEOUT",
                        "The Android VPN process did not accept the reconfigure request in time.",
                        null,
                    )
                }
            }
            return true
        }
        val requestId = allocateRequestId()
        pendingSnapshots[requestId] = result
        if (!service.send(
                UsqueVpnService.MSG_RECONFIGURE,
                requestId,
                mapOf(UsqueVpnService.EXTRA_PROFILE_JSON to profileJson, "auth_only" to authOnly),
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
        scheduler.postDelayed(reconfigureTimeoutMillis, snapshotTimeoutToken(requestId)) {
            pendingSnapshots.remove(requestId)?.error(
                "ENGINE_IPC_TIMEOUT",
                "The Android VPN process did not reconfigure in time.",
                null,
            )
        }
        return true
    }

    fun notifyApplyPerApp(revision: Long = 0L) {
        if (destroyed) return
        pendingPerAppRevision = maxOf(pendingPerAppRevision ?: 0L, revision)
        if (endpoint == null) bind()
        flushPerApp()
    }

    private fun flushPerApp() {
        val revision = pendingPerAppRevision ?: return
        val service = endpoint ?: return
        if (service.send(UsqueVpnService.MSG_APPLY_PER_APP, extras = mapOf("revision" to revision))) {
            pendingPerAppRevision = null
        } else {
            endpoint = null
        }
    }

    fun requestDisconnect(result: MethodChannel.Result) {
        cancelPendingConnections()
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
        pendingPerAppRevision = null
        cancelPendingConnections()
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
        cancelPendingConnections("ENGINE_IPC_CLOSED")
        pendingVpnGate.forEach { (id, request) ->
            scheduler.cancel("vpn-gate-$id")
            request.result.error("VPN_GATE_UNAVAILABLE", "The catalogue service was closed.", null)
        }
        pendingVpnGate.clear()
        pendingSettings.forEach { (id, request) ->
            scheduler.cancel("settings-$id")
            request.result.error("NETWORK_SETTINGS_UNCONFIRMED", "The settings result is not confirmed.", null)
        }
        pendingSettings.clear()
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

        pendingDiagnosticProbes.keys.toList().forEach { requestId ->
            scheduler.cancel(snapshotTimeoutToken(requestId))
        }
        pendingDiagnosticProbes.values.forEach { callback ->
            callback(SnapshotProbe(lastSnapshot.toMap(), false))
        }
        pendingDiagnosticProbes.clear()
        pendingNetworkProbe?.let { (id, response) ->
            endpoint?.send(UsqueVpnService.MSG_CANCEL_DIAGNOSTIC_PROBE, id)
            response.complete("{\"code\":\"cancelled\"}")
        }
        pendingNetworkProbe = null

        pendingTimeline?.let { (id, callback) ->
            pendingTimeline = null
            scheduler.cancel(snapshotTimeoutToken(id))
            callback(null)
        }

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

        pendingReconfigure?.let { pending ->
            scheduler.cancel(reconfigurePendingToken(pending.result))
            pending.result.error(
                "ENGINE_IPC_CLOSED",
                "The Android UI closed before the session could be reconfigured.",
                null,
            )
        }
        pendingReconfigure = null

        eventsWanted = false
        unregisterForEvents()
        unbind()
        eventListener = null
        clearAllAcknowledgedListener = null
    }

    fun resetAfterClear() {
        cancelPendingConnections()
        pendingPerAppRevision = null
        val oldTimeline = pendingTimeline
        pendingTimeline = null
        oldTimeline?.let { (id, callback) ->
            scheduler.cancel(snapshotTimeoutToken(id))
            callback(null)
        }
        val oldProbes = pendingDiagnosticProbes.toMap()
        pendingDiagnosticProbes.clear()
        oldProbes.forEach { (id, callback) ->
            scheduler.cancel(snapshotTimeoutToken(id))
            callback(SnapshotProbe(disconnectedSnapshot(), false))
        }
        eventsWanted = false
        desiredLocaleCatalog = null
        eventRefreshGeneration++
        scheduler.cancel(eventRefreshToken)
        eventSubscriptionReachable = false
        endpoint = null
        lastSnapshot = disconnectedSnapshot()
        // stopSelf alone cannot destroy a service still retained by this bind.
        if (controlBound) {
            controlBound = false
            runCatching { serviceUnbinder(controlConnection) }
        }
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

        val diagnosticProbe = pendingDiagnosticProbes.remove(requestId)
        if (diagnosticProbe != null) {
            scheduler.cancel(snapshotTimeoutToken(requestId))
            if (errorCode != null) {
                diagnosticProbe(SnapshotProbe(lastSnapshot.toMap(), false))
            } else {
                val payload = snapshot ?: disconnectedSnapshot()
                lastSnapshot = payload
                diagnosticProbe(SnapshotProbe(payload.toMap(), true))
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
        if (!eventDeliveryWanted) return
        eventSubscriptionReachable = true
        // Queued replies can still arrive after a send lost the control endpoint.
        // They must not postpone the deadline that repairs that binding.
        if (endpoint != null) scheduleEventRefresh()
        lastSnapshot = snapshot
        eventListener?.onEvent(snapshot)
    }

    /** Simulates [ServiceConnection.onServiceConnected] for JVM tests. */
    fun attachEndpointForTest(testEndpoint: ControlEndpoint) {
        endpoint = testEndpoint
        if (eventDeliveryWanted) {
            registerForEvents()
        }
        pendingDisconnectResult?.let { result ->
            pendingDisconnectResult = null
            scheduler.cancel(disconnectPendingToken(result))
            requestDisconnect(result)
        }
        flushPendingRetry()
        flushPendingReconfigure()
        flushSettings()
        flushLocale()
        flushPerApp()
    }

    fun detachEndpointForTest() {
        endpoint = null
    }

    fun notifyBindingDiedForTest() {
        controlConnection.onBindingDied(null)
    }

    fun pendingSnapshotCountForTest(): Int = pendingSnapshots.size

    fun pendingDiagnosticProbeCountForTest(): Int = pendingDiagnosticProbes.size

    fun pendingClearAllCountForTest(): Int = pendingClearAll.size

    fun pendingDisconnectForTest(): MethodChannel.Result? = pendingDisconnectResult

    fun pendingReconfigureForTest(): MethodChannel.Result? = pendingReconfigure?.result

    fun pendingReconfigureProfileForTest(): String? = pendingReconfigure?.profileJson

    fun inFlightClearAllForTest(): MethodChannel.Result? = synchronized(clearAllStateLock) { inFlightClearAll }

    fun claimedClearAllForTest(): MethodChannel.Result? = synchronized(clearAllStateLock) { claimedClearAll }

    private fun onReply(
        what: Int,
        arg1: Int,
        data: Bundle,
    ): Boolean =
        when (what) {
            UsqueVpnService.MSG_VPN_GATE -> {
                deliverVpnGateReply(arg1, data.getString("vpn_gate_directory"), data.getString("vpn_gate_error"))
                true
            }

            UsqueVpnService.MSG_SAVE_SETTINGS, UsqueVpnService.MSG_GET_SETTINGS -> {
                deliverSettingsReply(arg1, data.getString("network_settings"), data.getString("settings_error"))
                true
            }

            UsqueVpnService.MSG_SETTINGS_EVENT -> {
                if (eventDeliveryWanted) {
                    data.getString("network_settings")?.let { json ->
                        runCatching { NetworkSettingsFields.decode(json) }.getOrNull()?.let {
                            eventListener?.onEvent(mapOf("network_settings" to it))
                        }
                    }
                }
                true
            }

            UsqueVpnService.MSG_CONNECTION_TIMELINE -> {
                deliverTimelineReply(arg1, data.getString("connection_timeline"))
                true
            }

            UsqueVpnService.MSG_DIAGNOSTIC_PROBE -> {
                pendingNetworkProbe?.takeIf { it.first == arg1 }?.let { (_, response) ->
                    pendingNetworkProbe = null
                    response.complete(data.getString("probe_result"))
                }
                true
            }

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
                if (eventDeliveryWanted) {
                    deliverEvent(snapshotFromBundle(data))
                }
                true
            }

            else -> {
                false
            }
        }

    private fun registerForEvents() {
        if (!eventDeliveryWanted) return
        eventSubscriptionReachable = false
        scheduleEventRefresh()
        sendEventControlMessage(UsqueVpnService.MSG_REGISTER_EVENTS)
    }

    private fun unregisterForEvents() {
        cancelEventRefresh()
        sendEventControlMessage(UsqueVpnService.MSG_UNREGISTER_EVENTS)
        eventSubscriptionReachable = false
    }

    private fun cancelEventRefresh() {
        eventRefreshGeneration++
        scheduler.cancel(eventRefreshToken)
    }

    private fun scheduleEventRefresh() {
        cancelEventRefresh()
        if (!eventDeliveryWanted) return
        val generation = eventRefreshGeneration
        scheduler.postDelayed(EVENT_REFRESH_INTERVAL_MILLIS, eventRefreshToken) {
            if (!eventDeliveryWanted || generation != eventRefreshGeneration) return@postDelayed
            if (endpoint == null) {
                // A failed send or a missing service callback can leave controlBound
                // true without a usable endpoint. Only retry the read-only binding;
                // never retry a command that was already sent.
                unbind()
                bind()
            }
            // Quiet snapshots are normally deduplicated. Silence is not proof of
            // a failed tunnel: ask for a fresh snapshot and renew the subscription.
            registerForEvents()
        }
    }

    private fun sendEventControlMessage(what: Int): Boolean {
        val service = endpoint ?: return false
        if (!service.send(what)) {
            endpoint = null
            eventSubscriptionReachable = false
            return false
        }
        return true
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

    private fun reconfigurePendingToken(result: MethodChannel.Result): Any =
        "reconfigure-pending-${System.identityHashCode(result)}"

    private fun flushPendingReconfigure() {
        pendingReconfigure?.let { pending ->
            pendingReconfigure = null
            scheduler.cancel(reconfigurePendingToken(pending.result))
            requestReconfigure(pending.profileJson, pending.result, pending.authOnly)
        }
    }

    private data class PendingReconfigure(
        val profileJson: String,
        val result: MethodChannel.Result,
        val authOnly: Boolean = false,
    )

    private fun snapshotFromBundle(bundle: Bundle): Map<String, Any?> {
        val failure =
            bundle.getString(ServiceSnapshotState.WireKeys.FAILURE_CODE)?.let { code ->
                mapOf(
                    "code" to code,
                    "stage" to bundle.getString(ServiceSnapshotState.WireKeys.FAILURE_STAGE),
                    "transport" to
                        bundle.getString(ServiceSnapshotState.WireKeys.FAILURE_TRANSPORT),
                    "address_family" to
                        bundle.getString(ServiceSnapshotState.WireKeys.FAILURE_ADDRESS_FAMILY),
                    "retryable" to
                        bundle.getBoolean(ServiceSnapshotState.WireKeys.FAILURE_RETRYABLE),
                    "fallback_allowed" to
                        bundle.getBoolean(
                            ServiceSnapshotState.WireKeys.FAILURE_FALLBACK_ALLOWED,
                        ),
                    "severity" to
                        bundle.getString(ServiceSnapshotState.WireKeys.FAILURE_SEVERITY),
                    "remediation_key" to
                        bundle.getString(ServiceSnapshotState.WireKeys.FAILURE_REMEDIATION_KEY),
                    "sanitized_detail" to
                        bundle.getString(ServiceSnapshotState.WireKeys.FAILURE_SANITIZED_DETAIL),
                ).filterValues { value -> value != null }
            }
        val snapshot =
            mapOf(
                "phase" to (bundle.getString("phase") ?: "error"),
                "warning" to bundle.getString("warning"),
                "error_code" to bundle.getString("error_code"),
                "failure" to failure,
                "transport" to bundle.getString("transport"),
                "data_plane" to L4StatusFields.mode(bundle.getString(ServiceSnapshotState.WireKeys.DATA_PLANE)),
                "l4" to L4StatusFields.decode(bundle.getString(ServiceSnapshotState.WireKeys.L4)),
                "vpn_gate" to VpnGateFields.decodeStatus(bundle.getString(ServiceSnapshotState.WireKeys.VPN_GATE)),
                "address_family" to bundle.getString("address_family"),
                "connected_at" to bundle.getString("connected_at"),
                "download_bytes_per_second" to bundle.getLong("download_bytes_per_second"),
                "upload_bytes_per_second" to bundle.getLong("upload_bytes_per_second"),
                "downloaded_bytes" to bundle.getLong("downloaded_bytes"),
                "uploaded_bytes" to bundle.getLong("uploaded_bytes"),
                "reconnect_count" to bundle.getInt("reconnect_count"),
                "network_quality" to
                    NetworkQualityFields.decode(bundle.getString(ServiceSnapshotState.WireKeys.NETWORK_QUALITY)),
                "direct_dns_mode" to
                    bundle.getString("direct_dns_mode")?.takeIf { it in setOf("physicalSystem", "doh", "dot") },
                "direct_dns_configuration" to
                    bundle.getString("direct_dns_configuration")?.takeIf {
                        it in
                            setOf("valid", "invalid", "unavailable", "unsupported")
                    },
                "active_listeners" to
                    (bundle.getStringArrayList("active_listeners") ?: arrayListOf<String>()),
                "active_frontends" to
                    (
                        bundle.getStringArrayList(ServiceSnapshotState.WireKeys.ACTIVE_FRONTENDS)
                            ?: arrayListOf<String>()
                    ),
                "session_congestion_control" to
                    CongestionControlSettings.token(
                        bundle.getString(ServiceSnapshotState.WireKeys.SESSION_CONGESTION_CONTROL),
                    ),
                "tunnel_ipv4_available" to
                    bundle.getBoolean(ServiceSnapshotState.WireKeys.TUNNEL_IPV4_AVAILABLE),
                "tunnel_ipv6_available" to
                    bundle.getBoolean(ServiceSnapshotState.WireKeys.TUNNEL_IPV6_AVAILABLE),
                "kill_switch_state" to bundle.getString("kill_switch_state"),
                "platform_lockdown" to bundle.getBoolean("platform_lockdown"),
                "always_on" to bundle.getBoolean("always_on"),
                "vpn_service_state" to bundle.getString("vpn_service_state"),
                "vpn_process_state" to bundle.getString("vpn_process_state"),
                "tun_fd_valid" to bundle.getBoolean("tun_fd_valid"),
                "tun_interface_present" to bundle.getBoolean("tun_interface_present"),
                "underlying_network_present" to
                    bundle.getBoolean("underlying_network_present"),
                "underlying_family_mask" to bundle.getInt("underlying_family_mask"),
                "network_generation" to bundle.getLong("network_generation"),
                "dns_server_count" to bundle.getInt("dns_server_count"),
                "native_runtime_state" to bundle.getString("native_runtime_state"),
                "foreground_notification_state" to
                    bundle.getString("foreground_notification_state"),
                "pending_cleanup" to bundle.getBoolean("pending_cleanup"),
                "platform_state_observed" to true,
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
            "platform_state_observed" to false,
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
