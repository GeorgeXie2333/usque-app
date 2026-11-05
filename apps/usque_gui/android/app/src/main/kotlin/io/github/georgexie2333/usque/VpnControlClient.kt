package io.github.georgexie2333.usque

import android.app.ActivityManager
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
import org.json.JSONObject
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException

/**
 * Binder control plane for [UsqueVpnService]: connection lifecycle, request IDs,
 * timeouts, pending Flutter results, and snapshot/event delivery.
 */
internal class VpnControlClient(
    private val scheduler: MainScheduler,
    private val serviceBinder: (ServiceConnection, Boolean) -> Boolean,
    private val serviceUnbinder: (ServiceConnection) -> Unit,
    private val endpointFromBinder: (
        IBinder,
        replyHandler: (what: Int, arg1: Int, data: Bundle) -> Boolean,
    ) -> ControlEndpoint?,
    private val snapshotTimeoutMillis: Long = SNAPSHOT_TIMEOUT_MILLIS,
    private val clearAllTimeoutMillis: Long = CLEAR_ALL_TIMEOUT_MILLIS,
    private val reconfigureTimeoutMillis: Long = RECONFIGURE_TIMEOUT_MILLIS,
    private val nowNanos: () -> Long = System::nanoTime,
    private val controlServiceProcessAbsent: () -> Boolean = { false },
) {
    companion object {
        const val SNAPSHOT_TIMEOUT_MILLIS = 2_000L
        const val CLEAR_ALL_TIMEOUT_MILLIS = 45_000L
        const val EVENT_REFRESH_INTERVAL_MILLIS = 5_000L
        private const val PROBE_OPERATION_NANOS = 3_850_000_000L
        private const val PROBE_TOTAL_NANOS = 4_000_000_000L
        private const val PROBE_CLEANUP_NANOS = 500_000_000L

        // Covers the maximum automatic underlay plus chain startup and native
        // reporting margin. Quick acceptance and snapshot deadlines stay short.
        const val RECONFIGURE_TIMEOUT_MILLIS = 715_000L

        fun create(
            context: Context,
            looper: Looper = Looper.getMainLooper(),
        ): VpnControlClient {
            val handler = Handler(looper)
            var replies: Messenger? = null
            var repliesFor: ((Int, Int, Bundle) -> Boolean)? = null
            return VpnControlClient(
                scheduler = HandlerMainScheduler(handler),
                serviceBinder = { connection, creating ->
                    context.bindService(
                        Intent(context, UsqueVpnService::class.java)
                            .setAction(UsqueVpnService.ACTION_CONTROL),
                        connection,
                        if (creating) Context.BIND_AUTO_CREATE else 0,
                    )
                },
                serviceUnbinder = { connection ->
                    context.unbindService(connection)
                },
                endpointFromBinder = { binder, replyHandler ->
                    // A retry may bind the same live service. Reuse the callback
                    // Binder so its subscriber list cannot accumulate old listeners.
                    val replyMessenger =
                        replies.takeIf { repliesFor === replyHandler } ?: Messenger(
                            Handler(looper) { message ->
                                replyHandler(message.what, message.arg1, message.data)
                            },
                        ).also {
                            replies = it
                            repliesFor = replyHandler
                        }
                    MessengerControlEndpoint(Messenger(binder), replyMessenger)
                },
                controlServiceProcessAbsent = {
                    ControlServiceProcess.knownAbsent(
                        context.packageName,
                        context
                            .getSystemService(ActivityManager::class.java)
                            ?.runningAppProcesses
                            ?.map { it.processName },
                    )
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
        val scenario: Boolean = false,
        var jobDomain: String? = null,
        var jobVersion: Long = 0,
        val passive: Boolean = false,
    )

    private val pendingSettings = mutableMapOf<Int, SettingsRequest>()
    private val pendingVpnGate = mutableMapOf<Int, SettingsRequest>()

    private class ProtonRequest(
        val command: String,
        val input: ByteArray,
        val result: MethodChannel.Result,
        var sent: Boolean = false,
        var jobVersion: Long = 0,
    )

    private val pendingProton = mutableMapOf<Int, ProtonRequest>()

    fun requestProtonVpn(
        command: String,
        input: ByteArray,
        result: MethodChannel.Result,
    ) {
        if (runCatching { JSONObject(command) }.isFailure) {
            result.error("PROTON_UNAVAILABLE", "The Proton request is invalid.", null)
            return
        }
        requestProtonVpnOwned(command, input, if (input.size > 40 * 1024) result else creatingResult(result))
    }

    fun requestVpnGate(
        json: String,
        result: MethodChannel.Result,
    ) {
        if (runCatching { JSONObject(json) }.isFailure) {
            result.error("VPN_GATE_UNAVAILABLE", "The catalogue request is invalid.", null)
            return
        }
        requestVpnGateOwned(json, creatingResult(result))
    }

    fun requestRetry(
        result: MethodChannel.Result,
        continuous: Boolean = false,
    ) = requestRetryOwned(creatingResult(result), continuous)

    fun requestReconfigure(
        profileJson: String,
        result: MethodChannel.Result,
        authOnly: Boolean = false,
        accountSelection: Boolean = false,
    ): Boolean = requestReconfigureOwned(profileJson, creatingResult(result), authOnly, accountSelection)

    fun requestDisconnect(result: MethodChannel.Result) = requestDisconnectOwned(creatingResult(result))

    private fun requestProtonVpnOwned(
        command: String,
        input: ByteArray,
        result: MethodChannel.Result,
    ) {
        if (destroyed || input.size > 40 * 1024) {
            result.error("PROTON_UNAVAILABLE", "Proton VPN is unavailable.", null)
            return
        }
        val id = allocateRequestId()
        pendingProton[id] = ProtonRequest(command, input.copyOf(), result)
        postDelayed(50_000L, "proton-$id") {
            pendingProton.remove(id)?.let {
                if (it.sent && currentJobReply(it.result, "proton", it.jobVersion)) rememberProtonJob(it.command)
                it.input.fill(0)
                it.result.error("PROTON_RESULT_UNCONFIRMED", "Refresh Proton status before retrying.", null)
            }
        }
        bind()
        flushProton()
    }

    private fun flushProton() {
        val service = endpoint ?: return
        pendingProton.toMap().forEach { (id, request) ->
            if (!request.sent) {
                request.sent = true
                markSent(request.result)
                request.jobVersion = observeJobDispatch("proton", startsProtonJob(request.command))
                val sent =
                    service.send(
                        UsqueVpnService.MSG_PROTON_VPN,
                        id,
                        mapOf(
                            "proton_command" to request.command,
                            "proton_input" to request.input,
                        ),
                    )
                request.input.fill(0)
                if (!sent) {
                    scheduler.cancel("proton-$id")
                    pendingProton.remove(id)?.result?.error("PROTON_UNAVAILABLE", "Proton VPN is unavailable.", null)
                } else if (pendingProton.containsKey(id) &&
                    currentJobReply(request.result, "proton", request.jobVersion)
                ) {
                    rememberProtonJob(request.command)
                }
            }
        }
    }

    private fun deliverProtonReply(
        id: Int,
        data: Bundle,
    ) = deliverProtonReply(
        id,
        data.getByteArray("proton_metadata") ?: ByteArray(0),
        data.getByteArray("proton_handoff") ?: ByteArray(0),
    )

    internal fun deliverProtonReply(
        id: Int,
        metadata: ByteArray,
        handoff: ByteArray = ByteArray(0),
    ) {
        scheduler.cancel("proton-$id")
        try {
            val request = pendingProton.remove(id) ?: return
            request.input.fill(0)
            val value =
                runCatching {
                    require(handoff.size <= 64 * 1024)
                    require(handoff.isEmpty() || JSONObject(request.command).optString("action") == "handoff")
                    mapOf("metadata" to ProtonVpnFields.response(metadata), "sensitive_handoff" to handoff)
                }.getOrNull()
            if (value ==
                null
            ) {
                request.result.error("PROTON_UNAVAILABLE", "Proton request failed.", null)
            } else {
                if (currentJobReply(request.result, "proton", request.jobVersion)) {
                    val metadataValue = value["metadata"] as? Map<*, *>
                    updateServiceJob(
                        "proton",
                        metadataValue?.get("job") as? Map<*, *>,
                        metadataValue?.get("error"),
                        JSONObject(request.command).optString("action") == "get",
                        JSONObject(request.command).optString("job_id").takeIf { it.isNotEmpty() },
                    )
                }
                request.result.success(value)
            }
        } finally {
            metadata.fill(0)
            handoff.fill(0)
        }
    }

    var vpnGateRefreshPending = false
        private set
    var warpGenerationPending = false
        private set

    private fun requestVpnGateOwned(
        json: String,
        result: MethodChannel.Result,
    ) {
        if (destroyed) {
            result.error("VPN_GATE_UNAVAILABLE", "The catalogue service is unavailable.", null)
            return
        }
        val id = allocateRequestId()
        val request = org.json.JSONObject(json)
        if (request.optString("command") == "warp_wireguard" &&
            request.optJSONObject("warp_wireguard")?.optString("action") == "generate"
        ) {
            warpGenerationPending = true
        }
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
        postDelayed(50_000L, "vpn-gate-$id") {
            pendingVpnGate.remove(id)?.let {
                if (it.sent && currentJobReply(it.result, it.jobDomain, it.jobVersion)) rememberVpnGateJob(it.json)
                it.result.error("VPN_GATE_UNAVAILABLE", "The catalogue service did not respond.", null)
            }
        }
        bind()
        flushVpnGate()
    }

    private fun flushVpnGate() {
        val service = endpoint ?: return
        pendingVpnGate.toMap().forEach { (id, request) ->
            if (!request.sent) {
                request.sent = true
                markSent(request.result)
                request.jobDomain = vpnGateJobDomain(request.json)
                request.jobVersion = observeJobDispatch(request.jobDomain, startsVpnGateJob(request.json))
                if (!service.send(UsqueVpnService.MSG_VPN_GATE, id, mapOf("vpn_gate_request" to request.json))) {
                    scheduler.cancel("vpn-gate-$id")
                    pendingVpnGate
                        .remove(
                            id,
                        )?.result
                        ?.error("VPN_GATE_UNAVAILABLE", "The catalogue service is unavailable.", null)
                } else if (pendingVpnGate.containsKey(id) &&
                    currentJobReply(request.result, request.jobDomain, request.jobVersion)
                ) {
                    rememberVpnGateJob(request.json)
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
        if (request.json?.let { org.json.JSONObject(it).optString("command") } == "warp_wireguard") {
            val value = json?.let { runCatching { WarpWireguardFields.response(it) }.getOrNull() }
            if (value == null) {
                request.result.error(
                    WarpWireguardFields.failureCode(error),
                    "WARP configuration generation request failed.",
                    null,
                )
            } else {
                if (currentJobReply(request.result, request.jobDomain, request.jobVersion)) {
                    if (value["error"] == null) {
                        warpGenerationPending = (value["job"] as? Map<*, *>)?.get("state") == "running"
                    }
                    updateServiceJob(
                        "warp",
                        value["job"] as? Map<*, *>,
                        value["error"],
                        JSONObject(
                            request.json.orEmpty(),
                        ).optJSONObject("warp_wireguard")?.optString("action") == "get",
                        JSONObject(
                            request.json.orEmpty(),
                        ).optJSONObject("warp_wireguard")?.optString("job_id")?.takeIf { it.isNotEmpty() },
                    )
                }
                request.result.success(value)
            }
            return
        }
        val parsed = json?.let { runCatching { VpnGateFields.directory(it) }.getOrNull() }
        if (parsed == null) {
            request.result.error(error ?: "VPN_GATE_UNAVAILABLE", "The catalogue request failed.", null)
        } else {
            val nodeRunning = (parsed["node_progress"] as? Map<*, *>)?.get("stage") == "preparing"
            if (currentJobReply(request.result, request.jobDomain, request.jobVersion)) {
                if ((!nodeRunning && parsed["refresh_stage"] in setOf("complete", "failed", "cancelled")) ||
                    org.json.JSONObject(request.json.orEmpty()).optBoolean("cancel")
                ) {
                    vpnGateRefreshPending = false
                    if (org.json.JSONObject(request.json.orEmpty()).optBoolean("cancel")) warpGenerationPending = false
                }
                val stage = parsed["refresh_stage"] as? String
                if (nodeRunning || (stage != null && stage !in setOf("idle", "complete", "failed", "cancelled"))) {
                    jobs["catalogue"] = ServiceJob("catalogue", null, serviceEpoch)
                } else if (stage in setOf("idle", "complete", "failed", "cancelled")) {
                    jobs.remove("catalogue")
                }
                scheduleJobRefresh()
            }
            request.result.success(parsed)
        }
    }

    fun requestNetworkSettings(
        json: String?,
        result: MethodChannel.Result,
        scenario: Boolean = false,
    ) {
        if (destroyed) {
            result.error("ENGINE_IPC_CLOSED", "The settings service is unavailable.", null)
            return
        }
        val id = allocateRequestId()
        val creating =
            json != null && (!scenario || runCatching { JSONObject(json).optString("action") }.getOrNull() != "list")
        pendingSettings[id] =
            SettingsRequest(
                json,
                if (creating) creatingResult(result) else result,
                scenario = scenario,
                passive = !creating,
            )
        postDelayed(10_000L, "settings-$id") {
            pendingSettings.remove(id)?.let { request ->
                // A non-creating bind can be accepted without ever delivering
                // an endpoint. Only an unsent passive read may use the saved
                // configuration fallback; sent reads and writes stay uncertain.
                val inactive = !creating && !request.sent
                if (inactive && uiVisible && endpoint == null && !bindings.createsService) {
                    controlServiceInactive = true
                }
                request.result.error(
                    if (inactive) "CONTROL_SERVICE_INACTIVE" else "NETWORK_SETTINGS_UNCONFIRMED",
                    if (inactive) {
                        "No live VPN control service is available."
                    } else {
                        "The settings result is not confirmed."
                    },
                    null,
                )
            }
            completeInactivePassiveReads()
        }
        bind()
        flushSettings()
        if (!creating && endpoint == null && (!bindings.isBound || controlServiceKnownInactive())) {
            scheduler.cancel("settings-$id")
            pendingSettings
                .remove(
                    id,
                )?.result
                ?.error("CONTROL_SERVICE_INACTIVE", "No live VPN control service is available.", null)
        }
        reconcileBindings()
    }

    /**
     * An absent control-service process, or a full passive bind deadline
     * without an endpoint while the UI observes, shows that no control service
     * is running. Until an endpoint arrives, a creating bind starts, or UI
     * visibility changes, passive reads use the saved configuration
     * immediately instead of waiting for a binding that cannot connect.
     */
    private fun controlServiceKnownInactive(): Boolean =
        endpoint == null &&
            !bindings.createsService &&
            (controlServiceInactive || runCatching(controlServiceProcessAbsent).getOrDefault(false))

    private fun completeInactivePassiveReads() {
        if (!controlServiceKnownInactive()) return
        pendingSettings.filterValues { it.passive && !it.sent }.keys.forEach { id ->
            scheduler.cancel("settings-$id")
            pendingSettings
                .remove(
                    id,
                )?.result
                ?.error("CONTROL_SERVICE_INACTIVE", "No live VPN control service is available.", null)
        }
    }

    fun requestScenarios(
        json: String,
        result: MethodChannel.Result,
    ) = requestNetworkSettings(json, result, scenario = true)

    private fun flushSettings() {
        val service = endpoint ?: return
        pendingSettings.toMap().forEach { (id, request) ->
            if (!request.sent) {
                request.sent = true
                markSent(request.result)
                val what =
                    if (request.scenario) {
                        UsqueVpnService.MSG_SCENARIOS
                    } else if (request.json ==
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
        pendingSettings.remove(id)?.let { request ->
            val result = request.result
            val parsed =
                json?.let {
                    runCatching {
                        if (request.scenario) ScenarioFields.decode(it) else NetworkSettingsFields.decode(it)
                    }.getOrNull()
                }
            if (parsed == null) {
                val routing = RoutingSettingsError.fromWire(error)
                result.error(
                    routing?.substringBefore(':') ?: error ?: "NETWORK_SETTINGS_UNCONFIRMED",
                    routing ?: "Network settings could not be confirmed.",
                    null,
                )
            } else {
                result.success(parsed)
            }
        }
    }

    private val pendingDiagnosticProbes = mutableMapOf<Int, (SnapshotProbe) -> Unit>()

    private data class NetworkProbeReply(
        val json: String?,
        val cleanupConfirmed: Boolean,
    )

    private var pendingNetworkProbe: Pair<Int, CompletableFuture<NetworkProbeReply>>? = null
    private var pendingTimeline: Pair<Int, (Map<String, Any?>?) -> Unit>? = null
    private var pendingLogs: Pair<Int, (AndroidLogStore.Snapshot?) -> Unit>? = null
    private val pendingClearAll = mutableMapOf<Int, MethodChannel.Result>()
    private var pendingClearAllBind: MethodChannel.Result? = null
    private var nextSnapshotId = 1
    private var liveEndpoint: ControlEndpoint? = null
    private var testEndpoint: ControlEndpoint? = null
    private var endpoint: ControlEndpoint?
        get() = liveEndpoint ?: testEndpoint
        set(value) {
            liveEndpoint = value
            if (value == null) testEndpoint = null
        }

    private var serviceEpoch = 0L
    private var serviceBinderIdentity: IBinder? = null
    private var replyReceiverEpoch = -1L
    private var replyReceiver: ((Int, Int, Bundle) -> Boolean)? = null
    private val serviceResults = mutableSetOf<ServiceResult>()
    private var reconcilingBindings = false
    private var reconcileAgain = false
    private val controlOwners = mutableMapOf<String, ControlOwner>()
    private val jobs = mutableMapOf<String, ServiceJob>()
    private val jobVersions = mutableMapOf<String, Long>()
    private val jobRefreshToken = Any()
    private var jobRefreshInFlight = false
    private var pendingNetworkProbeEpoch: Long? = null
    private var eventsWanted = false
    private var uiVisible = false
    private var eventSubscriptionReachable = false
    private val eventRefreshToken = Any()
    private var eventRefreshGeneration = 0L
    private var pendingDisconnectResult: MethodChannel.Result? = null
    private var pendingRetryResult: MethodChannel.Result? = null
    private var pendingRetryContinuous = false
    private var pendingReconfigure: PendingReconfigure? = null
    private var desiredLocaleCatalog: String? = null
    private var pendingPerAppRevision: Long? = null
    private var controlServiceInactive = false

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
        get() = bindings.isBound

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

    private val bindings: VpnControlBindings =
        VpnControlBindings(
            serviceBinder,
            serviceUnbinder,
            connected = { binder ->
                if (!destroyed) {
                    if (serviceBinderIdentity != null && serviceBinderIdentity !== binder) retireServiceEndpoint()
                    serviceBinderIdentity = binder
                    liveEndpoint = endpointFromBinder(binder, receiverForCurrentService())
                    flushConnectedRequests()
                }
            },
            unavailable = { unexpected, mayRebind ->
                if (!bindings.hasEndpoint) {
                    liveEndpoint = null
                    eventSubscriptionReachable = false
                    if (endpoint == null) networkProbeDisconnected()
                }
                if (unexpected) {
                    retireServiceEndpoint()
                }
                if (unexpected || mayRebind) reconcileBindings()
            },
        )

    private fun retireServiceEndpoint() {
        serviceEpoch++
        serviceBinderIdentity = null
        jobs.clear()
        scheduler.cancel(jobRefreshToken)
        jobRefreshInFlight = false
    }

    private fun receiverForCurrentService(): (Int, Int, Bundle) -> Boolean {
        if (replyReceiverEpoch != serviceEpoch || replyReceiver == null) {
            val epoch = serviceEpoch
            replyReceiverEpoch = epoch
            replyReceiver = { what, id, data ->
                // Reject the old actor before decoding any payload or updating caches.
                if (destroyed || epoch != serviceEpoch) true else onReply(what, id, data)
            }
        }
        return requireNotNull(replyReceiver)
    }

    private inner class ServiceResult(
        val original: MethodChannel.Result,
    ) : MethodChannel.Result {
        val client: VpnControlClient = this@VpnControlClient
        var sentEpoch: Long? = null
        private var released = false
        private var completed = false

        val retainsCurrentService: Boolean
            get() = !released && (sentEpoch == null || sentEpoch == serviceEpoch)

        fun releaseService() {
            if (released) return
            released = true
            serviceResults.remove(this)
            reconcileBindings()
        }

        private fun complete(action: () -> Unit) {
            if (completed) return
            completed = true
            try {
                action()
            } finally {
                releaseService()
            }
        }

        override fun success(result: Any?) = complete { original.success(result) }

        override fun error(
            code: String,
            message: String?,
            details: Any?,
        ) = complete { original.error(code, message, details) }

        override fun notImplemented() = complete { original.notImplemented() }
    }

    private data class ControlOwner(
        var reply: MethodChannel.Result?,
        var epoch: Long? = null,
    )

    private data class ServiceJob(
        val domain: String,
        val id: String?,
        val epoch: Long,
    )

    private fun creatingResult(result: MethodChannel.Result): MethodChannel.Result {
        if (result is ServiceResult && result.client === this) return result
        if (destroyed) return result
        return ServiceResult(result).also {
            serviceResults += it
            reconcileBindings()
        }
    }

    private fun markSent(result: MethodChannel.Result) {
        (result as? ServiceResult)?.sentEpoch = serviceEpoch
    }

    private fun currentReply(result: MethodChannel.Result): Boolean =
        (result as? ServiceResult)?.sentEpoch?.let { it == serviceEpoch } ?: true

    private fun observeJobDispatch(
        domain: String?,
        startsWork: Boolean,
    ): Long {
        if (domain == null) return 0
        if (startsWork) jobVersions[domain] = (jobVersions[domain] ?: 0) + 1
        return jobVersions[domain] ?: 0
    }

    private fun currentJobReply(
        result: MethodChannel.Result,
        domain: String?,
        version: Long,
    ): Boolean = currentReply(result) && (domain == null || version == (jobVersions[domain] ?: 0))

    private fun pendingLiveReads(): Boolean =
        pendingSnapshots.values.any { it !is ServiceResult } ||
            pendingSettings.values.any { it.result !is ServiceResult } ||
            pendingDiagnosticProbes.isNotEmpty() || pendingTimeline != null || pendingLogs != null ||
            (pendingNetworkProbe != null && pendingNetworkProbeEpoch == serviceEpoch)

    private fun reconcileBindings() {
        if (reconcilingBindings) {
            reconcileAgain = true
            return
        }
        reconcilingBindings = true
        try {
            do {
                reconcileAgain = false
                if (destroyed) {
                    bindings.close()
                } else {
                    val work =
                        serviceResults.any { it.retainsCurrentService } ||
                            controlOwners.values.any { it.epoch == null || it.epoch == serviceEpoch } ||
                            jobs.values.any { it.epoch == serviceEpoch }
                    val retainRead = bindings.createsService && pendingLiveReads() && endpoint != null
                    if (work || retainRead) bindings.retainService()
                    if (uiVisible || pendingLiveReads()) bindings.observe() else bindings.releaseObservation()
                    if (!work && !retainRead) bindings.releaseService()
                }
            } while (reconcileAgain)
        } finally {
            reconcilingBindings = false
        }
    }

    private fun flushConnectedRequests() {
        controlServiceInactive = false
        if (eventDeliveryWanted) registerForEvents()
        completeControlOwners()
        pendingDisconnectResult?.let { result ->
            pendingDisconnectResult = null
            scheduler.cancel(disconnectPendingToken(result))
            requestDisconnect(result)
        }
        flushPendingRetry()
        flushPendingReconfigure()
        pendingClearAllBind?.let { result ->
            pendingClearAllBind = null
            scheduler.cancel("clear-all-bind")
            sendClearAllData(result)
        }
        flushSettings()
        flushVpnGate()
        flushProton()
        flushLocale()
        flushPerApp()
        scheduleJobRefresh()
    }

    fun bind() {
        if (!destroyed && (!bindings.createsService || uiVisible || pendingLiveReads())) bindings.observe()
    }

    fun unbind() {
        bindings.releaseObservation()
        if (!bindings.createsService) liveEndpoint = null
        if (endpoint == null) networkProbeDisconnected()
        eventSubscriptionReachable = false
    }

    private fun postDelayed(
        delayMillis: Long,
        token: Any,
        action: () -> Unit,
    ) {
        scheduler.postDelayed(delayMillis, token) {
            try {
                action()
            } finally {
                reconcileBindings()
            }
        }
    }

    fun beginControlOperation(result: MethodChannel.Result) {
        if (destroyed || controlOwners.size >= 64) {
            result.error("ENGINE_IPC_UNAVAILABLE", "The control operation is unavailable.", null)
            return
        }
        val token =
            java.util.UUID
                .randomUUID()
                .toString()
        controlOwners[token] = ControlOwner(result)
        postDelayed(snapshotTimeoutMillis, "control-owner-$token") {
            controlOwners
                .remove(
                    token,
                )?.reply
                ?.error("ENGINE_IPC_TIMEOUT", "The control service did not become ready.", null)
        }
        reconcileBindings()
        completeControlOwners()
    }

    fun endControlOperation(token: String) {
        scheduler.cancel("control-owner-$token")
        controlOwners
            .remove(
                token,
            )?.reply
            ?.error("ENGINE_REQUEST_CANCELLED", "The control operation was cancelled.", null)
        reconcileBindings()
    }

    private fun completeControlOwners() {
        if (endpoint == null) return
        controlOwners.toMap().forEach { (token, owner) ->
            owner.reply?.let { reply ->
                owner.reply = null
                owner.epoch = serviceEpoch
                scheduler.cancel("control-owner-$token")
                reply.success(mapOf("owner_token" to token))
            }
        }
    }

    private fun originalResult(result: MethodChannel.Result?): MethodChannel.Result? =
        (result as? ServiceResult)?.original ?: result

    private fun retireControlOwners() {
        serviceEpoch++
        val retired = controlOwners.toMap()
        controlOwners.clear()
        retired.forEach { (token, owner) ->
            scheduler.cancel("control-owner-$token")
            owner.reply?.error("ENGINE_REQUEST_CANCELLED", "The control owner was retired.", null)
        }
        jobs.clear()
        vpnGateRefreshPending = false
        warpGenerationPending = false
        jobRefreshInFlight = false
        scheduler.cancel(jobRefreshToken)
        reconcileBindings()
    }

    private fun startsProtonJob(command: String): Boolean =
        JSONObject(command).optString("action") in setOf("guest", "login", "refresh", "logout")

    private fun rememberProtonJob(command: String) {
        if (startsProtonJob(command)) {
            jobs.putIfAbsent("proton", ServiceJob("proton", null, serviceEpoch))
            scheduleJobRefresh()
        }
    }

    private fun vpnGateJobDomain(command: String?): String? =
        when (JSONObject(command.orEmpty()).optString("command")) {
            "warp_wireguard" -> "warp"
            "chain_profile" -> null
            else -> "catalogue"
        }

    private fun startsVpnGateJob(command: String?): Boolean {
        val request = JSONObject(command.orEmpty())
        return when (request.optString("command")) {
            "warp_wireguard" -> request.optJSONObject("warp_wireguard")?.optString("action") == "generate"
            "refresh" -> !request.optBoolean("cancel")
            "node" -> request.optString("action") in setOf("prepare", "favorite", "update_favorite")
            else -> false
        }
    }

    private fun rememberVpnGateJob(command: String?) {
        if (!startsVpnGateJob(command)) return
        val domain = vpnGateJobDomain(command) ?: return
        jobs.putIfAbsent(domain, ServiceJob(domain, null, serviceEpoch))
        scheduleJobRefresh()
    }

    private fun updateServiceJob(
        domain: String,
        job: Map<*, *>?,
        error: Any?,
        query: Boolean,
        requestedJobId: String?,
    ) {
        if (query && requestedJobId != null && jobs[domain]?.id != requestedJobId) return
        val currentQuery = query && requestedJobId == null
        val id = job?.get("id") as? String
        val state = job?.get("state")
        if (id == null) {
            if (currentQuery && error == null) jobs.remove(domain)
            scheduleJobRefresh()
            return
        }
        when (state) {
            "running", "waiting_totp", "waiting_verification", "waiting_node" -> {
                jobs[domain] =
                    ServiceJob(domain, id, serviceEpoch)
            }

            "completed", "cancelled", "failed" -> {
                if (currentQuery || jobs[domain]?.id == null ||
                    jobs[domain]?.id == id
                ) {
                    jobs.remove(domain)
                }
            }
        }
        scheduleJobRefresh()
    }

    private var jobPollCursor = 0

    private fun scheduleJobRefresh() {
        scheduler.cancel(jobRefreshToken)
        if (destroyed || jobRefreshInFlight || jobs.isEmpty()) return
        postDelayed(1_000L, jobRefreshToken) {
            if (destroyed || jobRefreshInFlight) return@postDelayed
            val active = jobs.values.filter { it.epoch == serviceEpoch }
            if (active.isEmpty() || endpoint == null) return@postDelayed
            val job = active[Math.floorMod(jobPollCursor++, active.size)]
            val epoch = serviceEpoch
            jobRefreshInFlight = true
            val reply =
                object : MethodChannel.Result {
                    private fun finished() {
                        if (epoch != serviceEpoch) return
                        jobRefreshInFlight = false
                        scheduleJobRefresh()
                    }

                    override fun success(result: Any?) = finished()

                    override fun error(
                        code: String,
                        message: String?,
                        details: Any?,
                    ) = finished()

                    override fun notImplemented() = finished()
                }
            when (job.domain) {
                "proton" -> requestProtonVpn("""{"action":"get"}""", ByteArray(0), reply)
                "warp" -> requestVpnGate("""{"command":"warp_wireguard","warp_wireguard":{"action":"get"}}""", reply)
                else -> requestVpnGate("""{"command":"list","limit":1,"status_only":true}""", reply)
            }
        }
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
        reconcileBindings()
    }

    /** Keep the Dart subscription intent across Activity stops, without queuing background status IPC. */
    fun setUiVisible(visible: Boolean) {
        if (destroyed || uiVisible == visible) return
        uiVisible = visible
        // Inactivity is known only while a visible observation stays retained.
        // A service can start while the UI is stopped without notifying it.
        controlServiceInactive = false
        if (eventDeliveryWanted) {
            bind()
            // The service may have dropped this subscriber while the UI was frozen,
            // even though its Binder connection is still alive. Registration is
            // idempotent and immediately returns the authoritative snapshot.
            registerForEvents()
        } else if (!visible) {
            unregisterForEvents()
        }
        reconcileBindings()
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
            reconcileBindings()
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

        postDelayed(snapshotTimeoutMillis, snapshotTimeoutToken(requestId)) {
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
            reconcileBindings()
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
        postDelayed(minOf(snapshotTimeoutMillis, 750L), snapshotTimeoutToken(requestId)) {
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
        postDelayed(750L, snapshotTimeoutToken(id)) { deliverTimelineReply(id, null) }
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

    /** Capture at the service writer barrier; never block the main thread or poll logs. */
    fun requestLogs(callback: (AndroidLogStore.Snapshot?) -> Unit) {
        val service = endpoint
        if (destroyed || service == null || pendingLogs != null) {
            callback(null)
            return
        }
        val id = allocateRequestId()
        pendingLogs = id to callback
        if (!service.send(UsqueVpnService.MSG_LOG_SNAPSHOT, id)) {
            deliverLogsReply(id, null)
            return
        }
        postDelayed(1_500L, snapshotTimeoutToken(id)) { deliverLogsReply(id, null) }
    }

    internal fun deliverLogsReply(
        id: Int,
        raw: String?,
    ) {
        val pending = pendingLogs?.takeIf { it.first == id } ?: return
        pendingLogs = null
        scheduler.cancel(snapshotTimeoutToken(id))
        val snapshot =
            if (raw == null || raw.length > 384 * 1024 || raw.toByteArray(Charsets.UTF_8).size > 384 * 1024) {
                null
            } else {
                runCatching {
                    val source = JSONObject(raw)
                    val health = source.optJSONObject("health") ?: JSONObject()
                    AndroidLogStore.fromMap(
                        mapOf(
                            "lines" to source.optString("lines"),
                            "health" to
                                health
                                    .keys()
                                    .asSequence()
                                    .take(20)
                                    .associateWith { health.opt(it) },
                        ),
                    )
                }.getOrNull()
            }
        pending.second(snapshot)
    }

    private fun cancelPendingLogs() {
        pendingLogs?.let { (id, callback) ->
            pendingLogs = null
            scheduler.cancel(snapshotTimeoutToken(id))
            callback(null)
        }
    }

    /** Called only on the existing diagnostic worker, never on the UI thread. */
    fun runNetworkProbe(
        checkId: String,
        cancelled: () -> Boolean,
    ): Map<String, Any?> {
        val response = CompletableFuture<NetworkProbeReply>()
        val started = nowNanos()
        scheduler.post {
            val service = endpoint
            if (pendingNetworkProbe != null) {
                response.complete(NetworkProbeReply(null, false))
            } else if (destroyed || cancelled() || service == null || nowNanos() - started >= PROBE_OPERATION_NANOS) {
                response.complete(NetworkProbeReply(null, true))
            } else {
                val id = allocateRequestId()
                pendingNetworkProbe = id to response
                pendingNetworkProbeEpoch = serviceEpoch
                val kind = if (checkId == "transport.h3_path_validation_probe") "h3" else "dns"
                if (!service.send(UsqueVpnService.MSG_DIAGNOSTIC_PROBE, id, mapOf("probe_kind" to kind))) {
                    pendingNetworkProbe = null
                    response.complete(NetworkProbeReply(null, true))
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
            while (nowNanos() - started < PROBE_OPERATION_NANOS && !cancelled()) {
                try {
                    val reply = response.get(50, TimeUnit.MILLISECONDS)
                    return if (reply.cleanupConfirmed) {
                        NetworkDiagnosticChecks.probe(checkId, reply.json)
                    } else {
                        unconfirmedProbeCleanup(checkId)
                    }
                } catch (
                    _: TimeoutException,
                ) {
                    // bounded wait; recheck session cancellation
                }
            }
            cancelPending()
            val cleanupWait = minOf(PROBE_CLEANUP_NANOS, (PROBE_TOTAL_NANOS - (nowNanos() - started)).coerceAtLeast(0))
            val reply =
                try {
                    response.get(cleanupWait, TimeUnit.NANOSECONDS)
                } catch (
                    _: Exception,
                ) {
                    null
                }
            if (reply?.cleanupConfirmed != true) return unconfirmedProbeCleanup(checkId)
            return NetworkDiagnosticChecks.probe(
                checkId,
                if (cancelled()) "{\"code\":\"cancelled\"}" else "{\"code\":\"timeout\"}",
            )
        } catch (_: Exception) {
            cancelPending()
            return unconfirmedProbeCleanup(checkId)
        }
    }

    private fun unconfirmedProbeCleanup(checkId: String): Map<String, Any?> =
        NetworkDiagnosticChecks.result(
            checkId,
            "failed",
            "nq_finding_unavailable",
            "export_diagnostics",
            listOf("probe_cleanup_unconfirmed"),
        ) + ("cleanup_confirmed" to false)

    internal fun deliverNetworkProbeReply(
        id: Int,
        json: String?,
    ) {
        pendingNetworkProbe?.takeIf { it.first == id }?.let { (_, response) ->
            // Accepted service probes reply after cleanup; rejected requests own no probe.
            pendingNetworkProbe = null
            pendingNetworkProbeEpoch = null
            response.complete(NetworkProbeReply(json, true))
            reconcileBindings()
        }
    }

    private fun networkProbeDisconnected() {
        // Losing Binder is not a cleanup acknowledgement. Keep the request gate
        // until its actual reply, or destruction of this client.
        pendingNetworkProbe?.second?.complete(NetworkProbeReply(null, false))
    }

    private fun requestRetryOwned(
        result: MethodChannel.Result,
        continuous: Boolean = false,
    ) {
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
            pendingRetryContinuous = continuous
            bind()
            postDelayed(snapshotTimeoutMillis, result) {
                if (pendingRetryResult === result) {
                    pendingRetryResult = null
                    result.error("ENGINE_IPC_TIMEOUT", "The VPN process did not accept retry in time.", null)
                }
            }
            return
        }
        val requestId = allocateRequestId()
        pendingSnapshots[requestId] = result
        markSent(result)
        if (!service.send(UsqueVpnService.MSG_RETRY, requestId, mapOf("continuous" to continuous))) {
            pendingSnapshots.remove(requestId)
            endpoint = null
            result.error(
                "ENGINE_IPC_UNAVAILABLE",
                "The Android VPN process could not receive the retry request.",
                null,
            )
            return
        }
        postDelayed(snapshotTimeoutMillis, snapshotTimeoutToken(requestId)) {
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
        requestRetry(result, pendingRetryContinuous)
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

    private fun requestReconfigureOwned(
        profileJson: String,
        result: MethodChannel.Result,
        authOnly: Boolean = false,
        accountSelection: Boolean = false,
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
            val previous = pendingReconfigure
            if (accountSelection && previous?.accountSelection == true) {
                pendingReconfigure = null
                scheduler.cancel(reconfigurePendingToken(previous.result))
                // Both selections are durable; only the newest needs delivery.
                previous.result.success(null)
            }
            if (pendingReconfigure != null) {
                result.error(
                    "RECONFIGURE_IN_PROGRESS",
                    "A reconfigure request is already in progress.",
                    null,
                )
                return true
            }
            pendingReconfigure = PendingReconfigure(profileJson, result, authOnly, accountSelection)
            bind()
            val token = reconfigurePendingToken(result)
            postDelayed(reconfigureTimeoutMillis, token) {
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
        markSent(result)
        if (!service.send(
                UsqueVpnService.MSG_RECONFIGURE,
                requestId,
                mapOf(
                    UsqueVpnService.EXTRA_PROFILE_JSON to profileJson,
                    "auth_only" to authOnly,
                    "account_selection" to accountSelection,
                ),
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
        postDelayed(reconfigureTimeoutMillis, snapshotTimeoutToken(requestId)) {
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

    private fun requestDisconnectOwned(result: MethodChannel.Result) {
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
            postDelayed(snapshotTimeoutMillis, token) {
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
        markSent(result)
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

        postDelayed(snapshotTimeoutMillis, snapshotTimeoutToken(requestId)) {
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
        if (pendingClearAll.isNotEmpty() || pendingClearAllBind != null || localWipeActive) {
            result.error(
                "CLEAR_ALL_IN_PROGRESS",
                "Another clear-all operation is already in progress.",
                null,
            )
            return false
        }
        val owned = creatingResult(result)
        if (endpoint == null) {
            pendingClearAllBind = owned
            postDelayed(clearAllTimeoutMillis, "clear-all-bind") {
                pendingClearAllBind?.let {
                    pendingClearAllBind = null
                    it.error("ENGINE_IPC_TIMEOUT", "The control service did not accept clear-all.", null)
                }
            }
            return true
        }
        return sendClearAllData(owned)
    }

    private fun sendClearAllData(result: MethodChannel.Result): Boolean {
        val service = endpoint
        if (service == null) {
            result.error(
                "ENGINE_IPC_UNAVAILABLE",
                "The Android network process is not ready. Try again.",
                null,
            )
            return false
        }
        val requestId = allocateRequestId()
        pendingClearAll[requestId] = result
        markSent(result)
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
        postDelayed(clearAllTimeoutMillis, clearAllTimeoutToken(requestId)) {
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
        cancelPendingConnections("ENGINE_IPC_CLOSED")
        pendingVpnGate.forEach { (id, request) ->
            scheduler.cancel("vpn-gate-$id")
            request.result.error("VPN_GATE_UNAVAILABLE", "The catalogue service was closed.", null)
        }
        pendingVpnGate.clear()
        pendingProton.forEach { (id, request) ->
            scheduler.cancel("proton-$id")
            request.input.fill(0)
            request.result.error("PROTON_UNAVAILABLE", "Proton control was closed.", null)
        }
        pendingProton.clear()
        pendingSettings.forEach { (id, request) ->
            scheduler.cancel("settings-$id")
            request.result.error("NETWORK_SETTINGS_UNCONFIRMED", "The settings result is not confirmed.", null)
        }
        pendingSettings.clear()
        scheduler.cancel("clear-all-bind")
        pendingClearAllBind?.error("CLEAR_ALL_CANCELLED", "The Android UI closed before clearing data.", null)
        pendingClearAllBind = null
        retireControlOwners()

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
            response.complete(NetworkProbeReply(null, false))
        }
        pendingNetworkProbe = null

        pendingTimeline?.let { (id, callback) ->
            pendingTimeline = null
            scheduler.cancel(snapshotTimeoutToken(id))
            callback(null)
        }
        cancelPendingLogs()

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
        bindings.close()
        endpoint = null
        eventListener = null
        clearAllAcknowledgedListener = null
    }

    fun resetAfterClear() {
        cancelPendingConnections()
        cancelPendingLogs()
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
        controlServiceInactive = false
        networkProbeDisconnected()
        lastSnapshot = disconnectedSnapshot()
        // stopSelf alone cannot destroy a service still retained by a creating bind.
        retireControlOwners()
        bindings.close()
    }

    /**
     * Atomically claims an acknowledged clear-all result for the wipe worker. Returns false
     * when destroy already cancelled it or another worker owns the destructive operation.
     */
    fun claimInFlightClearAll(result: MethodChannel.Result): Boolean =
        synchronized(clearAllStateLock) {
            if (destroyed || originalResult(inFlightClearAll) !== originalResult(result) || claimedClearAll != null) {
                return@synchronized false
            }
            claimedClearAll = inFlightClearAll
            inFlightClearAll = null
            true
        }

    /** Releases a worker-owned clear-all result for its one terminal completion. */
    fun takeClaimedClearAll(result: MethodChannel.Result): Boolean =
        synchronized(clearAllStateLock) {
            if (originalResult(claimedClearAll) !== originalResult(result)) return@synchronized false
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
            (clearResult as? ServiceResult)?.releaseService()
            if (errorCode != null) {
                clearResult.error(
                    errorCode,
                    errorMessage ?: "The Android VPN process rejected the operation.",
                    null,
                )
            } else {
                retireControlOwners()
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
        this.testEndpoint = testEndpoint
        flushConnectedRequests()
    }

    fun detachEndpointForTest() {
        endpoint = null
        networkProbeDisconnected()
    }

    fun notifyBindingDiedForTest() {
        bindings.bindingDiedForTest()
    }

    fun pendingSnapshotCountForTest(): Int = pendingSnapshots.size

    fun creatingBindingForTest(): Boolean = bindings.createsService

    fun controlOwnerCountForTest(): Int = controlOwners.size

    fun pendingDiagnosticProbeCountForTest(): Int = pendingDiagnosticProbes.size

    fun pendingClearAllCountForTest(): Int = pendingClearAll.size

    fun pendingDisconnectForTest(): MethodChannel.Result? = originalResult(pendingDisconnectResult)

    fun pendingReconfigureForTest(): MethodChannel.Result? = originalResult(pendingReconfigure?.result)

    fun pendingReconfigureProfileForTest(): String? = pendingReconfigure?.profileJson

    fun inFlightClearAllForTest(): MethodChannel.Result? =
        synchronized(clearAllStateLock) {
            originalResult(inFlightClearAll)
        }

    fun claimedClearAllForTest(): MethodChannel.Result? =
        synchronized(clearAllStateLock) {
            originalResult(claimedClearAll)
        }

    private fun onReply(
        what: Int,
        arg1: Int,
        data: Bundle,
    ): Boolean =
        try {
            dispatchReply(what, arg1, data)
        } finally {
            reconcileBindings()
        }

    private fun dispatchReply(
        what: Int,
        arg1: Int,
        data: Bundle,
    ): Boolean =
        when (what) {
            UsqueVpnService.MSG_PROTON_VPN -> {
                deliverProtonReply(arg1, data)
                true
            }

            UsqueVpnService.MSG_VPN_GATE -> {
                deliverVpnGateReply(arg1, data.getString("vpn_gate_directory"), data.getString("vpn_gate_error"))
                true
            }

            UsqueVpnService.MSG_SAVE_SETTINGS, UsqueVpnService.MSG_GET_SETTINGS, UsqueVpnService.MSG_SCENARIOS -> {
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

            UsqueVpnService.MSG_LOG_SNAPSHOT -> {
                deliverLogsReply(arg1, data.getString("log_snapshot"))
                true
            }

            UsqueVpnService.MSG_DIAGNOSTIC_PROBE -> {
                deliverNetworkProbeReply(arg1, data.getString("probe_result"))
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
        postDelayed(EVENT_REFRESH_INTERVAL_MILLIS, eventRefreshToken) {
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
            requestReconfigure(pending.profileJson, pending.result, pending.authOnly, pending.accountSelection)
        }
    }

    private data class PendingReconfigure(
        val profileJson: String,
        val result: MethodChannel.Result,
        val authOnly: Boolean = false,
        val accountSelection: Boolean = false,
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
                    "cause_code" to bundle.getString(ServiceSnapshotState.WireKeys.FAILURE_CAUSE_CODE),
                    "cause_stage" to bundle.getString(ServiceSnapshotState.WireKeys.FAILURE_CAUSE_STAGE),
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
                "endpoint_source" to bundle.getString(ServiceSnapshotState.WireKeys.ENDPOINT_SOURCE),
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
                "ads_rule_revision" to bundle.getString(ServiceSnapshotState.WireKeys.ADS_RULE_REVISION),
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
                "connection_generation" to
                    if (bundle.containsKey("connection_generation")) bundle.getLong("connection_generation") else null,
                "connection_instance_id" to
                    bundle.getString("connection_instance_id")?.takeIf {
                        it.matches(Regex("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"))
                    },
                "dns_server_count" to bundle.getInt("dns_server_count"),
                "native_runtime_state" to bundle.getString("native_runtime_state"),
                "foreground_notification_state" to
                    bundle.getString("foreground_notification_state"),
                "pending_cleanup" to bundle.getBoolean("pending_cleanup"),
                "platform_state_observed" to true,
                "observed_at_unix_milliseconds" to System.currentTimeMillis(),
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
                    is ByteArray -> bundle.putByteArray(key, value)
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
