package io.github.georgexie2333.usque

import android.annotation.SuppressLint
import android.content.Intent
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
import android.service.quicksettings.TileService
import androidx.annotation.Keep
import androidx.core.content.ContextCompat
import org.json.JSONObject
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
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

        /**
         * Whether the reconfigure payload still wants the VpnService TUN.
         * Composable profiles keep legacy `mode=vpn` after turning only
         * `frontends.tunnel` off; the TUN must follow the frontend flag.
         */
        internal fun tunnelFrontendEnabled(profileJson: String): Boolean {
            val source = JSONObject(profileJson)
            val frontends = source.optJSONObject("frontends")
            if (frontends != null && frontends.has("tunnel")) {
                return frontends.getBoolean("tunnel")
            }
            return source.optString("mode") == "vpn"
        }

        /**
         * [MSG_RECONFIGURE] follow-up after [NativeEngine.reconfigure] returns
         * OK. Closes the VpnService TUN when `frontends.tunnel` is false, even
         * if legacy `mode` remains `vpn`.
         */
        internal fun <Fd> handleReconfigureNativeOk(
            what: Int,
            extras: Map<String, Any?>,
            tunnel: AtomicReference<Fd?>,
            lastTunIdentity: AtomicReference<TunIdentity?>,
            closeQuietly: (Fd?) -> Unit,
        ) {
            if (what != MSG_RECONFIGURE) {
                return
            }
            val profileJson = extras[EXTRA_PROFILE_JSON] as? String ?: return
            if (!tunnelFrontendEnabled(profileJson)) {
                lastTunIdentity.set(null)
                closeQuietly(tunnel.getAndSet(null))
            }
        }

        private const val NATIVE_STATUS_INTERVAL_MILLIS = 1_000L
        private const val PHYSICAL_NETWORK_WAIT_MILLIS = 8_000L
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
    private val clearAllRequested = AtomicBoolean()
    private val activeProfileJson = AtomicReference<String?>(null)
    private val activeMode = AtomicReference<String?>(null)
    private val lastTunIdentity = AtomicReference<TunIdentity?>(null)
    @Volatile private var pendingTunRestart: TunRestartDecision = TunRestartDecision.TEARDOWN
    private val eventClients = CopyOnWriteArrayList<Messenger>()
    private val recoveryPreferences by lazy {
        createDeviceProtectedStorageContext()
            .getSharedPreferences(RECOVERY_PREFERENCES, MODE_PRIVATE)
    }
    private val flagCache by lazy { FlagSvgCache(this) }
    private val logStore by lazy { AndroidLogStore(this) }
    private val snapshotState = ServiceSnapshotState()
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
    private var statusTask: ScheduledFuture<*>? = null

    private val controlMessenger =
        Messenger(
            Handler(Looper.getMainLooper()) { message ->
                when (message.what) {
                    MSG_SNAPSHOT -> {
                        replyWithSnapshot(message)
                        true
                    }

                    MSG_REGISTER_EVENTS -> {
                        message.replyTo?.let { client ->
                            if (!eventClients.contains(client)) eventClients += client
                            sendEvent(client)
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
                val recoveryMode =
                    try {
                        recoveryProfile?.let(::JSONObject)?.optString("mode")
                    } catch (_: Exception) {
                        null
                    }
                if (
                    recoveryProfile != null &&
                    (recoveryMode != "vpn" || VpnService.prepare(this) == null) &&
                    recoveryProfile.toByteArray(Charsets.UTF_8).size <= MAX_PROFILE_BYTES
                ) {
                    beginConnection(recoveryProfile)
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
        if (!clearAllRequested.get()) {
            logStore.record(AndroidLogStore.Event.SERVICE_DESTROYED, phase = snapshotState.phase)
        }
        destroyed = true
        networkMonitor.cancelScheduledSelection()
        connectionGeneration.incrementAndGet()
        networkMonitor.bumpGeneration()
        activeProfileJson.set(null)
        activeMode.set(null)
        nativeRuntimeActive.set(false)
        statusTask?.cancel(false)
        statusTask = null
        eventClients.clear()
        NativeEngine.cancel()
        val descriptor = tunnel.getAndSet(null)
        closeQuietly(descriptor)
        stopExecutor.execute {
            NativeEngine.stop()
        }
        engineExecutor.shutdownNow()
        statusExecutor.shutdownNow()
        stopExecutor.shutdown()
        networkMonitor.unregister(getSystemService(ConnectivityManager::class.java))
        super.onDestroy()
    }

    // Recovery state must be durable before starting the native connection.
    @SuppressLint("ApplySharedPref", "UseKtx")
    private fun beginConnection(profileJson: String) {
        if (profileJson.toByteArray(Charsets.UTF_8).size > MAX_PROFILE_BYTES) {
            startForeground(
                VpnNotificationController.NOTIFICATION_ID,
                notifications.build("Invalid VPN profile"),
            )
            snapshotState.reset("error")
            snapshotState.warning = "The VPN profile exceeds the Android safety limit."
            broadcastSnapshot()
            return
        }
        val mode =
            try {
                JSONObject(profileJson).getString("mode").also { parsedMode ->
                    require(parsedMode in setOf("vpn", "socks5", "httpProxy"))
                    if (parsedMode == "vpn") AndroidVpnProfile.parse(profileJson)
                }
            } catch (error: Exception) {
                startForeground(
                    VpnNotificationController.NOTIFICATION_ID,
                    notifications.build("Invalid network profile"),
                )
                snapshotState.reset("error")
                snapshotState.warning = "The network profile is invalid: ${safeMessage(error)}"
                broadcastSnapshot()
                return
            }
        if (
            !recoveryPreferences
                .edit()
                .putString(RECOVERY_PROFILE, profileJson)
                .putString(LAST_PROFILE, profileJson)
                .commit()
        ) {
            startForeground(
                VpnNotificationController.NOTIFICATION_ID,
                notifications.build("VPN recovery unavailable"),
            )
            snapshotState.reset("error")
            snapshotState.warning = "Android could not save the non-secret recovery profile."
            broadcastSnapshot()
            return
        }
        val generation = connectionGeneration.incrementAndGet()
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
            notifications.build("Preparing secure tunnel"),
        )
        snapshotState.reset("preparing")
        notifyTileStateChanged()
        broadcastSnapshot()

        val incomingIdentity =
            runCatching { TunIdentity.from(AndroidVpnProfile.parse(profileJson)) }.getOrNull()
        val decision =
            TunRestartPolicy.decide(
                killSwitch = incomingIdentity != null && JSONObject(profileJson).optBoolean("kill_switch", false),
                tunnelFrontend = mode == "vpn",
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
                tunnel.getAndSet(null)
            } else {
                null
            }
        val stopped =
            stopExecutor.submit {
                NativeEngine.stop()
                closeQuietly(staleDescriptor)
            }
        engineExecutor.execute {
            try {
                stopped.get(35, TimeUnit.SECONDS)
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
        val profileJson = recoveryPreferences.getString(LAST_PROFILE, null)
        if (profileJson.isNullOrEmpty()) {
            request.let(::replyWithSnapshot)
            return
        }
        beginConnection(profileJson)
        request.let(::replyWithSnapshot)
    }

    @SuppressLint("ApplySharedPref", "UseKtx")
    private fun reconfigureConnection(request: Message) {
        val profileJson = request.data.getString(EXTRA_PROFILE_JSON).orEmpty()
        if (profileJson.isEmpty() || profileJson.toByteArray(Charsets.UTF_8).size > MAX_PROFILE_BYTES) {
            replyControlError(request, "INVALID_ARGUMENT", "The reconfigure profile is malformed.")
            return
        }
        recoveryPreferences
            .edit()
            .putString(RECOVERY_PROFILE, profileJson)
            .putString(LAST_PROFILE, profileJson)
            .commit()
        activeProfileJson.set(profileJson)
        val mode =
            try {
                JSONObject(profileJson).getString("mode")
            } catch (_: Exception) {
                replyControlError(request, "INVALID_PROFILE", "The reconfigure profile is invalid.")
                return
            }
        activeMode.set(mode)

        if (!nativeRuntimeActive.get()) {
            beginConnection(profileJson)
            request.let(::replyWithSnapshot)
            return
        }

        engineExecutor.execute {
            val result = NativeEngine.reconfigure(profileJson)
            mainHandler.post {
                when (result) {
                    NativeEngine.OK -> {
                        handleReconfigureNativeOk(
                            MSG_RECONFIGURE,
                            mapOf(EXTRA_PROFILE_JSON to profileJson),
                            tunnel,
                            lastTunIdentity,
                            ::closeQuietly,
                        )
                        if (!tunnelFrontendEnabled(profileJson) && mode == "vpn") {
                            activeMode.set("socks5")
                        }
                        refreshNativeSnapshot()
                        replyWithSnapshot(request)
                    }
                    NativeEngine.RECONFIGURE_NEED_COLD -> {
                        beginConnection(profileJson)
                        replyWithSnapshot(request)
                    }
                    NativeEngine.RECONFIGURE_NEED_ATTACH -> {
                        attachTunWhileRunning(profileJson, request)
                    }
                    else -> {
                        val failure = nativeStartFailure(result)
                        fail(
                            connectionGeneration.get(),
                            failure.code,
                            failure.message,
                        )
                        mainHandler.post { replyWithSnapshot(request) }
                    }
                }
            }
        }
    }

    private fun attachTunWhileRunning(
        profileJson: String,
        request: Message,
    ) {
        engineExecutor.execute {
            val generation = connectionGeneration.get()
            val profile =
                try {
                    AndroidVpnProfile.parse(profileJson)
                } catch (error: Exception) {
                    fail(generation, "The VPN profile is invalid: ${safeMessage(error)}")
                    mainHandler.post { replyWithSnapshot(request) }
                    return@execute
                }
            val secret =
                try {
                    SecureIdentityStore(this).get(profile.id, SecureIdentityStore.Record.WARP_SECRET)
                } catch (error: Exception) {
                    fail(
                        generation,
                        "IDENTITY_INVALID",
                        "Android Keystore could not read the WARP identity.",
                    )
                    mainHandler.post { replyWithSnapshot(request) }
                    return@execute
                }
            if (secret == null) {
                fail(generation, "IDENTITY_INVALID", "This profile has no Consumer WARP identity.")
                mainHandler.post { replyWithSnapshot(request) }
                return@execute
            }
            try {
                val assignment =
                    try {
                        val metadata =
                            NativeEngine.inspectWarpSecret(secret)
                                ?: throw IllegalArgumentException("identity metadata is unavailable")
                        WarpAddressAssignment.parse(metadata)
                    } catch (error: Exception) {
                        fail(
                            generation,
                            "IDENTITY_INVALID",
                            "The stored WARP identity is invalid: ${safeMessage(error)}",
                        )
                        mainHandler.post { replyWithSnapshot(request) }
                        return@execute
                    }
                val routePlan =
                    try {
                        VpnRoutePlanner.plan(
                            includeIpv4 = profile.includeIpv4,
                            includeIpv6 = profile.includeIpv6,
                            allowLan = profile.allowLan,
                            bypassCidrs = profile.bypassCidrs,
                            supportsRouteExclusion = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU,
                        )
                    } catch (error: Exception) {
                        fail(generation, "The bypass route configuration is unsafe: ${safeMessage(error)}")
                        mainHandler.post { replyWithSnapshot(request) }
                        return@execute
                    }
                val descriptor =
                    try {
                        establishVpn(profile, assignment, routePlan)
                    } catch (error: Exception) {
                        fail(generation, "Android refused the VPN configuration: ${safeMessage(error)}")
                        mainHandler.post { replyWithSnapshot(request) }
                        return@execute
                    }
                if (descriptor == null) {
                    fail(generation, "Android refused to create the VPN interface.")
                    mainHandler.post { replyWithSnapshot(request) }
                    return@execute
                }
                val existing = tunnel.getAndSet(descriptor)
                if (existing != null && existing !== descriptor) {
                    closeQuietly(existing)
                }
                lastTunIdentity.set(TunIdentity.from(profile))
                val attached = NativeEngine.attachTun(descriptor.fd, profileJson)
                if (attached != NativeEngine.OK) {
                    tunnel.compareAndSet(descriptor, null)
                    closeQuietly(descriptor)
                    lastTunIdentity.set(null)
                    val failure = nativeStartFailure(attached)
                    fail(generation, failure.code, failure.message)
                    mainHandler.post { replyWithSnapshot(request) }
                    return@execute
                }
                mainHandler.post {
                    snapshotState.killSwitchEnabled = profile.killSwitch
                    refreshNativeSnapshot()
                    replyWithSnapshot(request)
                }
            } finally {
                secret.fill(0)
            }
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
                    "Open Usque and connect a VPN profile once before using the tile.",
                )
            }
            stopSelf()
            return
        }

        val profile = runCatching { JSONObject(profileJson) }.getOrNull()
        val profileId = profile?.optString("id").orEmpty()
        if (profile == null || profile.optString("mode") != "vpn" || profileId.isBlank()) {
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
                    "Open Usque and configure the WARP identity for this profile.",
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
                    "Open Usque to grant Android VPN permission.",
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
                "A proxy-only connection is active. Open Usque to enable the VPN frontend.",
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
        val mode =
            try {
                JSONObject(profileJson).getString("mode")
            } catch (error: Exception) {
                fail(generation, "The VPN profile is invalid: ${safeMessage(error)}")
                return
            }
        activeMode.set(mode)
        if (mode == "socks5" || mode == "httpProxy") {
            startProxyConnection(generation, profileJson)
            return
        }
        if (mode != "vpn") {
            fail(generation, "The Android operating mode is invalid.")
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
                SecureIdentityStore(this).get(
                    profile.id,
                    SecureIdentityStore.Record.WARP_SECRET,
                )
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
                    val metadata =
                        NativeEngine.inspectWarpSecret(secret)
                            ?: throw IllegalArgumentException("identity metadata is unavailable")
                    WarpAddressAssignment.parse(metadata)
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
                    VpnRoutePlanner.plan(
                        includeIpv4 = profile.includeIpv4,
                        includeIpv6 = profile.includeIpv6,
                        allowLan = profile.allowLan,
                        bypassCidrs = profile.bypassCidrs,
                        supportsRouteExclusion = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU,
                    )
                } catch (error: Exception) {
                    fail(generation, "The bypass route configuration is unsafe: ${safeMessage(error)}")
                    return
                }
            if (!awaitPhysicalNetwork(generation)) {
                fail(
                    generation,
                    "ANDROID_WAITING_FOR_PHYSICAL_NETWORK",
                    "Android did not provide a usable non-VPN physical network within 8 seconds.",
                )
                return
            }
            if (!isCurrent(generation)) return
            val existing = tunnel.get()
            val restart = pendingTunRestart
            val descriptor =
                when {
                    restart == TunRestartDecision.RETAIN && existing != null -> existing
                    else -> {
                        val created =
                            try {
                                establishVpn(profile, assignment, routePlan)
                            } catch (error: Exception) {
                                fail(generation, "Android refused the VPN configuration: ${safeMessage(error)}")
                                return
                            }
                        if (created == null) {
                            fail(generation, "Android refused to create the VPN interface.")
                            return
                        }
                        if (restart == TunRestartDecision.REPLACE_NEW_FIRST && existing != null) {
                            tunnel.compareAndSet(existing, created)
                            closeQuietly(existing)
                        } else {
                            tunnel.set(created)
                        }
                        created
                    }
                }
            if (!isCurrent(generation)) {
                if (restart != TunRestartDecision.RETAIN) {
                    tunnel.compareAndSet(descriptor, null)
                    closeQuietly(descriptor)
                }
                return
            }
            lastTunIdentity.set(TunIdentity.from(profile))
            snapshotState.killSwitchEnabled = profile.killSwitch
            postPhase(generation, "connectingH3", null)
            val proxyPassword = loadProxyPassword(profile.id, profileJson)
            val startResult =
                try {
                    NativeEngine.start(
                        descriptor.fd,
                        profileJson,
                        secret,
                        proxyPassword,
                        this,
                    )
                } finally {
                    proxyPassword.fill(0)
                }
            if (startResult != NativeEngine.OK) {
                if (!profile.killSwitch) {
                    tunnel.compareAndSet(descriptor, null)
                    closeQuietly(descriptor)
                    lastTunIdentity.set(null)
                }
                val failure = nativeStartFailure(startResult)
                fail(generation, failure.code, failure.message)
                return
            }
            if (!isCurrent(generation)) {
                NativeEngine.stop()
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
                SecureIdentityStore(this).get(
                    profileId,
                    SecureIdentityStore.Record.WARP_SECRET,
                )
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
                fail(generation, failure.code, failure.message)
                return
            }
            if (!isCurrent(generation)) {
                NativeEngine.stop()
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

    private fun establishVpn(
        profile: AndroidVpnProfile,
        assignment: WarpAddressAssignment,
        routePlan: RoutePlan,
    ): ParcelFileDescriptor? {
        val builder =
            Builder()
                .setSession(profile.name)
                .setMtu(profile.mtu)
                .setBlocking(false)
        networkMonitor.underlyingNetwork()?.let { network ->
            builder.setUnderlyingNetworks(arrayOf(network))
        }
        if (profile.includeIpv4) builder.addAddress(assignment.ipv4, 32)
        if (profile.includeIpv6) builder.addAddress(assignment.ipv6, 128)
        profile.dnsServers.forEach(builder::addDnsServer)
        routePlan.included.forEach { route ->
            builder.addRoute(route.address, route.prefixLength)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            routePlan.excluded.forEach { route ->
                builder.excludeRoute(IpPrefix(route.address, route.prefixLength))
            }
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            builder.setMetered(false)
        }
        return builder.establish()
    }

    // Remove recovery state before the service can be stopped.
    @SuppressLint("ApplySharedPref", "UseKtx")
    private fun disconnect(
        stopService: Boolean,
        request: Message? = null,
    ) {
        recoveryPreferences.edit().remove(RECOVERY_PROFILE).commit()
        lastTunIdentity.set(null)
        pendingTunRestart = TunRestartDecision.TEARDOWN
        val generation = connectionGeneration.incrementAndGet()
        networkMonitor.bumpGeneration()
        activeProfileJson.set(null)
        val stoppedMode = activeMode.getAndSet(null)
        nativeRuntimeActive.set(false)
        statusTask?.cancel(false)
        statusTask = null
        NativeEngine.cancel()
        val descriptor = tunnel.getAndSet(null)
        closeQuietly(descriptor)
        snapshotState.reset("disconnected")
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
        // visible disconnect. The TUN and cancellation gate are already closed.
        stopExecutor.execute {
            NativeEngine.stop()
            mainHandler.post {
                if (connectionGeneration.get() == generation && stopService) stopSelf()
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
        recoveryPreferences.edit().clear().commit()
        val generation = connectionGeneration.incrementAndGet()
        networkMonitor.bumpGeneration()
        activeProfileJson.set(null)
        activeMode.set(null)
        nativeRuntimeActive.set(false)
        statusTask?.cancel(false)
        statusTask = null
        snapshotState.phase = "disconnecting"
        snapshotState.warning = null
        notifyTileStateChanged()
        broadcastSnapshot()
        NativeEngine.cancel()
        val descriptor = tunnel.getAndSet(null)
        closeQuietly(descriptor)
        stopExecutor.execute {
            NativeEngine.stop()
            mainHandler.post {
                if (connectionGeneration.get() == generation) {
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
                    snapshotState.phase = "reconnecting"
                    snapshotState.errorCode = null
                    snapshotState.warning =
                        if (selectedNetwork == null) {
                            "ANDROID_WAITING_FOR_PHYSICAL_NETWORK: waiting for a usable non-VPN network."
                        } else {
                            "The underlying Android network changed; rebuilding the secure channel."
                        }
                    updateNotification()
                    notifyTileStateChanged()
                    broadcastSnapshot()
                    ensureStatusTask()
                }
            }
        }
    }

    private fun awaitPhysicalNetwork(connectionToken: Long): Boolean =
        networkMonitor.awaitPhysicalNetwork(
            isCurrent = { isCurrent(connectionToken) },
            waitMillis = PHYSICAL_NETWORK_WAIT_MILLIS,
        )

    private fun ensureStatusTask() {
        if (statusTask?.isDone == false) return
        statusTask =
            statusExecutor.scheduleWithFixedDelay(
                ::refreshNativeSnapshotInBackground,
                0,
                NATIVE_STATUS_INTERVAL_MILLIS,
                TimeUnit.MILLISECONDS,
            )
    }

    private fun refreshNativeSnapshot() {
        if (destroyed || !nativeRuntimeActive.get()) return
        statusExecutor.execute(::refreshNativeSnapshotInBackground)
    }

    private fun refreshNativeSnapshotInBackground() {
        if (destroyed || !nativeRuntimeActive.get()) return
        val source =
            try {
                JSONObject(NativeEngine.snapshot() ?: return)
            } catch (_: Exception) {
                return
            }
        mainHandler.post {
            if (!destroyed && nativeRuntimeActive.get()) {
                applyNativeSnapshot(source)
            }
        }
    }

    private fun applyNativeSnapshot(source: JSONObject) {
        val merge = snapshotState.applyNativeSnapshot(source)
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
            // Keep the TUN open and fail closed until the user retries or disconnects.
            statusTask?.cancel(false)
            statusTask = null
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
    ) {
        mainHandler.post {
            if (!isCurrent(generation)) return@post
            nativeRuntimeActive.set(false)
            snapshotState.phase = "error"
            snapshotState.errorCode = code
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
        )

    private fun updateNotification() {
        notifications.update(snapshotState.notificationText())
    }

    private fun isCurrent(generation: Long): Boolean = !destroyed && connectionGeneration.get() == generation

    @Keep
    fun getUnderlyingNetworkHandle(): Long = networkMonitor.underlyingNetwork()?.networkHandle ?: 0L

    @Keep
    fun getUnderlyingFamilyMask(): Int = networkMonitor.underlyingFamilyMask()

    @Keep
    fun getUnderlyingNetworkGeneration(): Long = networkMonitor.generation()

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
    )

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
            SecureIdentityStore(this).get(profileId, SecureIdentityStore.Record.PROXY_PASSWORD)
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
