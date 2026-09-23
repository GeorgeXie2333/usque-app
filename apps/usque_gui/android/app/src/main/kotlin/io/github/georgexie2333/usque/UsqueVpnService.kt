package io.github.georgexie2333.usque

import android.annotation.SuppressLint
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.net.ConnectivityManager
import android.net.IpPrefix
import android.net.Network
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import android.os.ParcelFileDescriptor
import android.os.RemoteException
import android.os.SystemClock
import android.service.quicksettings.TileService
import androidx.annotation.Keep
import androidx.core.content.ContextCompat
import org.json.JSONObject
import java.io.File
import java.net.Inet6Address
import java.net.InetAddress
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import java.util.concurrent.Future
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

class UsqueVpnService : VpnService() {
    companion object {
        const val ACTION_CONNECT = "io.github.georgexie2333.usque.CONNECT"
        const val ACTION_DISCONNECT = "io.github.georgexie2333.usque.DISCONNECT"
        const val ACTION_CONTROL = "io.github.georgexie2333.usque.CONTROL"
        const val ACTION_CONNECT_LAST = "io.github.georgexie2333.usque.CONNECT_LAST"
        const val ACTION_TOGGLE = "io.github.georgexie2333.usque.TOGGLE"
        private const val ACTION_RETAIN_TILE_CONNECTION =
            "io.github.georgexie2333.usque.RETAIN_TILE_CONNECTION"
        const val EXTRA_PROFILE_JSON = "profile_json"

        const val MSG_SNAPSHOT = 1
        const val MSG_REGISTER_EVENTS = 2
        const val MSG_UNREGISTER_EVENTS = 3
        const val MSG_EVENT = 4

        // 5 was MSG_PAUSE_CAPTIVE_PORTAL (removed).
        const val MSG_CLEAR_ALL_DATA = 6
        const val MSG_DISCONNECT = 7
        const val MSG_TILE_TOGGLE = 8
        const val MSG_RETRY = 9
        const val MSG_RECONFIGURE = 10
        const val MSG_APPLY_PER_APP = 11
        const val MSG_DIAGNOSTIC_PROBE = 12
        const val MSG_CANCEL_DIAGNOSTIC_PROBE = 13
        const val MSG_CONNECTION_TIMELINE = 14
        const val MSG_SAVE_SETTINGS = 15
        const val MSG_GET_SETTINGS = 16
        const val MSG_SETTINGS_EVENT = 17
        const val MSG_UPDATE_LOCALE = 18
        const val MSG_VPN_GATE = 19

        private const val NATIVE_STATUS_INTERVAL_MILLIS = 1_000L
        private const val PHYSICAL_NETWORK_WAIT_MILLIS = 8_000L
        private const val SPLIT_DNS_IPV4 = "198.18.0.1"
        private const val SPLIT_DNS_IPV6 = "fd00::1"
        internal const val RECOVERY_PREFERENCES = "usque_vpn_recovery_v1"
        internal const val RECOVERY_PROFILE = "active_profile_json"
        internal const val LAST_PROFILE = "last_profile_json"
        internal const val START_ON_BOOT = "start_on_boot"
        internal const val TILE_VPN_ACTIVE = "tile_vpn_active"
        private const val MAX_PROFILE_BYTES = 256 * 1024
    }

    private val mainHandler = Handler(Looper.getMainLooper())
    private val engineExecutor =
        Executors.newSingleThreadExecutor { task ->
            Thread(task, "usque-android-engine").apply { isDaemon = true }
        }
    private val stopExecutor =
        Executors.newSingleThreadExecutor { task ->
            Thread(task, "usque-android-stop").apply { isDaemon = true }
        }
    private val statusExecutor =
        Executors.newSingleThreadScheduledExecutor { task ->
            Thread(task, "usque-android-status").apply { isDaemon = true }
        }
    private val connectionGeneration = AtomicLong()
    private val tunnel = AtomicReference<ParcelFileDescriptor?>()
    private val nativeRuntimeActive = AtomicBoolean()
    private val nativeStops = NativeStopTracker()
    private val clearAllRequested = AtomicBoolean()
    private val activeProfileJson = AtomicReference<String?>(null)
    private val settingsExecutor = Executors.newSingleThreadExecutor()
    private val settingsApplication = NetworkSettingsApplicationTracker()
    private var settingsStateJson: String? = null
    private var settingsUncertain = false
    private var runtimeReconfigureInFlight = false
    private var confirmedSettingsProfile: String? = null
    private val settingsPath: String
        get() = File(noBackupFilesDir, "usque_config/profiles-v2.json").absolutePath
    private val activeMode = AtomicReference<String?>(null)
    private val lastTunIdentity = AtomicReference<TunIdentity?>(null)

    @Volatile private var pendingTunRestart: TunRestartDecision = TunRestartDecision.TEARDOWN
    private val eventClients = CopyOnWriteArrayList<Messenger>()
    private val recoveryPreferences by lazy {
        AndroidPolicyStore.recovery(this)
    }
    private val flagCache by lazy { FlagSvgCache(this) }
    private val logStore by lazy { AndroidLogStore(this) }
    private val snapshotState = ServiceSnapshotState()
    private val diagnosticProbes by lazy {
        ServiceDiagnosticProbes(
            this,
            engineExecutor,
            mainHandler,
            profile = { activeProfileJson.get() ?: recoveryPreferences.getString(LAST_PROFILE, null) },
            loadSecret = { id, json -> loadWarpSecret(id, json, readOnly = true) },
            runtimeBusy = {
                activeProfileJson.get() != null || nativeRuntimeActive.get() || tunnel.get() != null ||
                    snapshotState.phase != "disconnected"
            },
            vpnProtected = { tunnel.get() != null },
        )
    }
    private val notifications by lazy { VpnNotificationController(this) }
    private var lastTilePresentation: QuickSettingsTileState.Presentation? = null
    private val networkMonitor =
        PhysicalNetworkMonitor(
            mainHandler = mainHandler,
            listener =
                object : PhysicalNetworkMonitor.Listener {
                    override fun onUnderlyingNetworkChanged(
                        selectedNetwork: Network?,
                        @Suppress("UNUSED_PARAMETER") selectedFamilyMask: Int,
                        generation: Long,
                    ) {
                        // selectedFamilyMask is already stored on PhysicalNetworkMonitor for
                        // JNI getUnderlyingFamilyMask(); reconnect only needs network + generation.
                        handleUnderlyingNetworkChanged(selectedNetwork, generation)
                    }
                },
        )

    @Volatile private var destroyed = false
    private val statusTaskLock = Any()
    private var statusTaskGeneration = 0L
    private var statusTask: ScheduledFuture<*>? = null
    private var gateHandoffGeneration = -1L

    private val controlMessenger =
        Messenger(
            Handler(Looper.getMainLooper()) { message ->
                when (message.what) {
                    MSG_SAVE_SETTINGS, MSG_GET_SETTINGS -> {
                        networkSettingsRequest(Message.obtain(message))
                        true
                    }

                    MSG_SNAPSHOT -> {
                        replyWithSnapshot(message)
                        true
                    }

                    MSG_REGISTER_EVENTS -> {
                        message.replyTo?.let { client ->
                            if (!eventClients.contains(client)) eventClients += client
                            sendEvent(client)
                            settingsStateJson?.let { json ->
                                runCatching {
                                    client.send(
                                        Message.obtain(null, MSG_SETTINGS_EVENT).apply {
                                            data = Bundle().apply { putString("network_settings", json) }
                                        },
                                    )
                                }
                            }
                        }
                        true
                    }

                    MSG_UNREGISTER_EVENTS -> {
                        message.replyTo?.let(eventClients::remove)
                        true
                    }

                    MSG_CLEAR_ALL_DATA -> {
                        clearAllData(message)
                        true
                    }

                    MSG_DISCONNECT -> {
                        disconnect(stopService = true, request = message)
                        true
                    }

                    MSG_TILE_TOGGLE -> {
                        toggleFromTile(message)
                        true
                    }

                    MSG_RETRY -> {
                        retryConnection(message)
                        true
                    }

                    MSG_RECONFIGURE -> {
                        reconfigureConnection(message)
                        true
                    }

                    MSG_APPLY_PER_APP -> {
                        applyPerAppFilter(message)
                        true
                    }

                    MSG_CONNECTION_TIMELINE -> {
                        val raw = NativeEngine.connectionTimeline()
                        val safe = NativeTimelineFields.decode(raw)
                        val reply =
                            Message.obtain(null, MSG_CONNECTION_TIMELINE, message.arg1, 0).apply {
                                data =
                                    Bundle().apply {
                                        if (safe != null) putString("connection_timeline", JSONObject(safe).toString())
                                    }
                            }
                        try {
                            message.replyTo?.send(reply)
                        } catch (_: RemoteException) {
                            // caller gone
                        }
                        true
                    }

                    MSG_DIAGNOSTIC_PROBE -> {
                        diagnosticProbes.start(message)
                        true
                    }

                    MSG_CANCEL_DIAGNOSTIC_PROBE -> {
                        diagnosticProbes.cancel(message.arg1)
                        true
                    }

                    MSG_UPDATE_LOCALE -> {
                        updateLocale(message.data.getString("catalog_id"))
                        true
                    }

                    MSG_VPN_GATE -> {
                        vpnGateCommand(Message.obtain(message))
                        true
                    }

                    else -> {
                        false
                    }
                }
            },
        )

    override fun onCreate() {
        super.onCreate()
        logStore.record(AndroidLogStore.Event.SERVICE_CREATED)
        notifications.createChannel()
        networkMonitor.register(getSystemService(ConnectivityManager::class.java))
    }

    // Called by JNI before permitting ordinary HTTP or temporary WARP egress.
    @Keep
    fun cataloguePhysicalAllowed(): Boolean =
        !destroyed && tunnel.get() == null && !nativeRuntimeActive.get() &&
            !runtimeReconfigureInFlight && snapshotState.phase in setOf("disconnected", "error") &&
            !(Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && isLockdownEnabled)

    private fun vpnGateCommand(request: Message) {
        settingsExecutor.execute {
            var secret = ByteArray(0)
            var response: String? = null
            var errorCode: String? = null
            var warpRequest = false
            try {
                val raw = request.data.getString("vpn_gate_request") ?: error("Missing request")
                require(raw.length <= 256 * 1024)
                val query = JSONObject(raw)
                warpRequest = query.optString("command") == "warp_wireguard"
                require(query.optString("command") == "chain_profile" || raw.length <= 4096)
                if ((
                        query.optString("command") == "warp_wireguard" &&
                            query.optJSONObject("warp_wireguard")?.optString("action") in
                            setOf("generate", "start", "resume")
                    ) ||
                    (query.optString("command") == "refresh" && !query.optBoolean("cancel")) ||
                    (
                        query.optString("command") == "node" &&
                            query.optString("action") in setOf("prepare", "favorite", "update_favorite")
                    )
                ) {
                    val catalog =
                        JSONObject(
                            NativeEngine.applyProfileCommand(settingsPath, """{"command":"list_profiles"}""")
                                ?: error("No account catalog"),
                        )
                    val id = catalog.getString("active_profile_id")
                    val profiles = catalog.getJSONArray("profiles")
                    val profile =
                        (0 until profiles.length()).asSequence().map { profiles.getJSONObject(it) }.firstOrNull {
                            it.optString("id") ==
                                id
                        }
                    if (profile !=
                        null
                    ) {
                        secret = loadWarpSecret(id, profile.toString(), readOnly = true) ?: ByteArray(0)
                    }
                    if (warpRequest && !nativeRuntimeActive.get() && secret.isEmpty()) {
                        error("WARP_IDENTITY_REQUIRED")
                    }
                }
                response = NativeEngine.vpnGate(settingsPath, raw, secret, this)
            } catch (error: Exception) {
                errorCode = if (warpRequest) WarpWireguardFields.failureCode(error.message) else "VPN_GATE_UNAVAILABLE"
            } finally {
                secret.fill(0)
            }
            val reply =
                Message.obtain(null, MSG_VPN_GATE, request.arg1, 0).apply {
                    data =
                        Bundle().apply {
                            putString("vpn_gate_directory", response)
                            putString("vpn_gate_error", errorCode)
                        }
                }
            mainHandler.post { runCatching { request.replyTo?.send(reply) } }
        }
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        refreshLocalizedSurfaces()
    }

    override fun onBind(intent: Intent?): IBinder? =
        if (intent?.action == ACTION_CONTROL) {
            controlMessenger.binder
        } else {
            super.onBind(intent)
        }

    override fun onStartCommand(
        intent: Intent?,
        flags: Int,
        startId: Int,
    ): Int {
        when (intent?.action) {
            ACTION_CONNECT -> {
                beginConnection(intent.getStringExtra(EXTRA_PROFILE_JSON) ?: "{}")
            }

            ACTION_DISCONNECT -> {
                disconnect(stopService = true)
            }

            ACTION_CONNECT_LAST -> {
                connectLastProfile()
            }

            ACTION_TOGGLE -> {
                if (recoveryPreferences.contains(RECOVERY_PROFILE)) {
                    disconnect(stopService = true)
                } else {
                    connectLastProfile()
                }
            }

            ACTION_RETAIN_TILE_CONNECTION -> {
                // Keep the foreground service alive while the tile-triggered
                // connection is handed over to the regular service lifecycle.
            }

            null -> {
                val recoveryProfile = recoveryPreferences.getString(RECOVERY_PROFILE, null)
                val recoveryNeedsVpn =
                    recoveryProfile != null &&
                        runCatching { VpnReconfigure.tunnelFrontendEnabled(recoveryProfile) }
                            .getOrDefault(false)
                if (
                    recoveryProfile != null &&
                    (!recoveryNeedsVpn || VpnService.prepare(this) == null) &&
                    recoveryProfile.toByteArray(Charsets.UTF_8).size <= MAX_PROFILE_BYTES
                ) {
                    beginConnection(recoveryProfile, newSession = false)
                } else {
                    stopSelf()
                    return START_NOT_STICKY
                }
            }
        }
        return START_STICKY
    }

    override fun onRevoke() {
        logStore.record(AndroidLogStore.Event.VPN_PERMISSION_REVOKED, phase = snapshotState.phase)
        disconnect(stopService = true)
        super.onRevoke()
    }

    override fun onDestroy() {
        diagnosticProbes.cancel()
        if (!clearAllRequested.get()) {
            logStore.record(AndroidLogStore.Event.SERVICE_DESTROYED, phase = snapshotState.phase)
        }
        destroyed = true
        networkMonitor.cancelScheduledSelection()
        connectionGeneration.incrementAndGet()
        settingsApplication.cancel()
        networkMonitor.bumpGeneration()
        activeProfileJson.set(null)
        activeMode.set(null)
        nativeRuntimeActive.set(false)
        stopStatusTask()
        eventClients.clear()
        NativeEngine.cancel()
        val descriptor = tunnel.getAndSet(null)
        closeQuietly(descriptor)
        submitNativeStop()
        engineExecutor.shutdownNow()
        settingsExecutor.shutdownNow()
        statusExecutor.shutdownNow()
        stopExecutor.shutdown()
        networkMonitor.unregister(getSystemService(ConnectivityManager::class.java))
        super.onDestroy()
    }

    // Recovery state must be durable before starting the native connection.
    @SuppressLint("ApplySharedPref", "UseKtx")
    private fun beginConnection(
        requestedProfileJson: String,
        desiredProfileJson: String = requestedProfileJson,
        newSession: Boolean = true,
        settingsToken: Long? = null,
    ) {
        val previousGeneration = connectionGeneration.get()
        val continuingSettings =
            settingsToken != null && settingsApplication.owns(settingsToken, previousGeneration) &&
                settingsApplication.phase == NetworkSettingsApplicationTracker.Phase.RECONFIGURING
        if (settingsToken != null && !continuingSettings) return
        var profileJson = requestedProfileJson
        diagnosticProbes.cancel()
        if (profileJson.toByteArray(Charsets.UTF_8).size > MAX_PROFILE_BYTES) {
            startForeground(
                VpnNotificationController.NOTIFICATION_ID,
                notifications.build(AndroidLocaleController.getString(this, R.string.vpn_notif_invalid_profile)),
            )
            snapshotState.reset("error")
            snapshotState.warning = "The VPN profile exceeds the Android safety limit."
            broadcastSnapshot()
            return
        }
        val (mode, tunnelEnabled) =
            try {
                if (newSession) {
                    val configPath = File(noBackupFilesDir, "usque_config/profiles-v2.json").absolutePath
                    val catalog =
                        requireNotNull(NativeEngine.applyProfileCommand(configPath, """{"command":"list_profiles"}"""))
                    profileJson = NetworkSettingsFields.savedProfile(profileJson, catalog)
                }
                val source = JSONObject(profileJson)
                val tunnelEnabled = VpnReconfigure.tunnelFrontendEnabled(source)
                if (tunnelEnabled) {
                    AndroidVpnProfile.parse(profileJson)
                } else {
                    val parsedMode = source.optString("mode")
                    require(parsedMode.isEmpty() || parsedMode in setOf("vpn", "socks5", "httpProxy"))
                }
                VpnReconfigure.canonicalMode(tunnelEnabled) to tunnelEnabled
            } catch (error: Exception) {
                startForeground(
                    VpnNotificationController.NOTIFICATION_ID,
                    notifications.build(
                        AndroidLocaleController.getString(this, R.string.vpn_notif_invalid_network_profile),
                    ),
                )
                snapshotState.reset("error")
                snapshotState.warning = "The network profile is invalid: ${safeMessage(error)}"
                broadcastSnapshot()
                return
            }
        val recoveryProfile = if (settingsApplication.busy && !newSession) confirmedSettingsProfile else profileJson
        if (
            !recoveryPreferences
                .edit()
                .putString(RECOVERY_PROFILE, recoveryProfile)
                .putString(LAST_PROFILE, if (newSession) profileJson else desiredProfileJson)
                .commit()
        ) {
            startForeground(
                VpnNotificationController.NOTIFICATION_ID,
                notifications.build(
                    AndroidLocaleController.getString(this, R.string.vpn_notif_recovery_unavailable),
                ),
            )
            snapshotState.reset("error")
            snapshotState.warning = "Android could not save the non-secret recovery profile."
            broadcastSnapshot()
            return
        }
        val generation = connectionGeneration.incrementAndGet()
        settingsUncertain = false
        runtimeReconfigureInFlight = false
        if (settingsToken != null &&
            settingsApplication.migrateSession(settingsToken, previousGeneration, generation)
        ) {
            settingsExecutor.execute {
                runCatching {
                    NativeEngine.networkSettings(
                        settingsPath,
                        JSONObject()
                            .put("command", "observe")
                            .put("profile", JSONObject.NULL)
                            .put("session_id", generation.toString())
                            .put("applying", true)
                            .toString(),
                    )
                }
            }
        } else {
            settingsApplication.cancel()
        }
        networkMonitor.bumpGeneration()
        activeProfileJson.set(profileJson)
        activeMode.set(mode)
        logStore.record(
            AndroidLogStore.Event.CONNECTION_REQUESTED,
            phase = "preparing",
            mode = mode,
        )
        startForeground(
            VpnNotificationController.NOTIFICATION_ID,
            notifications.build(notifications.copyFor("preparing")),
        )
        snapshotState.reset("preparing")
        notifyTileStateChanged()
        broadcastSnapshot()

        val incomingIdentity =
            if (tunnelEnabled) {
                runCatching {
                    tunIdentity(AndroidVpnProfile.parse(profileJson))
                }.getOrNull()
            } else {
                null
            }
        val decision =
            TunRestartPolicy.decide(
                killSwitch =
                    incomingIdentity != null && (
                        JSONObject(profileJson).optBoolean("kill_switch", false) ||
                            incomingIdentity.vpnGateEnabled || lastTunIdentity.get()?.vpnGateEnabled == true
                    ),
                tunnelFrontend = tunnelEnabled,
                hasCurrentFd = tunnel.get() != null,
                sameIdentity =
                    incomingIdentity != null &&
                        lastTunIdentity.get()?.sameForReuse(incomingIdentity) == true,
                userRequestedDisconnect = false,
            )
        pendingTunRestart = decision

        val staleDescriptor =
            if (decision == TunRestartDecision.TEARDOWN) {
                lastTunIdentity.set(null)
                // Retain Java's protective FD until native stop is confirmed.
                // A timed-out stop must not lose its cleanup owner to GC.
                tunnel.get()
            } else {
                null
            }
        val stopped = submitNativeStop()
        engineExecutor.execute {
            try {
                check(stopped.get(35, TimeUnit.SECONDS)) { "Native stop is unconfirmed" }
                if (staleDescriptor != null && tunnel.compareAndSet(staleDescriptor, null)) {
                    closeQuietly(staleDescriptor)
                }
                if (!isCurrent(generation)) return@execute
                startConnection(generation, profileJson)
            } catch (error: Exception) {
                fail(
                    generation,
                    "The previous tunnel could not be stopped safely (${error.javaClass.simpleName}).",
                )
            }
        }
    }

    private fun retryConnection(request: Message) {
        val generation = connectionGeneration.get()
        engineExecutor.execute {
            val profile =
                runCatching {
                    CurrentAccountProfile.read(
                        requireNotNull(
                            NativeEngine.applyProfileCommand(settingsPath, "{\"command\":\"list_profiles\"}"),
                        ),
                    )
                }.getOrNull()
            mainHandler.post {
                if (!isCurrent(generation)) {
                    replyWithSnapshot(request)
                    return@post
                }
                if (profile == null) {
                    replyControlError(request, "PROFILE_STORE_FAILED", "The current account could not be loaded.")
                } else {
                    beginConnection(profile)
                    replyWithSnapshot(request)
                }
            }
        }
    }

    @SuppressLint("ApplySharedPref", "UseKtx")
    private fun reconfigureConnection(
        request: Message,
        settingsToken: Long? = null,
    ) {
        if (request.data.getBoolean("auth_only", false)) {
            reconfigureProxyAuth(request)
            return
        }
        val settingsRequest = settingsToken != null
        if (settingsApplication.busy && !settingsRequest) {
            replyControlError(request, "NETWORK_SETTINGS_BUSY", "A settings application is in progress.")
            return
        }
        if (settingsToken != null && !settingsApplication.owns(settingsToken, connectionGeneration.get())) return
        val desiredProfileJson = request.data.getString(EXTRA_PROFILE_JSON).orEmpty()
        val profileJson = desiredProfileJson
        if (profileJson.isEmpty() || profileJson.toByteArray(Charsets.UTF_8).size > MAX_PROFILE_BYTES) {
            replyControlError(request, "INVALID_ARGUMENT", "The reconfigure profile is malformed.")
            return
        }
        val mode =
            try {
                val source = JSONObject(profileJson)
                val tunnelEnabled = VpnReconfigure.tunnelFrontendEnabled(source)
                if (tunnelEnabled) {
                    AndroidVpnProfile.parse(profileJson)
                } else {
                    val parsedMode = source.optString("mode")
                    require(parsedMode.isEmpty() || parsedMode in setOf("vpn", "socks5", "httpProxy"))
                }
                VpnReconfigure.canonicalMode(tunnelEnabled)
            } catch (_: Exception) {
                replyControlError(request, "INVALID_PROFILE", "The reconfigure profile is invalid.")
                return
            }
        if (
            !recoveryPreferences
                .edit()
                .putString(RECOVERY_PROFILE, if (settingsRequest) confirmedSettingsProfile else profileJson)
                .putString(LAST_PROFILE, desiredProfileJson)
                .commit()
        ) {
            replyControlError(
                request,
                "RECOVERY_UNAVAILABLE",
                "Android could not save the non-secret recovery profile.",
            )
            return
        }
        // Disconnect bumps this; JNI continuations must not reconnect a stopped session.
        val generation = connectionGeneration.get()
        runtimeReconfigureInFlight = true
        request.data.putLong("runtime_reconfigure_generation", generation)
        activeProfileJson.set(profileJson)
        activeMode.set(mode)

        if (!nativeRuntimeActive.get()) {
            beginConnection(profileJson, desiredProfileJson, newSession = false, settingsToken = settingsToken)
            request.let(::replyWithSnapshot)
            return
        }

        engineExecutor.execute {
            if (!isCurrent(generation)) {
                mainHandler.post { replyWithSnapshot(request) }
                return@execute
            }
            val result = withProxyPassword(profileJson) { NativeEngine.reconfigure(profileJson, it) }
            if (!isCurrent(generation)) {
                mainHandler.post { replyWithSnapshot(request) }
                return@execute
            }
            when (result) {
                NativeEngine.OK -> {
                    mainHandler.post {
                        if (isCurrent(generation)) {
                            VpnReconfigure.applyNativeOk(
                                MSG_RECONFIGURE,
                                profileJson,
                                tunnel,
                                lastTunIdentity,
                                ::closeQuietly,
                            )
                            refreshNativeSnapshot()
                        }
                        replyWithSnapshot(request)
                    }
                }

                NativeEngine.RECONFIGURE_NEED_COLD -> {
                    mainHandler.post {
                        if (isCurrent(generation)) {
                            beginConnection(
                                profileJson,
                                desiredProfileJson,
                                newSession = false,
                                settingsToken = settingsToken,
                            )
                        }
                        replyWithSnapshot(request)
                    }
                }

                NativeEngine.RECONFIGURE_NEED_ATTACH -> {
                    attachTunWhileRunning(generation, profileJson, request)
                }

                else -> {
                    val failure = nativeStartFailure(result)
                    fail(generation, failure.code, failure.message, failure.gateStatus)
                    mainHandler.post { replyWithSnapshot(request) }
                }
            }
        }
    }

    private fun attachTunWhileRunning(
        generation: Long,
        profileJson: String,
        request: Message,
    ) {
        if (!performTunHandoff(generation, profileJson, request) && isCurrent(generation)) {
            // A rejected platform assignment is terminal. Keep the Java TUN
            // blocking and stop the final data plane; retain the WARP session.
            NativeEngine.rejectFinalNetwork()
        }
    }

    private fun performTunHandoff(
        generation: Long,
        profileJson: String,
        request: Message,
    ): Boolean {
        if (!isCurrent(generation)) {
            mainHandler.post { replyWithSnapshot(request) }
            return false
        }
        val profile =
            try {
                AndroidVpnProfile.parse(profileJson)
            } catch (error: Exception) {
                fail(generation, "The VPN profile is invalid: ${safeMessage(error)}")
                mainHandler.post { replyWithSnapshot(request) }
                return false
            }
        val secret =
            try {
                loadWarpSecret(profile.id, profileJson)
            } catch (error: Exception) {
                fail(
                    generation,
                    "IDENTITY_INVALID",
                    "Android Keystore could not read the WARP identity.",
                )
                mainHandler.post { replyWithSnapshot(request) }
                return false
            }
        if (secret == null) {
            fail(generation, "IDENTITY_INVALID", "This profile has no Consumer WARP identity.")
            mainHandler.post { replyWithSnapshot(request) }
            return false
        }
        try {
            val assignment =
                try {
                    inspectAssignment(secret)
                } catch (error: Exception) {
                    fail(
                        generation,
                        "IDENTITY_INVALID",
                        "The stored WARP identity is invalid: ${safeMessage(error)}",
                    )
                    mainHandler.post { replyWithSnapshot(request) }
                    return false
                }
            val routePlan =
                try {
                    planRoutes(profile)
                } catch (error: Exception) {
                    fail(generation, "The bypass route configuration is unsafe: ${safeMessage(error)}")
                    mainHandler.post { replyWithSnapshot(request) }
                    return false
                }
            val descriptor =
                try {
                    val network =
                        if (profile.vpnGateEnabled) {
                            VpnGateNetwork.parse(
                                JSONObject(
                                    NativeEngine.snapshot() ?: error("Missing final network"),
                                ).getJSONObject("final_network"),
                            )
                        } else {
                            null
                        }
                    ensureTun(profile, assignment, routePlan, retainExisting = false, finalNetwork = network)
                } catch (error: PerAppProxyEmptyException) {
                    fail(
                        generation,
                        ANDROID_PER_APP_EMPTY,
                        "No selected apps are still installed for per-app proxy.",
                    )
                    mainHandler.post { replyWithSnapshot(request) }
                    return false
                } catch (error: Exception) {
                    fail(generation, "Android refused the VPN configuration: ${safeMessage(error)}")
                    mainHandler.post { replyWithSnapshot(request) }
                    return false
                }
            if (descriptor == null) {
                fail(generation, "Android refused to create the VPN interface.")
                mainHandler.post { replyWithSnapshot(request) }
                return false
            }
            if (!isCurrent(generation)) {
                tunnel.compareAndSet(descriptor, null)
                closeQuietly(descriptor)
                mainHandler.post { replyWithSnapshot(request) }
                return false
            }
            lastTunIdentity.set(tunIdentity(profile))
            val attached = withProxyPassword(profileJson) { NativeEngine.attachTun(descriptor.fd, profileJson, it) }
            if (attached != NativeEngine.OK) {
                if (!profile.vpnGateEnabled) {
                    tunnel.compareAndSet(descriptor, null)
                    closeQuietly(descriptor)
                    lastTunIdentity.set(null)
                }
                val failure = nativeStartFailure(attached)
                fail(generation, failure.code, failure.message, failure.gateStatus)
                mainHandler.post { replyWithSnapshot(request) }
                return false
            }
            mainHandler.post {
                if (isCurrent(generation)) {
                    snapshotState.killSwitchEnabled = profile.killSwitch
                    refreshNativeSnapshot()
                }
                replyWithSnapshot(request)
            }
            return true
        } finally {
            secret.fill(0)
        }
    }

    private fun connectLastProfile(request: Message? = null) {
        val profileJson = recoveryPreferences.getString(LAST_PROFILE, null)
        if (
            profileJson == null ||
            profileJson.toByteArray(Charsets.UTF_8).size > MAX_PROFILE_BYTES
        ) {
            request?.let {
                replyControlError(
                    it,
                    "TILE_PROFILE_REQUIRED",
                    AndroidLocaleController.getString(this, R.string.tile_profile_required),
                )
            }
            stopSelf()
            return
        }

        val profile = runCatching { JSONObject(profileJson) }.getOrNull()
        val profileId = profile?.optString("id").orEmpty()
        val tunnelEnabled =
            profile != null &&
                runCatching { VpnReconfigure.tunnelFrontendEnabled(profile) }.getOrDefault(false)
        if (profile == null || !tunnelEnabled || profileId.isBlank()) {
            request?.let {
                replyControlError(
                    it,
                    "TILE_VPN_PROFILE_REQUIRED",
                    "The last active profile does not have the Android VPN frontend enabled.",
                )
            }
            stopSelf()
            return
        }

        val hasIdentity =
            runCatching {
                SecureIdentityStore(this)
                    .get(profileId, SecureIdentityStore.Record.WARP_SECRET)
                    ?.let { secret ->
                        val present = secret.isNotEmpty()
                        secret.fill(0)
                        present
                    } ?: false
            }.getOrDefault(false)
        if (!hasIdentity) {
            request?.let {
                replyControlError(
                    it,
                    "TILE_IDENTITY_REQUIRED",
                    AndroidLocaleController.getString(this, R.string.tile_identity_required),
                )
            }
            stopSelf()
            return
        }

        if (VpnService.prepare(this) != null) {
            request?.let {
                replyControlError(
                    it,
                    "TILE_VPN_PERMISSION_REQUIRED",
                    AndroidLocaleController.getString(this, R.string.tile_vpn_permission_required),
                )
            }
            if (request == null) {
                packageManager.getLaunchIntentForPackage(packageName)?.let { launch ->
                    launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                    startActivity(launch)
                }
            }
            stopSelf()
            return
        }
        val retained =
            runCatching {
                ContextCompat.startForegroundService(
                    this,
                    Intent(this, UsqueVpnService::class.java)
                        .setAction(ACTION_RETAIN_TILE_CONNECTION),
                )
            }.isSuccess
        if (!retained) {
            request?.let {
                replyControlError(
                    it,
                    "TILE_START_FAILED",
                    "Android did not allow the VPN service to start from Quick Settings.",
                )
            }
            stopSelf()
            return
        }
        beginConnection(profileJson)
        request?.let(::replyWithSnapshot)
    }

    private fun toggleFromTile(request: Message) {
        val anyFrontendActive = activeProfileJson.get() != null
        val vpnFrontendActive = anyFrontendActive && activeMode.get() == "vpn"
        if (vpnFrontendActive) {
            disconnect(stopService = true, request = request)
        } else if (anyFrontendActive) {
            replyControlError(
                request,
                "TILE_VPN_FRONTEND_INACTIVE",
                AndroidLocaleController.getString(this, R.string.tile_proxy_only),
            )
        } else {
            connectLastProfile(request)
        }
    }

    private fun notifyTileStateChanged() {
        val nextPresentation =
            QuickSettingsTileState.fromSnapshot(
                snapshotState.phase,
                activeProfileJson.get() != null && activeMode.get() == "vpn",
            )
        if (nextPresentation == lastTilePresentation) return
        lastTilePresentation = nextPresentation
        TileService.requestListeningState(
            this,
            android.content.ComponentName(this, UsqueTileService::class.java),
        )
    }

    private fun startConnection(
        generation: Long,
        profileJson: String,
    ) {
        if (!NativeEngine.isReady()) {
            fail(generation, "The Rust data channel is unavailable; no VPN interface was created.")
            return
        }
        val tunnelEnabled =
            try {
                VpnReconfigure.tunnelFrontendEnabled(profileJson)
            } catch (error: Exception) {
                fail(generation, "The VPN profile is invalid: ${safeMessage(error)}")
                return
            }
        activeMode.set(if (tunnelEnabled) "vpn" else "socks5")
        if (!tunnelEnabled) {
            startProxyConnection(generation, profileJson)
            return
        }
        val permissionRequired =
            try {
                VpnService.prepare(this) != null
            } catch (error: Exception) {
                fail(generation, "Android could not verify VPN permission.")
                return
            }
        if (permissionRequired) {
            fail(generation, "VPN permission is not granted.")
            return
        }

        val profile =
            try {
                AndroidVpnProfile.parse(profileJson)
            } catch (error: Exception) {
                fail(generation, "The VPN profile is invalid: ${safeMessage(error)}")
                return
            }
        val secret =
            try {
                loadWarpSecret(profile.id, profileJson)
            } catch (error: Exception) {
                fail(
                    generation,
                    "IDENTITY_INVALID",
                    "Android Keystore could not read the WARP identity.",
                )
                return
            }
        if (secret == null) {
            fail(
                generation,
                "IDENTITY_INVALID",
                "This profile has no Consumer WARP identity.",
            )
            return
        }

        try {
            val assignment =
                try {
                    inspectAssignment(secret)
                } catch (error: Exception) {
                    fail(
                        generation,
                        "IDENTITY_INVALID",
                        "The stored WARP identity is invalid: ${safeMessage(error)}",
                    )
                    return
                }
            val routePlan =
                try {
                    planRoutes(profile)
                } catch (error: Exception) {
                    fail(generation, "The bypass route configuration is unsafe: ${safeMessage(error)}")
                    return
                }
            if (!awaitPhysicalNetwork(generation, requireDns = profile.requiresPhysicalDns)) {
                fail(
                    generation,
                    "ANDROID_WAITING_FOR_PHYSICAL_NETWORK",
                    "Android did not provide a usable non-VPN physical network within 8 seconds.",
                )
                return
            }
            if (!isCurrent(generation)) return
            val restart = pendingTunRestart
            var descriptor =
                try {
                    ensureTun(
                        profile,
                        assignment,
                        routePlan,
                        retainExisting = restart == TunRestartDecision.RETAIN,
                    )
                } catch (error: PerAppProxyEmptyException) {
                    fail(
                        generation,
                        ANDROID_PER_APP_EMPTY,
                        "No selected apps are still installed for per-app proxy.",
                    )
                    return
                } catch (error: Exception) {
                    fail(generation, "Android refused the VPN configuration: ${safeMessage(error)}")
                    return
                }
            if (descriptor == null) {
                fail(generation, "Android refused to create the VPN interface.")
                return
            }
            if (!isCurrent(generation)) {
                if (restart != TunRestartDecision.RETAIN) {
                    tunnel.compareAndSet(descriptor, null)
                    closeQuietly(descriptor)
                }
                return
            }
            lastTunIdentity.set(tunIdentity(profile))
            snapshotState.killSwitchEnabled = profile.killSwitch
            postPhase(generation, "connectingH3", null)
            val proxyPassword = loadProxyPassword(profile.id, profileJson)
            val startResult =
                try {
                    if (profile.vpnGateEnabled) {
                        NativeEngine.startProxy(profileJson, secret, proxyPassword, this)
                    } else {
                        NativeEngine.start(
                            descriptor.fd,
                            profileJson,
                            secret,
                            proxyPassword,
                            this,
                        )
                    }
                } finally {
                    proxyPassword.fill(0)
                }
            if (startResult != NativeEngine.OK) {
                val failure = nativeStartFailure(startResult)
                val splitDnsStartupFailed =
                    failure.code == "ANDROID_SPLIT_DNS_FAILED" ||
                        failure.code == "ANDROID_GEO_RULES_UNAVAILABLE"
                if (!profile.killSwitch || splitDnsStartupFailed) {
                    tunnel.compareAndSet(descriptor, null)
                    closeQuietly(descriptor)
                    lastTunIdentity.set(null)
                }
                fail(generation, failure.code, failure.message, failure.gateStatus)
                return
            }
            if (profile.vpnGateEnabled) {
                val finalDescriptor =
                    try {
                        val native = JSONObject(NativeEngine.snapshot() ?: error("Missing final network"))
                        val network = VpnGateNetwork.parse(native.getJSONObject("final_network"))
                        if (!isCurrent(generation)) {
                            stopNativeRuntime(beginNativeStop())
                            return
                        }
                        establishVpn(profile, assignment, routePlan, network)
                            ?: error("Android refused the final VPN interface")
                    } catch (_: Exception) {
                        stopNativeRuntime(beginNativeStop())
                        fail(generation, "ANDROID_TUN_FAILED", "Could not apply the VPN Gate network configuration.")
                        return
                    }
                // establish() switches Android to the new blocking interface.
                // Keep both FDs until native packet ownership has transferred.
                val previous = tunnel.getAndSet(finalDescriptor)
                descriptor = finalDescriptor
                val attached =
                    withProxyPassword(profileJson) { NativeEngine.attachTun(finalDescriptor.fd, profileJson, it) }
                closeQuietly(previous)
                if (attached != NativeEngine.OK) {
                    stopNativeRuntime(beginNativeStop())
                    val failure = nativeStartFailure(attached)
                    fail(generation, failure.code, failure.message, failure.gateStatus)
                    return
                }
            }
            if (!isCurrent(generation)) {
                stopNativeRuntime(beginNativeStop())
                if (!profile.killSwitch) {
                    tunnel.compareAndSet(descriptor, null)
                    closeQuietly(descriptor)
                    lastTunIdentity.set(null)
                }
                return
            }
            nativeRuntimeActive.set(true)
            mainHandler.post {
                if (isCurrent(generation)) {
                    snapshotState.killSwitchEnabled = profile.killSwitch
                    ensureStatusTask()
                    refreshNativeSnapshot()
                }
            }
        } finally {
            secret.fill(0)
        }
    }

    private fun startProxyConnection(
        generation: Long,
        profileJson: String,
    ) {
        val profileId =
            try {
                JSONObject(profileJson).getString("id")
            } catch (error: Exception) {
                fail(generation, "The proxy profile is invalid: ${safeMessage(error)}")
                return
            }
        val secret =
            try {
                loadWarpSecret(profileId, profileJson)
            } catch (_: Exception) {
                null
            }
        if (secret == null) {
            fail(
                generation,
                "IDENTITY_INVALID",
                "This proxy profile has no Consumer WARP identity.",
            )
            return
        }
        try {
            if (!awaitPhysicalNetwork(generation)) {
                fail(
                    generation,
                    "ANDROID_WAITING_FOR_PHYSICAL_NETWORK",
                    "Android did not provide a usable non-VPN physical network within 8 seconds.",
                )
                return
            }
            postPhase(generation, "connectingH3", null)
            val proxyPassword = loadProxyPassword(profileId, profileJson)
            val result =
                try {
                    NativeEngine.startProxy(profileJson, secret, proxyPassword, this)
                } finally {
                    proxyPassword.fill(0)
                }
            if (result != NativeEngine.OK) {
                val failure = nativeStartFailure(result)
                fail(generation, failure.code, failure.message, failure.gateStatus)
                return
            }
            if (!isCurrent(generation)) {
                stopNativeRuntime(beginNativeStop())
                return
            }
            nativeRuntimeActive.set(true)
            mainHandler.post {
                if (isCurrent(generation)) {
                    snapshotState.killSwitchEnabled = false
                    ensureStatusTask()
                    refreshNativeSnapshot()
                }
            }
        } finally {
            secret.fill(0)
        }
    }

    private fun loadWarpSecret(
        profileId: String,
        profileJson: String,
        readOnly: Boolean = false,
    ): ByteArray? {
        val store = SecureIdentityStore(this)
        val encodedRollback =
            runCatching {
                store.get(
                    profileId,
                    SecureIdentityStore.Record.PENDING_REPLACEMENT_IDENTITY,
                )
            }.getOrNull()
        var current = store.get(profileId, SecureIdentityStore.Record.WARP_SECRET)
        try {
            val authority = identityReplacementAuthority(profileId) ?: return null
            if (!authority.matches(profileJson)) return null
            return when (authority.state) {
                IdentityReplacementState.Preparing -> {
                    current.also { current = null }
                }

                IdentityReplacementState.Armed -> {
                    val rollback =
                        IdentityReplacementRollbackCodec.decode(encodedRollback ?: return null)
                    try {
                        rollback.identity?.copyOf()
                    } finally {
                        rollback.clear()
                    }
                }

                IdentityReplacementState.None -> {
                    if (encodedRollback != null && !readOnly) {
                        runCatching {
                            store.delete(
                                profileId,
                                SecureIdentityStore.Record.PENDING_REPLACEMENT_IDENTITY,
                            )
                        }
                    }
                    current?.fill(0)
                    current = null
                    store.get(profileId, SecureIdentityStore.Record.WARP_SECRET)
                }
            }
        } catch (_: Exception) {
            return null
        } finally {
            current?.fill(0)
            encodedRollback?.fill(0)
        }
    }

    private enum class IdentityReplacementState {
        None,
        Preparing,
        Armed,
    }

    private data class IdentityReplacementAuthority(
        val state: IdentityReplacementState,
        val endpointIpv4: String,
        val endpointIpv6: String,
        val endpointPort: Int,
        val sni: String,
        val endpointReady: Boolean,
    ) {
        fun matches(profileJson: String): Boolean =
            runCatching {
                val profile = JSONObject(profileJson)

                fun numericAddress(value: String): ByteArray? {
                    if (value.isEmpty() ||
                        value.any { character ->
                            !character.isDigit() &&
                                character.lowercaseChar() !in 'a'..'f' &&
                                character != '.' &&
                                character != ':'
                        }
                    ) {
                        return null
                    }
                    return InetAddress.getByName(value).address
                }
                endpointReady &&
                    numericAddress(profile.getString("endpoint_v4"))
                        ?.contentEquals(numericAddress(endpointIpv4) ?: return@runCatching false) == true &&
                    numericAddress(profile.getString("endpoint_v6"))
                        ?.contentEquals(numericAddress(endpointIpv6) ?: return@runCatching false) == true &&
                    profile.getInt("endpoint_port") == endpointPort &&
                    profile.getString("sni").equals(sni, ignoreCase = true)
            }.getOrDefault(false)
    }

    private fun identityReplacementAuthority(profileId: String): IdentityReplacementAuthority? {
        val configPath = File(noBackupFilesDir, "usque_config/profiles-v2.json").absolutePath
        val response =
            NativeEngine.applyProfileCommand(
                configPath,
                """{"command":"list_profiles"}""",
            ) ?: return null
        val catalog = JSONObject(response)
        val profiles = catalog.optJSONArray("profiles") ?: return null
        var profile: JSONObject? = null
        for (index in 0 until profiles.length()) {
            val candidate = profiles.optJSONObject(index) ?: continue
            if (candidate.optString("id") == profileId) {
                profile = candidate
                break
            }
        }
        profile ?: return null
        val pendingValues =
            catalog.optJSONArray("pending_identity_replacements") ?: return null
        var pending = false
        for (index in 0 until pendingValues.length()) {
            if (pendingValues.optString(index) == profileId) {
                pending = true
                break
            }
        }
        val state =
            if (!pending) {
                IdentityReplacementState.None
            } else {
                val armedValues =
                    catalog.optJSONArray("armed_identity_replacements") ?: return null
                var armed = false
                for (index in 0 until armedValues.length()) {
                    if (armedValues.optString(index) == profileId) {
                        armed = true
                        break
                    }
                }
                if (armed) IdentityReplacementState.Armed else IdentityReplacementState.Preparing
            }
        return IdentityReplacementAuthority(
            state = state,
            endpointIpv4 = profile.getString("endpoint_v4"),
            endpointIpv6 = profile.getString("endpoint_v6"),
            endpointPort = profile.getInt("endpoint_port"),
            sni = profile.getString("sni"),
            endpointReady = profile.optBoolean("zero_trust_endpoint_ready", true),
        )
    }

    private fun inspectAssignment(secret: ByteArray): WarpAddressAssignment {
        val metadata =
            NativeEngine.inspectWarpSecret(secret)
                ?: throw IllegalArgumentException("identity metadata is unavailable")
        return WarpAddressAssignment.parse(metadata)
    }

    private fun planRoutes(profile: AndroidVpnProfile): RoutePlan =
        VpnRoutePlanner.plan(
            includeIpv4 = profile.includeIpv4,
            includeIpv6 = profile.includeIpv6,
            allowLan = profile.allowLan,
            bypassCidrs = profile.bypassCidrs,
            supportsRouteExclusion = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU,
        )

    private fun ensureTun(
        profile: AndroidVpnProfile,
        assignment: WarpAddressAssignment,
        routePlan: RoutePlan,
        retainExisting: Boolean,
        finalNetwork: VpnGateNetwork? = null,
    ): ParcelFileDescriptor? {
        val existing = tunnel.get()
        if (retainExisting && existing != null) {
            return existing
        }
        val created = establishVpn(profile, assignment, routePlan, finalNetwork) ?: return null
        val previous = tunnel.getAndSet(created)
        if (previous != null && previous !== created) {
            closeQuietly(previous)
        }
        return created
    }

    private fun establishVpn(
        profile: AndroidVpnProfile,
        assignment: WarpAddressAssignment,
        routePlan: RoutePlan,
        finalNetwork: VpnGateNetwork? = null,
    ): ParcelFileDescriptor? {
        val builder =
            Builder()
                .setSession(profile.name)
                .setMtu(finalNetwork?.mtu ?: profile.mtu)
                .setBlocking(false)
        networkMonitor.underlyingNetwork()?.let { network ->
            builder.setUnderlyingNetworks(arrayOf(network))
        }
        if (finalNetwork != null) {
            finalNetwork.ipv4?.let { builder.addAddress(it, 32) }
            finalNetwork.ipv6?.let { builder.addAddress(it, 128) }
        } else {
            if (profile.includeIpv4) builder.addAddress(assignment.ipv4, 32)
            if (profile.includeIpv6) builder.addAddress(assignment.ipv6, 128)
        }
        val advertisedDns =
            if (profile.splitDnsEnabled) {
                listOf(
                    InetAddress.getByName(SPLIT_DNS_IPV4),
                    InetAddress.getByName(SPLIT_DNS_IPV6),
                ).filter {
                    finalNetwork == null ||
                        if (it is java.net.Inet4Address) finalNetwork.ipv4 != null else finalNetwork.ipv6 != null
                }
            } else {
                if (profile.dnsMode == "tunnel") finalNetwork?.dns ?: profile.dnsServers else profile.dnsServers
            }
        advertisedDns.forEach(builder::addDnsServer)
        routePlan.included.forEach { route ->
            builder.addRoute(route.address, route.prefixLength)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            routePlan.excluded.forEach { route ->
                builder.excludeRoute(IpPrefix(route.address, route.prefixLength))
            }
        }
        if (profile.splitDnsEnabled) {
            // Exact routes keep the in-process DNS listener inside the TUN even
            // when fd00::/8 is otherwise excluded by Allow LAN.
            builder.addRoute(InetAddress.getByName(SPLIT_DNS_IPV4), 32)
            builder.addRoute(InetAddress.getByName(SPLIT_DNS_IPV6), 128)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            builder.setMetered(false)
        }
        when (val plan = currentPerAppPlan()) {
            PerAppPlan.None -> {}

            PerAppPlan.Empty -> {
                throw PerAppProxyEmptyException()
            }

            is PerAppPlan.Allow -> {
                var added = 0
                for (allowed in plan.packages) {
                    try {
                        builder.addAllowedApplication(allowed)
                        added += 1
                    } catch (_: PackageManager.NameNotFoundException) {
                        // Uninstalled between planning and establish; skip.
                    }
                }
                if (added == 0) {
                    throw PerAppProxyEmptyException()
                }
            }
        }
        return builder.establish()
    }

    private fun tunIdentity(profile: AndroidVpnProfile): TunIdentity =
        TunIdentity.from(profile, PerAppProxyStore.load(this))

    private fun currentPerAppPlan(): PerAppPlan =
        PerAppProxyApplier.plan(
            settings = PerAppProxyStore.load(this),
            isInstalled = ::packageInstalled,
            selfPackage = packageName,
        )

    private fun packageInstalled(packageName: String): Boolean =
        try {
            packageManager.getApplicationInfo(packageName, 0)
            true
        } catch (_: PackageManager.NameNotFoundException) {
            false
        }

    private fun applyPerAppFilter(request: Message) {
        try {
            check(PerAppProxyStore.preferences(this).revision() >= request.data.getLong("revision", 0L))
            PerAppProxyStore.load(this)
        } catch (_: Exception) {
            replyControlError(request, "PER_APP_STORE_FAILED", "Android could not read the committed per-app policy.")
            return
        }
        val profileJson = activeProfileJson.get()
        val tunnelOn =
            profileJson != null &&
                runCatching { VpnReconfigure.tunnelFrontendEnabled(profileJson) }
                    .getOrDefault(false)
        if (profileJson.isNullOrEmpty() || !nativeRuntimeActive.get() || !tunnelOn) {
            request.let(::replyWithSnapshot)
            return
        }
        beginConnection(
            profileJson,
            recoveryPreferences.getString(LAST_PROFILE, null) ?: profileJson,
            newSession = false,
        )
        request.let(::replyWithSnapshot)
    }

    // Remove recovery state before the service can be stopped.
    @SuppressLint("ApplySharedPref", "UseKtx")
    private fun disconnect(
        stopService: Boolean,
        request: Message? = null,
        terminalFailure: ConnectionFailure? = null,
    ) {
        diagnosticProbes.cancel()
        recoveryPreferences.edit().remove(RECOVERY_PROFILE).commit()
        lastTunIdentity.set(null)
        pendingTunRestart = TunRestartDecision.TEARDOWN
        val generation = connectionGeneration.incrementAndGet()
        settingsApplication.cancel()
        runtimeReconfigureInFlight = false
        networkMonitor.bumpGeneration()
        activeProfileJson.set(null)
        val stoppedMode = activeMode.getAndSet(null)
        nativeRuntimeActive.set(false)
        val stopTicket = beginNativeStop()
        stopStatusTask()
        NativeEngine.cancel()
        val descriptor = tunnel.getAndSet(null)
        closeQuietly(descriptor)
        snapshotState.resetForDisconnect(terminalFailure)
        notifyTileStateChanged()
        logStore.record(
            AndroidLogStore.Event.CONNECTION_STOPPED,
            phase = snapshotState.phase,
            mode = stoppedMode,
        )
        broadcastSnapshot()
        request?.let(::replyWithSnapshot)
        stopForeground(STOP_FOREGROUND_REMOVE)

        // Joining the native Tokio thread is cleanup, not part of the user
        // visible disconnect. Java's TUN handle and the cancellation gate are
        // closed; native duplicate-FD and worker completion are tracked below.
        submitNativeStop(stopTicket) { confirmed ->
            mainHandler.post {
                if (connectionGeneration.get() == generation) {
                    broadcastSnapshot()
                    if (confirmed && stopService) stopSelf()
                    if (!confirmed) {
                        val cleanupWarning = "Native cleanup is not confirmed. Retry before reconnecting."
                        if (terminalFailure == null) {
                            fail(generation, cleanupWarning)
                        } else {
                            // Keep the original error and pending-cleanup evidence.
                            snapshotState.warning = "${terminalFailure.message.take(384)}\n$cleanupWarning"
                            broadcastSnapshot()
                        }
                    }
                }
            }
        }
    }

    // Clear recovery state before acknowledging the destructive request.
    @SuppressLint("ApplySharedPref", "UseKtx")
    private fun clearAllData(request: Message) {
        if (!request.data.getBoolean("confirmed", false)) {
            replyControlError(
                request,
                "CONFIRMATION_REQUIRED",
                "Clear All Data requires an explicit confirmation.",
            )
            return
        }
        clearAllRequested.set(true)
        try {
            AndroidPolicyStore.clear(this)
        } catch (_: Exception) {
            clearAllRequested.set(false)
            replyControlError(request, "POLICY_CLEAR_FAILED", "Android could not clear persisted policy.")
            return
        }
        AndroidLocaleController.clear(this)
        val generation = connectionGeneration.incrementAndGet()
        settingsApplication.cancel()
        networkMonitor.bumpGeneration()
        activeProfileJson.set(null)
        activeMode.set(null)
        nativeRuntimeActive.set(false)
        val stopTicket = beginNativeStop()
        stopStatusTask()
        snapshotState.phase = "disconnecting"
        snapshotState.warning = null
        notifyTileStateChanged()
        broadcastSnapshot()
        NativeEngine.cancel()
        val descriptor = tunnel.getAndSet(null)
        closeQuietly(descriptor)
        submitNativeStop(stopTicket) { confirmed ->
            mainHandler.post {
                if (connectionGeneration.get() == generation) {
                    if (!confirmed) {
                        replyControlError(request, "NATIVE_STOP_UNCONFIRMED", "Native cleanup has not completed.")
                        fail(generation, "Native cleanup has not completed. Retry stopping before clearing data.")
                        return@post
                    }
                    // A barrier puts the final wipe after every older settings
                    // write. Requests arriving during clear are rejected below.
                    settingsExecutor.execute {
                        val reset =
                            runCatching {
                                requireNotNull(NativeEngine.networkSettings(settingsPath, "{\"command\":\"reset\"}"))
                            }
                        mainHandler.post resetDone@{
                            if (!isCurrent(generation)) return@resetDone
                            if (reset.isFailure) {
                                replyControlError(request, "CLEAR_ALL_FAILED", "Network settings could not be reset.")
                                return@resetDone
                            }
                            settingsStateJson = null
                            confirmedSettingsProfile = null
                            settingsUncertain = false
                            runtimeReconfigureInFlight = false
                            diagnosticProbes.cancel()
                            snapshotState.reset("disconnected")
                            notifyTileStateChanged()
                            broadcastSnapshot()
                            replyWithSnapshot(request)
                            stopForeground(STOP_FOREGROUND_REMOVE)
                            stopSelf()
                        }
                    }
                }
            }
        }
    }

    private fun beginNativeStop(): Long {
        val ticket = nativeStops.begin()
        logStore.record(AndroidLogStore.Event.NATIVE_STOP_REQUESTED)
        return ticket
    }

    private fun stopNativeRuntime(ticket: Long): Boolean {
        val confirmed = NativeEngine.stop()
        nativeStops.complete(ticket, confirmed)
        logStore.record(
            if (confirmed) {
                AndroidLogStore.Event.NATIVE_STOP_COMPLETED
            } else {
                AndroidLogStore.Event.NATIVE_STOP_UNCONFIRMED
            },
        )
        return confirmed
    }

    private fun submitNativeStop(
        ticket: Long = beginNativeStop(),
        completed: (Boolean) -> Unit = {},
    ): Future<Boolean> =
        try {
            stopExecutor.submit<Boolean> {
                val confirmed = stopNativeRuntime(ticket)
                completed(confirmed)
                confirmed
            }
        } catch (error: java.util.concurrent.RejectedExecutionException) {
            nativeStops.complete(ticket, false)
            throw error
        }

    private fun handleUnderlyingNetworkChanged(
        selectedNetwork: Network?,
        generation: Long,
    ) {
        NativeEngine.notifyNetworkChanged(generation)
        logStore.record(
            AndroidLogStore.Event.NETWORK_CHANGED,
            phase = snapshotState.phase,
            mode = activeMode.get(),
        )

        if (nativeRuntimeActive.get() || tunnel.get() != null) {
            if (tunnel.get() != null) {
                setUnderlyingNetworks(
                    selectedNetwork?.let { arrayOf(it) } ?: emptyArray(),
                )
            }
            mainHandler.post {
                if (nativeRuntimeActive.get() || tunnel.get() != null) {
                    snapshotState.noteUnderlyingNetworkChange(selectedNetwork != null)
                    updateNotification()
                    notifyTileStateChanged()
                    broadcastSnapshot()
                    ensureStatusTask()
                }
            }
        }
    }

    private fun awaitPhysicalNetwork(
        connectionToken: Long,
        requireDns: Boolean = false,
    ): Boolean =
        networkMonitor.awaitPhysicalNetwork(
            isCurrent = { isCurrent(connectionToken) },
            waitMillis = PHYSICAL_NETWORK_WAIT_MILLIS,
            requireDns = requireDns,
        )

    private fun ensureStatusTask() {
        synchronized(statusTaskLock) {
            if (destroyed || statusTask?.isDone == false) return
            val generation = ++statusTaskGeneration
            val cadence = StatusSamplingCadence(NATIVE_STATUS_INTERVAL_MILLIS)

            fun scheduleNext(delayMillis: Long) {
                statusTask =
                    statusExecutor.schedule(
                        {
                            val current =
                                synchronized(statusTaskLock) {
                                    !destroyed && generation == statusTaskGeneration
                                }
                            if (current) {
                                if (cadence.takeDue(SystemClock.elapsedRealtime())) {
                                    refreshNativeSnapshotInBackground()
                                }
                                synchronized(statusTaskLock) {
                                    if (!destroyed && generation == statusTaskGeneration) {
                                        scheduleNext(cadence.delayUntilNext(SystemClock.elapsedRealtime()))
                                    }
                                }
                            }
                        },
                        delayMillis,
                        TimeUnit.MILLISECONDS,
                    )
            }
            scheduleNext(0)
        }
    }

    private fun stopStatusTask() {
        synchronized(statusTaskLock) {
            // A task already inside JNI may finish, but cannot queue another
            // beat after disconnect/destroy or replace a newer task's future.
            statusTaskGeneration++
            statusTask?.cancel(false)
            statusTask = null
        }
    }

    private fun networkSettingsRequest(request: Message) {
        if (clearAllRequested.get()) {
            replySettings(request, null, "NETWORK_SETTINGS_UNCONFIRMED")
            return
        }
        val generation = connectionGeneration.get()
        settingsApplication.cancelIfStale(generation)
        val phase = snapshotState.phase
        val available =
            !destroyed && !settingsApplication.busy && !settingsUncertain && !runtimeReconfigureInFlight &&
                nativeRuntimeActive.get() && phase in setOf("connected", "degraded")
        val saving = request.what == MSG_SAVE_SETTINGS
        val token = if (saving && available) settingsApplication.begin(generation) else null
        settingsExecutor.execute {
            val outcome =
                runCatching {
                    val command =
                        if (saving) {
                            JSONObject(request.data.getString("settings_request").orEmpty()).apply {
                                put("command", "save")
                                put("phase", phase)
                                put("available", available)
                                put("session_id", generation.toString())
                            }
                        } else {
                            JSONObject().put("command", "get")
                        }
                    JSONObject(requireNotNull(NativeEngine.networkSettings(settingsPath, command.toString())))
                }
            mainHandler.post {
                val source = outcome.getOrNull()
                if (source == null) {
                    if (token != null) settingsApplication.finish(token, generation)
                    val rejected = outcome.exceptionOrNull()?.message?.contains("NETWORK_SETTINGS_SAVE_FAILED") == true
                    replySettings(
                        request,
                        null,
                        if (rejected) "NETWORK_SETTINGS_SAVE_FAILED" else "NETWORK_SETTINGS_UNCONFIRMED",
                    )
                    return@post
                }
                val target = source.optJSONObject("target")?.toString()
                source.remove("target")
                val json = source.toString()
                if (isCurrent(generation) && (token == null || settingsApplication.owns(token, generation))) {
                    publishSettings(json)
                }
                // Session cancellation retires application ownership, not the
                // durable acknowledgement owed to this save's caller.
                replySettings(request, json, null)
                if (token == null || !isCurrent(generation) || !settingsApplication.owns(token, generation)) return@post
                if (target == null || snapshotState.phase !in setOf("connected", "degraded")) {
                    settingsApplication.finish(token, generation)
                    observeNetworkSettings()
                    return@post
                }
                val operation = source.getString("operation_id")
                if (!settingsApplication.committed(token, generation, operation)) return@post
                val reply =
                    Messenger(
                        Handler(Looper.getMainLooper()) { replyMessage ->
                            val currentGeneration = connectionGeneration.get()
                            if (!isCurrent(currentGeneration)) return@Handler true
                            val application =
                                settingsApplication.runtimeReplied(token, currentGeneration) ?: return@Handler true
                            if (replyMessage.data.getString("control_error_code") != null) {
                                val failedGeneration = application.generation
                                settingsUncertain = true
                                settingsApplication.finish(token, failedGeneration)
                                settingsExecutor.execute {
                                    val json =
                                        runCatching {
                                            NativeEngine.networkSettings(
                                                settingsPath,
                                                JSONObject()
                                                    .put("command", "failed")
                                                    .put("operation_id", application.operationId)
                                                    .put("session_id", failedGeneration.toString())
                                                    .toString(),
                                            )
                                        }.getOrNull()
                                    mainHandler.post {
                                        if (isCurrent(failedGeneration) && settingsApplication.isLatest(token) &&
                                            json != null
                                        ) {
                                            publishSettings(json)
                                        }
                                    }
                                }
                                return@Handler true
                            }
                            observeNetworkSettings()
                            refreshNativeSnapshot()
                            true
                        },
                    )
                reconfigureConnection(
                    Message.obtain(null, MSG_RECONFIGURE).apply {
                        replyTo = reply
                        data = Bundle().apply { putString(EXTRA_PROFILE_JSON, target) }
                    },
                    settingsToken = token,
                )
            }
        }
    }

    // The synchronous commit result is part of the recovery confirmation.
    @SuppressLint("ApplySharedPref", "UseKtx")
    private fun observeNetworkSettings() {
        if (destroyed) return
        val generation = connectionGeneration.get()
        settingsApplication.cancelIfStale(generation)
        if (!settingsApplication.allowsObservation(generation)) return
        if (runtimeReconfigureInFlight) return
        if (settingsUncertain) return
        val profile = activeProfileJson.get()
        val stable =
            snapshotState.phase in setOf("connected", "degraded") &&
                nativeRuntimeActive.get() && profile != null &&
                (!VpnReconfigure.tunnelFrontendEnabled(profile) || tunnel.get()?.fileDescriptor?.valid() == true)
        val application = settingsApplication.current(generation)
        val failed = application != null && snapshotState.phase == "error"
        if (application != null && !stable && !failed) return
        val command =
            if (failed) {
                settingsUncertain = true
                val requestedGate = profile?.let { ChainProfileFields.selection(JSONObject(it)) }
                val confirmedGate =
                    confirmedSettingsProfile?.let {
                        ChainProfileFields.selection(JSONObject(it))
                    }
                if (requestedGate == confirmedGate) activeProfileJson.set(confirmedSettingsProfile)
                JSONObject()
                    .put("command", "failed")
                    .put("operation_id", application.operationId)
                    .put("session_id", generation.toString())
            } else {
                if (stable && profile != confirmedSettingsProfile) {
                    if (!recoveryPreferences.edit().putString(RECOVERY_PROFILE, profile).commit()) {
                        settingsUncertain = true
                    } else {
                        confirmedSettingsProfile = profile
                    }
                }
                JSONObject()
                    .put("command", "observe")
                    .put("profile", if (stable) JSONObject(profile) else JSONObject.NULL)
                    .put("session_id", generation.toString())
                    .put("applying", false)
                    .put("unconfirmed", settingsUncertain)
            }
        if (application != null) settingsApplication.finish(application.token, generation)
        settingsExecutor.execute {
            val json = runCatching { NativeEngine.networkSettings(settingsPath, command.toString()) }.getOrNull()
            mainHandler.post {
                if (isCurrent(generation) && (application == null || settingsApplication.isLatest(application.token)) &&
                    json != null
                ) {
                    publishSettings(json)
                }
            }
        }
    }

    private fun publishSettings(json: String) {
        settingsStateJson = json
        eventClients.toList().forEach { client ->
            runCatching {
                client.send(
                    Message.obtain(null, MSG_SETTINGS_EVENT).apply {
                        data = Bundle().apply { putString("network_settings", json) }
                    },
                )
            }
        }
    }

    private fun replySettings(
        request: Message,
        json: String?,
        error: String?,
    ) {
        runCatching {
            request.replyTo?.send(
                Message.obtain(null, request.what, request.arg1, 0).apply {
                    data =
                        Bundle().apply {
                            putString("network_settings", json)
                            putString("settings_error", error)
                        }
                },
            )
        }
    }

    private fun refreshNativeSnapshot() {
        if (destroyed || !nativeRuntimeActive.get()) return
        statusExecutor.execute(::refreshNativeSnapshotInBackground)
    }

    private fun refreshNativeSnapshotInBackground() {
        if (destroyed || !nativeRuntimeActive.get()) return
        val generation = connectionGeneration.get()
        val source =
            try {
                JSONObject(NativeEngine.snapshot() ?: return)
            } catch (_: Exception) {
                return
            }
        mainHandler.post {
            if (isCurrent(generation) && nativeRuntimeActive.get()) {
                applyNativeSnapshot(source)
            }
        }
    }

    private fun applyNativeSnapshot(source: JSONObject) {
        val merge = snapshotState.applyNativeSnapshot(source)
        val gate = source.optJSONObject("vpn_gate")
        val gateGeneration = gate?.optLong("generation", -1L) ?: -1L
        if (gate?.optString("stage") == "configuring_network" &&
            source.optJSONObject("final_network") != null &&
            !runtimeReconfigureInFlight && gateGeneration != gateHandoffGeneration
        ) {
            val profileJson = activeProfileJson.get()
            val profile = profileJson?.let { runCatching { AndroidVpnProfile.parse(it) }.getOrNull() }
            if (profile?.vpnGateEnabled == true && VpnReconfigure.tunnelFrontendEnabled(JSONObject(profileJson))) {
                gateHandoffGeneration = gateGeneration
                val generation = connectionGeneration.get()
                runtimeReconfigureInFlight = true
                val request =
                    Message.obtain(null, MSG_RECONFIGURE).apply {
                        data = Bundle().apply { putLong("runtime_reconfigure_generation", generation) }
                    }
                engineExecutor.execute {
                    if (isCurrent(generation)) {
                        attachTunWhileRunning(generation, profileJson, request)
                    } else {
                        mainHandler.post { replyWithSnapshot(request) }
                    }
                }
            }
        }
        observeNetworkSettings()
        merge.cacheWrite?.let { write ->
            statusExecutor.execute {
                try {
                    flagCache.put(write.countryCode, write.svg)
                } catch (_: Exception) {
                    // A cache write failure is diagnostic-only.
                }
            }
        }
        merge.cacheLookupCountryCode?.let { countryCode ->
            statusExecutor.execute {
                val cached = flagCache.get(countryCode)
                if (cached != null) {
                    mainHandler.post {
                        if (
                            snapshotState.exitCountryCode == countryCode &&
                            snapshotState.exitFlagSvg == null
                        ) {
                            snapshotState.exitFlagSvg = cached
                            broadcastSnapshot()
                        }
                    }
                }
            }
        }
        if (merge.enteredError) {
            if (
                disconnectFailedVpnGate(
                    snapshotState.errorCode ?: "ANDROID_RUNTIME_FAILED",
                    snapshotState.warning ?: "The VPN Gate connection failed.",
                    VpnGateFields.stoppedStatus(gate),
                )
            ) {
                return
            }
            VpnGateFields.stoppedStatus(gate)?.let { snapshotState.vpnGateJson = it }
            // Keep the TUN open and fail closed until the user retries or disconnects.
            stopStatusTask()
        }
        if (merge.phaseChanged) {
            logStore.record(
                AndroidLogStore.Event.CONNECTION_PHASE_CHANGED,
                phase = snapshotState.phase,
                mode = activeMode.get(),
                transport = snapshotState.transport,
            )
            updateNotification()
            notifyTileStateChanged()
        }
        broadcastSnapshot()
    }

    private fun postPhase(
        generation: Long,
        nextPhase: String,
        nextWarning: String?,
    ) {
        mainHandler.post {
            if (isCurrent(generation)) {
                val phaseChanged = snapshotState.phase != nextPhase
                snapshotState.phase = nextPhase
                snapshotState.warning = nextWarning
                snapshotState.errorCode = null
                logStore.record(
                    AndroidLogStore.Event.CONNECTION_PHASE_CHANGED,
                    phase = snapshotState.phase,
                    mode = activeMode.get(),
                    transport = snapshotState.transport,
                )
                updateNotification()
                if (phaseChanged) notifyTileStateChanged()
                broadcastSnapshot()
            }
        }
    }

    /** Called on the main thread only after a generation-checked terminal failure. */
    private fun disconnectFailedVpnGate(
        code: String,
        message: String,
        gateStatus: String?,
    ): Boolean {
        val requestedGate =
            activeProfileJson.get()?.let { profile ->
                runCatching { ChainProfileFields.enabled(JSONObject(profile)) }
                    .getOrDefault(false)
            } == true
        if (!requestedGate) return false
        val source = (gateStatus ?: snapshotState.vpnGateJson)?.let { runCatching { JSONObject(it) }.getOrNull() }
        val stoppedGate =
            VpnGateFields.stoppedStatus(source)
                ?: VpnGateFields.stoppedStatus(JSONObject().put("stage", "error"))
        logStore.record(
            AndroidLogStore.Event.CONNECTION_FAILED,
            phase = "error",
            mode = activeMode.get(),
        )
        // End this connection intent, including its recovery record and Java
        // TUN. Cancellation precedes FD closure; native duplicate-FD cleanup
        // remains tracked by the same stop owner as an explicit Disconnect.
        disconnect(
            stopService = true,
            terminalFailure = ConnectionFailure(code, message, stoppedGate, snapshotState.failure),
        )
        return true
    }

    private fun fail(
        generation: Long,
        message: String,
    ) {
        fail(generation, "ANDROID_RUNTIME_FAILED", message)
    }

    private fun fail(
        generation: Long,
        code: String,
        message: String,
        gateStatus: String? = null,
    ) {
        mainHandler.post {
            if (!isCurrent(generation)) return@post
            if (disconnectFailedVpnGate(code, message, gateStatus)) return@post
            nativeRuntimeActive.set(false)
            snapshotState.phase = "error"
            snapshotState.errorCode = code
            if (gateStatus != null) {
                snapshotState.vpnGateJson = gateStatus
                snapshotState.activeFrontends = emptyList()
                snapshotState.activeListeners = emptyList()
                snapshotState.tunnelIpv4Available = false
                snapshotState.tunnelIpv6Available = false
            }
            logStore.record(
                AndroidLogStore.Event.CONNECTION_FAILED,
                phase = snapshotState.phase,
                mode = activeMode.get(),
            )
            snapshotState.warning = message.take(512)
            snapshotState.transport = null
            snapshotState.addressFamily = null
            updateNotification()
            notifyTileStateChanged()
            broadcastSnapshot()
        }
    }

    private fun replyWithSnapshot(request: Message) {
        if (request.what == MSG_RECONFIGURE &&
            request.data.getLong("runtime_reconfigure_generation", -1) == connectionGeneration.get()
        ) {
            runtimeReconfigureInFlight = false
            if (nativeRuntimeActive.get()) ensureStatusTask()
        }
        val reply =
            Message.obtain(null, MSG_SNAPSHOT).apply {
                arg1 = request.arg1
                data = snapshotBundle()
            }
        try {
            request.replyTo?.send(reply)
        } catch (_: RemoteException) {
            // The UI process disappeared; the VPN process remains authoritative.
        }
    }

    private fun replyControlError(
        request: Message,
        code: String,
        message: String,
    ) {
        val reply =
            Message.obtain(null, MSG_SNAPSHOT).apply {
                arg1 = request.arg1
                data =
                    snapshotBundle().apply {
                        putString("control_error_code", code)
                        putString("control_error_message", message.take(512))
                    }
            }
        try {
            request.replyTo?.send(reply)
        } catch (_: RemoteException) {
            // The UI process disappeared; the VPN process remains authoritative.
        }
    }

    private fun broadcastSnapshot() {
        observeNetworkSettings()
        val snapshot = snapshotState.takeBroadcastBundle(platformFlags()) ?: return
        eventClients.forEach { client -> sendEvent(client, snapshot) }
    }

    private fun sendEvent(
        client: Messenger,
        snapshot: Bundle = snapshotBundle(),
    ) {
        try {
            client.send(
                Message.obtain(null, MSG_EVENT).apply {
                    data = Bundle(snapshot)
                },
            )
        } catch (_: RemoteException) {
            eventClients.remove(client)
        }
    }

    private fun snapshotBundle(): Bundle =
        snapshotState.toBundle(platformFlags()).apply {
            val (dnsMode, dnsConfiguration) = diagnosticProbes.configuration()
            putString("direct_dns_mode", dnsMode)
            putString("direct_dns_configuration", dnsConfiguration)
            putBoolean(
                TILE_VPN_ACTIVE,
                activeProfileJson.get() != null && activeMode.get() == "vpn",
            )
        }

    private fun platformFlags(): ServiceSnapshotState.PlatformFlags =
        ServiceSnapshotState.PlatformFlags(
            tunnelOpen = tunnel.get() != null,
            activeMode = activeMode.get(),
            platformLockdown =
                Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && isLockdownEnabled,
            alwaysOn = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && isAlwaysOn,
            tunFdValid = tunnel.get()?.fileDescriptor?.valid() == true,
            underlyingNetworkPresent = networkMonitor.underlyingNetwork() != null,
            underlyingFamilyMask = networkMonitor.underlyingFamilyMask(),
            networkGeneration = networkMonitor.generation(),
            dnsServerCount = networkMonitor.underlyingDnsServers().size,
            nativeRuntimeActive = nativeRuntimeActive.get(),
            foregroundNotificationActive = activeProfileJson.get() != null,
            pendingCleanup = clearAllRequested.get() || nativeStops.pendingCleanup(),
        )

    private fun updateNotification() {
        notifications.update(notifications.copyFor(snapshotState))
    }

    private fun updateLocale(catalogId: String?) {
        if (catalogId == null || !AndroidLocaleController.applyToProcess(catalogId)) return
        refreshLocalizedSurfaces()
    }

    private fun refreshLocalizedSurfaces() {
        notifications.createChannel()
        if (snapshotState.phase != "disconnected") updateNotification()
        lastTilePresentation = null
        notifyTileStateChanged()
    }

    private fun isCurrent(generation: Long): Boolean = !destroyed && connectionGeneration.get() == generation

    @Keep
    fun getUnderlyingNetworkHandle(): Long = networkMonitor.underlyingNetwork()?.networkHandle ?: 0L

    @Keep
    fun getUnderlyingFamilyMask(): Int = networkMonitor.underlyingFamilyMask()

    @Keep
    fun getUnderlyingNetworkGeneration(): Long = networkMonitor.generation()

    @Keep
    fun getUnderlyingRecoverySnapshot(): LongArray = networkMonitor.recoverySnapshot()

    @Keep
    fun bindSocketToUnderlyingGeneration(
        descriptor: Int,
        expectedGeneration: Long,
        requireVpnProtection: Boolean,
    ): Int {
        // JNI result contract: 0 = bound, 1 = stale generation, 2 = rejected.
        if (descriptor < 0 || expectedGeneration < 0 || destroyed) return 2
        if (networkMonitor.generation() != expectedGeneration) return 1
        val network = networkMonitor.networkForGeneration(expectedGeneration) ?: return 1
        if (requireVpnProtection && !protect(descriptor)) return 2
        if (networkMonitor.generation() != expectedGeneration) return 1
        if (destroyed) return 2
        val duplicate =
            try {
                ParcelFileDescriptor.fromFd(descriptor)
            } catch (_: Exception) {
                return 2
            }
        try {
            network.bindSocket(duplicate.fileDescriptor)
        } catch (_: Exception) {
            return 2
        } finally {
            closeQuietly(duplicate)
        }
        if (destroyed) return 2
        return if (
            networkMonitor.generation() == expectedGeneration &&
            networkMonitor.networkForGeneration(expectedGeneration)?.networkHandle ==
            network.networkHandle
        ) {
            0
        } else {
            1
        }
    }

    @Keep
    fun getUnderlyingDnsServers(): Array<String> =
        networkMonitor
            .underlyingDnsServers()
            .mapNotNull { address ->
                val host = address.hostAddress?.substringBefore('%') ?: return@mapNotNull null
                val scope = if (address is Inet6Address) address.scopeId else 0
                "$host|$scope"
            }.distinct()
            .take(8)
            .toTypedArray()

    @Keep
    fun resolveUnderlyingHost(host: String): Array<String> {
        val network = networkMonitor.underlyingNetwork() ?: return emptyArray()
        return network
            .getAllByName(host)
            .mapNotNull { address -> address.hostAddress?.substringBefore('%') }
            .distinct()
            .take(16)
            .toTypedArray()
    }

    @Keep
    fun persistRefreshedWarpIdentity(
        profileId: String,
        secret: ByteArray,
    ): Boolean =
        try {
            SecureIdentityStore(this).put(
                profileId,
                SecureIdentityStore.Record.WARP_SECRET,
                secret,
            )
            true
        } catch (_: Exception) {
            false
        } finally {
            secret.fill(0)
        }

    private data class NativeStartFailure(
        val code: String,
        val message: String,
        val gateStatus: String? = null,
    )

    private fun withProxyPassword(
        profileJson: String,
        operation: (ByteArray) -> Int,
    ): Int {
        val password = loadProxyPassword(JSONObject(profileJson).optString("id"), profileJson)
        return try {
            operation(password)
        } finally {
            password.fill(0)
        }
    }

    private fun reconfigureProxyAuth(request: Message) {
        val current = activeProfileJson.get()
        if (current == null && !nativeRuntimeActive.get()) {
            replyWithSnapshot(request)
            return
        }
        if (settingsApplication.busy || runtimeReconfigureInFlight || !nativeRuntimeActive.get()) {
            disconnect(stopService = false)
            replyControlError(request, "PROXY_AUTH_APPLY_FAILED", "Credentials were saved. Reconnect to apply them.")
            return
        }
        val generation = connectionGeneration.get()
        runtimeReconfigureInFlight = true
        engineExecutor.execute {
            var applied: String? = null
            val code =
                try {
                    SharedProxyCredentials.withCurrent(
                        AndroidEngineMethodHandler.SecureIdentityStoreAdapter(SecureIdentityStore(this)),
                        {
                            JSONObject(
                                requireNotNull(
                                    NativeEngine.applyProfileCommand(settingsPath, "{\"command\":\"list_profiles\"}"),
                                ),
                            )
                        },
                    ) { catalog, password ->
                        val source = JSONObject(requireNotNull(current))
                        val shared = catalog.getJSONObject("shared_network_profile").getJSONObject("proxy")
                        source.getJSONObject("proxy").put("auth_username", shared.optString("auth_username"))
                        val next = source.toString()
                        if (isCurrent(generation)) {
                            applied = next
                            NativeEngine.reconfigure(next, password)
                        } else {
                            NativeEngine.OK
                        }
                    }
                } catch (_: Exception) {
                    NativeEngine.ERROR_NOT_LINKED
                }
            mainHandler.post {
                if (!isCurrent(generation)) {
                    replyWithSnapshot(request)
                    return@post
                }
                runtimeReconfigureInFlight = false
                if (code == NativeEngine.OK && applied != null) {
                    try {
                        activeProfileJson.set(applied)
                        recoveryPreferences.edit {
                            putString(RECOVERY_PROFILE, applied)
                            val last = recoveryPreferences.getString(LAST_PROFILE, null)
                            if (last != null) {
                                val saved = JSONObject(last)
                                saved
                                    .getJSONObject(
                                        "proxy",
                                    ).put(
                                        "auth_username",
                                        JSONObject(applied!!).getJSONObject("proxy").optString("auth_username"),
                                    )
                                putString(LAST_PROFILE, saved.toString())
                            }
                        }
                        refreshNativeSnapshot()
                        replyWithSnapshot(request)
                    } catch (_: Exception) {
                        disconnect(stopService = false)
                        replyControlError(
                            request,
                            "PROXY_AUTH_APPLY_FAILED",
                            "Credentials were saved. Reconnect to apply them.",
                        )
                    }
                } else {
                    disconnect(stopService = false)
                    replyControlError(
                        request,
                        "PROXY_AUTH_APPLY_FAILED",
                        "Credentials were saved. Reconnect to apply them.",
                    )
                }
            }
        }
    }

    private fun loadProxyPassword(
        profileId: String,
        profileJson: String,
    ): ByteArray {
        val username =
            runCatching {
                JSONObject(profileJson)
                    .optJSONObject("proxy")
                    ?.optString("auth_username")
                    .orEmpty()
            }.getOrDefault("")
        if (username.isEmpty() || profileId.isBlank()) {
            return ByteArray(0)
        }
        return runCatching {
            val catalog =
                JSONObject(
                    requireNotNull(NativeEngine.applyProfileCommand(settingsPath, "{\"command\":\"list_profiles\"}")),
                )
            SharedProxyCredentials.read(
                AndroidEngineMethodHandler.SecureIdentityStoreAdapter(SecureIdentityStore(this)),
                catalog,
            )
        }.getOrNull() ?: ByteArray(0)
    }

    private fun nativeStartFailure(result: Int): NativeStartFailure {
        val nativeSnapshot =
            try {
                NativeEngine.snapshot()?.let(::JSONObject)
            } catch (_: Exception) {
                null
            }
        val structuredCode = nativeSnapshot?.optNullableString("error_code")
        val structuredMessage = nativeSnapshot?.optNullableString("warning")
        val fallback =
            when (result) {
                NativeEngine.ERROR_INVALID_WARP_SECRET -> {
                    NativeStartFailure(
                        "IDENTITY_INVALID",
                        "The stored WARP identity was rejected.",
                    )
                }

                NativeEngine.ERROR_ALREADY_RUNNING -> {
                    NativeStartFailure(
                        "ANDROID_RUNTIME_FAILED",
                        "Another native data channel is already running.",
                    )
                }

                NativeEngine.ERROR_INVALID_PROFILE -> {
                    NativeStartFailure(
                        "ANDROID_RUNTIME_FAILED",
                        "The Rust engine rejected this profile.",
                    )
                }

                NativeEngine.ERROR_PLATFORM_FAILURE -> {
                    NativeStartFailure(
                        "ANDROID_RUNTIME_FAILED",
                        "Android could not initialize the native runtime.",
                    )
                }

                NativeEngine.ERROR_TRANSPORT_FAILURE -> {
                    NativeStartFailure(
                        "MASQUE_CONNECT_FAILED",
                        "The MASQUE endpoint could not be reached with HTTP/3 or HTTP/2.",
                    )
                }

                NativeEngine.ERROR_TUN_FAILURE -> {
                    NativeStartFailure(
                        "ANDROID_RUNTIME_FAILED",
                        "The Rust engine could not own the VPN interface.",
                    )
                }

                else -> {
                    NativeStartFailure(
                        "ANDROID_RUNTIME_FAILED",
                        "The native engine rejected the network request ($result).",
                    )
                }
            }
        return NativeStartFailure(
            structuredCode?.take(64) ?: fallback.code,
            structuredMessage?.take(512) ?: fallback.message,
            VpnGateFields.stoppedStatus(nativeSnapshot?.optJSONObject("vpn_gate")),
        )
    }

    private fun safeMessage(error: Exception): String = (error.message ?: error.javaClass.simpleName).take(256)

    private fun closeQuietly(descriptor: ParcelFileDescriptor?) {
        try {
            descriptor?.close()
        } catch (_: Exception) {
            // The descriptor may already have been revoked by Android.
        }
    }
}
