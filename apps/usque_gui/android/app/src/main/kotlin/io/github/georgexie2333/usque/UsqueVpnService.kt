package io.github.georgexie2333.usque

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.net.ConnectivityManager
import android.net.IpPrefix
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
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
import androidx.annotation.Keep
import org.json.JSONObject
import java.time.Instant
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

class UsqueVpnService : VpnService() {
    companion object {
        const val ACTION_CONNECT = "io.github.georgexie2333.usque.CONNECT"
        const val ACTION_DISCONNECT = "io.github.georgexie2333.usque.DISCONNECT"
        const val ACTION_CONTROL = "io.github.georgexie2333.usque.CONTROL"
        const val EXTRA_PROFILE_JSON = "profile_json"

        const val MSG_SNAPSHOT = 1
        const val MSG_REGISTER_EVENTS = 2
        const val MSG_UNREGISTER_EVENTS = 3
        const val MSG_EVENT = 4
        const val MSG_PAUSE_CAPTIVE_PORTAL = 5
        const val MSG_CLEAR_ALL_DATA = 6
        const val MSG_DISCONNECT = 7

        private const val CHANNEL_ID = "usque_vpn"
        private const val NOTIFICATION_ID = 1048
        private const val NATIVE_STATUS_INTERVAL_MILLIS = 1_000L
        private const val PHYSICAL_NETWORK_WAIT_MILLIS = 8_000L
        private const val RECOVERY_PREFERENCES = "usque_vpn_recovery_v1"
        private const val RECOVERY_PROFILE = "active_profile_json"
        private const val MAX_PROFILE_BYTES = 256 * 1024
        private const val FAMILY_IPV4 = 0x1
        private const val FAMILY_IPV6 = 0x2
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
    private val networkRestartGeneration = AtomicLong()
    private val tunnel = AtomicReference<ParcelFileDescriptor?>()
    private val nativeRuntimeActive = AtomicBoolean()
    private val clearAllRequested = AtomicBoolean()
    private val activeProfileJson = AtomicReference<String?>(null)
    private val activeMode = AtomicReference<String?>(null)
    private val underlyingNetwork = AtomicReference<Network?>(null)
    private val underlyingFamilyMask = AtomicInteger()
    private val availableNetworks = ConcurrentHashMap<Network, NetworkCandidate>()
    private val eventClients = CopyOnWriteArrayList<Messenger>()
    private val recoveryPreferences by lazy {
        createDeviceProtectedStorageContext()
            .getSharedPreferences(RECOVERY_PREFERENCES, MODE_PRIVATE)
    }
    private val flagCache by lazy { FlagSvgCache(this) }
    private val logStore by lazy { AndroidLogStore(this) }

    @Volatile private var destroyed = false
    private var statusTask: ScheduledFuture<*>? = null
    private var captivePauseTask: ScheduledFuture<*>? = null
    private var captivePauseDeadlineUnixMillis = 0L
    private var phase = "disconnected"
    private var warning: String? = null
    private var errorCode: String? = null
    private var transport: String? = null
    private var addressFamily: String? = null
    private var connectedAt: String? = null
    private var downloadBytesPerSecond = 0L
    private var uploadBytesPerSecond = 0L
    private var downloadedBytes = 0L
    private var uploadedBytes = 0L
    private var reconnectCount = 0
    private var activeListeners = emptyList<String>()
    private var exitIpv4: String? = null
    private var exitIpv6: String? = null
    private var exitCity: String? = null
    private var exitCountry: String? = null
    private var exitCountryCode: String? = null
    private var exitFlagSvg: String? = null
    private var flagCacheLookupCode: String? = null
    private var killSwitchEnabled = false
    private var lastBroadcastFingerprint: String? = null

    private data class NetworkCandidate(
        val capabilities: NetworkCapabilities? = null,
        val linkProperties: LinkProperties? = null,
        val blocked: Boolean = false,
    )

    private val networkSelectionTask = Runnable(::selectUnderlyingNetwork)

    private val networkCallback =
        object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                availableNetworks.putIfAbsent(network, NetworkCandidate())
                scheduleUnderlyingNetworkSelection()
            }

            override fun onCapabilitiesChanged(
                network: Network,
                networkCapabilities: NetworkCapabilities,
            ) {
                availableNetworks.compute(network) { _, previous ->
                    (previous ?: NetworkCandidate()).copy(capabilities = networkCapabilities)
                }
                if (
                    networkCapabilities.hasCapability(
                        NetworkCapabilities.NET_CAPABILITY_VALIDATED,
                    )
                ) {
                    mainHandler.post {
                        if (phase == "captivePortalPaused") {
                            resumeFromCaptivePortalPause()
                        }
                    }
                }
                scheduleUnderlyingNetworkSelection()
            }

            override fun onLinkPropertiesChanged(
                network: Network,
                linkProperties: LinkProperties,
            ) {
                availableNetworks.compute(network) { _, previous ->
                    (previous ?: NetworkCandidate()).copy(linkProperties = linkProperties)
                }
                scheduleUnderlyingNetworkSelection()
            }

            override fun onBlockedStatusChanged(
                network: Network,
                blocked: Boolean,
            ) {
                availableNetworks.compute(network) { _, previous ->
                    (previous ?: NetworkCandidate()).copy(blocked = blocked)
                }
                scheduleUnderlyingNetworkSelection()
            }

            override fun onLost(network: Network) {
                availableNetworks.remove(network)
                scheduleUnderlyingNetworkSelection()
            }
        }

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

                    MSG_PAUSE_CAPTIVE_PORTAL -> {
                        pauseForCaptivePortal(message)
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

                    else -> {
                        false
                    }
                }
            },
        )

    override fun onCreate() {
        super.onCreate()
        logStore.record(AndroidLogStore.Event.SERVICE_CREATED)
        val manager = getSystemService(NotificationManager::class.java)
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
        val request =
            NetworkRequest
                .Builder()
                .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
                .build()
        getSystemService(ConnectivityManager::class.java)
            .registerNetworkCallback(request, networkCallback)
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
        logStore.record(AndroidLogStore.Event.VPN_PERMISSION_REVOKED, phase = phase)
        disconnect(stopService = true)
        super.onRevoke()
    }

    override fun onDestroy() {
        if (!clearAllRequested.get()) {
            logStore.record(AndroidLogStore.Event.SERVICE_DESTROYED, phase = phase)
        }
        destroyed = true
        mainHandler.removeCallbacks(networkSelectionTask)
        connectionGeneration.incrementAndGet()
        networkRestartGeneration.incrementAndGet()
        activeProfileJson.set(null)
        activeMode.set(null)
        nativeRuntimeActive.set(false)
        statusTask?.cancel(false)
        statusTask = null
        captivePauseTask?.cancel(false)
        captivePauseTask = null
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
        try {
            getSystemService(ConnectivityManager::class.java)
                .unregisterNetworkCallback(networkCallback)
        } catch (_: IllegalArgumentException) {
            // The callback may already have been revoked while the process exited.
        }
        super.onDestroy()
    }

    private fun beginConnection(profileJson: String) {
        captivePauseTask?.cancel(false)
        captivePauseTask = null
        captivePauseDeadlineUnixMillis = 0L
        if (profileJson.toByteArray(Charsets.UTF_8).size > MAX_PROFILE_BYTES) {
            startForeground(NOTIFICATION_ID, buildNotification("Invalid VPN profile"))
            resetSnapshot("error")
            warning = "The VPN profile exceeds the Android safety limit."
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
                startForeground(NOTIFICATION_ID, buildNotification("Invalid network profile"))
                resetSnapshot("error")
                warning = "The network profile is invalid: ${safeMessage(error)}"
                broadcastSnapshot()
                return
            }
        if (
            !recoveryPreferences
                .edit()
                .putString(RECOVERY_PROFILE, profileJson)
                .commit()
        ) {
            startForeground(NOTIFICATION_ID, buildNotification("VPN recovery unavailable"))
            resetSnapshot("error")
            warning = "Android could not save the non-secret recovery profile."
            broadcastSnapshot()
            return
        }
        val generation = connectionGeneration.incrementAndGet()
        networkRestartGeneration.incrementAndGet()
        activeProfileJson.set(profileJson)
        activeMode.set(mode)
        logStore.record(
            AndroidLogStore.Event.CONNECTION_REQUESTED,
            phase = "preparing",
            mode = mode,
        )
        startForeground(NOTIFICATION_ID, buildNotification("Preparing secure tunnel"))
        resetSnapshot("preparing")
        broadcastSnapshot()

        val staleDescriptor = tunnel.getAndSet(null)
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
            val descriptor =
                try {
                    establishVpn(profile, assignment, routePlan)
                } catch (error: Exception) {
                    fail(generation, "Android refused the VPN configuration: ${safeMessage(error)}")
                    return
                }
            if (descriptor == null) {
                fail(generation, "Android refused to create the VPN interface.")
                return
            }
            if (!isCurrent(generation)) {
                closeQuietly(descriptor)
                return
            }
            tunnel.set(descriptor)
            postPhase(generation, "connectingH3", null)
            val startResult =
                NativeEngine.start(
                    descriptor.fd,
                    profileJson,
                    secret,
                    this,
                )
            if (startResult != NativeEngine.OK) {
                tunnel.compareAndSet(descriptor, null)
                closeQuietly(descriptor)
                val failure = nativeStartFailure(startResult)
                fail(generation, failure.code, failure.message)
                return
            }
            if (!isCurrent(generation)) {
                NativeEngine.stop()
                tunnel.compareAndSet(descriptor, null)
                closeQuietly(descriptor)
                return
            }
            nativeRuntimeActive.set(true)
            mainHandler.post {
                if (isCurrent(generation)) {
                    killSwitchEnabled = profile.killSwitch
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
            val result = NativeEngine.startProxy(profileJson, secret, this)
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
                    killSwitchEnabled = false
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
        underlyingNetwork.get()?.let { network ->
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

    private fun disconnect(
        stopService: Boolean,
        request: Message? = null,
    ) {
        recoveryPreferences.edit().remove(RECOVERY_PROFILE).commit()
        val generation = connectionGeneration.incrementAndGet()
        networkRestartGeneration.incrementAndGet()
        activeProfileJson.set(null)
        val stoppedMode = activeMode.getAndSet(null)
        nativeRuntimeActive.set(false)
        statusTask?.cancel(false)
        statusTask = null
        captivePauseTask?.cancel(false)
        captivePauseTask = null
        captivePauseDeadlineUnixMillis = 0L
        NativeEngine.cancel()
        val descriptor = tunnel.getAndSet(null)
        closeQuietly(descriptor)
        resetSnapshot("disconnected")
        logStore.record(
            AndroidLogStore.Event.CONNECTION_STOPPED,
            phase = phase,
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
        networkRestartGeneration.incrementAndGet()
        activeProfileJson.set(null)
        activeMode.set(null)
        nativeRuntimeActive.set(false)
        statusTask?.cancel(false)
        statusTask = null
        captivePauseTask?.cancel(false)
        captivePauseTask = null
        captivePauseDeadlineUnixMillis = 0L
        phase = "disconnecting"
        warning = null
        broadcastSnapshot()
        NativeEngine.cancel()
        val descriptor = tunnel.getAndSet(null)
        closeQuietly(descriptor)
        stopExecutor.execute {
            NativeEngine.stop()
            mainHandler.post {
                if (connectionGeneration.get() == generation) {
                    resetSnapshot("disconnected")
                    broadcastSnapshot()
                    replyWithSnapshot(request)
                    stopForeground(STOP_FOREGROUND_REMOVE)
                    stopSelf()
                }
            }
        }
    }

    private fun selectUnderlyingNetwork() {
        val candidates =
            availableNetworks.entries
                .mapNotNull { (network, candidate) ->
                    val capabilities = candidate.capabilities ?: return@mapNotNull null
                    if (
                        candidate.blocked ||
                        !capabilities.hasCapability(
                            NetworkCapabilities.NET_CAPABILITY_INTERNET,
                        ) ||
                        !capabilities.hasCapability(
                            NetworkCapabilities.NET_CAPABILITY_NOT_VPN,
                        )
                    ) {
                        return@mapNotNull null
                    }
                    val familyMask = familyMask(candidate.linkProperties)
                    if (familyMask == 0) return@mapNotNull null
                    network to
                        PhysicalNetworkCandidate(
                            handle = network.networkHandle,
                            score = networkScore(capabilities),
                            familyMask = familyMask,
                        )
                }
        val current = underlyingNetwork.get()
        val selection =
            choosePhysicalNetwork(
                currentHandle = current?.networkHandle,
                candidates = candidates.map { it.second },
            )
        val selectedNetwork =
            candidates
                .firstOrNull { it.second.handle == selection?.handle }
                ?.first
        val selectedFamilyMask = selection?.familyMask ?: 0
        val previousNetwork = underlyingNetwork.getAndSet(selectedNetwork)
        val previousFamilyMask = underlyingFamilyMask.getAndSet(selectedFamilyMask)
        if (previousNetwork == selectedNetwork && previousFamilyMask == selectedFamilyMask) return
        val generation = networkRestartGeneration.incrementAndGet()
        NativeEngine.notifyNetworkChanged(generation)
        logStore.record(
            AndroidLogStore.Event.NETWORK_CHANGED,
            phase = phase,
            mode = activeMode.get(),
        )

        if (phase == "captivePortalPaused") {
            mainHandler.post(::resumeFromCaptivePortalPause)
            return
        }

        if (nativeRuntimeActive.get() || tunnel.get() != null) {
            if (tunnel.get() != null) {
                setUnderlyingNetworks(
                    selectedNetwork?.let { arrayOf(it) } ?: emptyArray(),
                )
            }
            mainHandler.post {
                if (nativeRuntimeActive.get() || tunnel.get() != null) {
                    phase = "reconnecting"
                    errorCode = null
                    warning =
                        if (selectedNetwork == null) {
                            "ANDROID_WAITING_FOR_PHYSICAL_NETWORK: waiting for a usable non-VPN network."
                        } else {
                            "The underlying Android network changed; rebuilding the secure channel."
                        }
                    updateNotification()
                    broadcastSnapshot()
                    ensureStatusTask()
                }
            }
        }
    }

    private fun scheduleUnderlyingNetworkSelection() {
        mainHandler.removeCallbacks(networkSelectionTask)
        mainHandler.postDelayed(networkSelectionTask, 100L)
    }

    private fun familyMask(linkProperties: LinkProperties?): Int {
        if (linkProperties == null) return FAMILY_IPV4 or FAMILY_IPV6
        var mask = 0
        for (route in linkProperties.routes) {
            if (!route.isDefaultRoute) continue
            when (route.destination.address) {
                is java.net.Inet4Address -> mask = mask or FAMILY_IPV4
                is java.net.Inet6Address -> mask = mask or FAMILY_IPV6
            }
        }
        return mask
    }

    private fun networkScore(capabilities: NetworkCapabilities): Int {
        var score =
            if (capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)) {
                100
            } else {
                0
            }
        score +=
            when {
                capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> 40
                capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> 30
                capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> 20
                else -> 10
            }
        return score
    }

    private fun awaitPhysicalNetwork(connectionToken: Long): Boolean {
        val deadline =
            System.nanoTime() +
                TimeUnit.MILLISECONDS.toNanos(PHYSICAL_NETWORK_WAIT_MILLIS)
        while (System.nanoTime() < deadline) {
            if (!isCurrent(connectionToken)) return false
            if (underlyingNetwork.get() != null) return true
            try {
                Thread.sleep(50)
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
                return false
            }
        }
        return underlyingNetwork.get() != null && isCurrent(connectionToken)
    }

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

    private fun pauseForCaptivePortal(request: Message) {
        val seconds = request.data.getInt("seconds", 0)
        val error =
            when {
                seconds !in 1..600 -> {
                    "Captive Portal Pause must be between 1 and 600 seconds."
                }

                activeMode.get() != "vpn" || tunnel.get() == null -> {
                    "Captive Portal Pause requires an active Android VPN."
                }

                Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && isLockdownEnabled -> {
                    "Android Block connections without VPN is enabled. Disable it in system settings before pausing."
                }

                else -> {
                    null
                }
            }
        if (error != null) {
            replyControlError(request, "CAPTIVE_PORTAL_PAUSE_UNAVAILABLE", error)
            return
        }

        val profileJson = activeProfileJson.get()
        if (profileJson == null) {
            replyControlError(
                request,
                "CAPTIVE_PORTAL_PAUSE_UNAVAILABLE",
                "The active VPN profile cannot be resumed safely.",
            )
            return
        }
        val generation = connectionGeneration.incrementAndGet()
        networkRestartGeneration.incrementAndGet()
        nativeRuntimeActive.set(false)
        statusTask?.cancel(false)
        statusTask = null
        captivePauseTask?.cancel(false)
        captivePauseDeadlineUnixMillis =
            System.currentTimeMillis() + TimeUnit.SECONDS.toMillis(seconds.toLong())
        phase = "captivePortalPaused"
        logStore.record(
            AndroidLogStore.Event.CAPTIVE_PORTAL_PAUSED,
            phase = phase,
            mode = activeMode.get(),
        )
        warning =
            "VPN and Kill Switch are paused temporarily for captive portal access."
        updateNotification()
        broadcastSnapshot()
        replyWithSnapshot(request)

        val descriptor = tunnel.getAndSet(null)
        stopExecutor.execute {
            NativeEngine.stop()
            closeQuietly(descriptor)
        }
        captivePauseTask =
            statusExecutor.scheduleWithFixedDelay(
                {
                    mainHandler.post {
                        if (
                            connectionGeneration.get() == generation &&
                            phase == "captivePortalPaused"
                        ) {
                            if (captivePauseRemainingSeconds() == 0) {
                                resumeFromCaptivePortalPause()
                            } else {
                                broadcastSnapshot()
                            }
                        }
                    }
                },
                1,
                1,
                TimeUnit.SECONDS,
            )
    }

    private fun resumeFromCaptivePortalPause() {
        if (phase != "captivePortalPaused") return
        val profileJson = activeProfileJson.get() ?: return
        captivePauseTask?.cancel(false)
        captivePauseTask = null
        captivePauseDeadlineUnixMillis = 0L
        logStore.record(
            AndroidLogStore.Event.CAPTIVE_PORTAL_RESUMED,
            phase = phase,
            mode = activeMode.get(),
        )
        beginConnection(profileJson)
    }

    private fun captivePauseRemainingSeconds(): Int {
        if (captivePauseDeadlineUnixMillis <= 0L) return 0
        val remaining = captivePauseDeadlineUnixMillis - System.currentTimeMillis()
        return if (remaining <= 0L) {
            0
        } else {
            ((remaining + 999L) / 1_000L).coerceAtMost(600L).toInt()
        }
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
        val previousPhase = phase
        phase = source.optString("phase", "error")
        warning = source.optNullableString("warning")
        errorCode = source.optNullableString("error_code")
        transport = source.optNullableString("transport")
        addressFamily = source.optNullableString("address_family")
        downloadBytesPerSecond = source.optLong("download_bytes_per_second", 0).coerceAtLeast(0)
        uploadBytesPerSecond = source.optLong("upload_bytes_per_second", 0).coerceAtLeast(0)
        downloadedBytes = source.optLong("downloaded_bytes", 0).coerceAtLeast(0)
        uploadedBytes = source.optLong("uploaded_bytes", 0).coerceAtLeast(0)
        reconnectCount = source.optInt("reconnect_count", 0).coerceAtLeast(0)
        activeListeners =
            source.optJSONArray("active_listeners")?.let { listeners ->
                List(listeners.length()) { index -> listeners.getString(index) }
            } ?: emptyList()
        exitIpv4 = source.optNullableString("exit_ipv4")
        exitIpv6 = source.optNullableString("exit_ipv6")
        exitCity = source.optNullableString("exit_city")
        exitCountry = source.optNullableString("exit_country")
        exitCountryCode = source.optNullableString("exit_country_code")
        val nativeFlag = source.optNullableString("exit_flag_svg")
        if (nativeFlag != null && nativeFlag != exitFlagSvg) {
            exitFlagSvg = nativeFlag
            val countryCode = exitCountryCode
            if (countryCode != null) {
                statusExecutor.execute {
                    try {
                        flagCache.put(countryCode, nativeFlag)
                    } catch (_: Exception) {
                        // A cache write failure is diagnostic-only.
                    }
                }
            }
        } else if (
            nativeFlag == null &&
            exitFlagSvg == null &&
            exitCountryCode != null &&
            flagCacheLookupCode != exitCountryCode
        ) {
            val countryCode = requireNotNull(exitCountryCode)
            flagCacheLookupCode = countryCode
            statusExecutor.execute {
                val cached = flagCache.get(countryCode)
                if (cached != null) {
                    mainHandler.post {
                        if (exitCountryCode == countryCode && exitFlagSvg == null) {
                            exitFlagSvg = cached
                            broadcastSnapshot()
                        }
                    }
                }
            }
        }
        if ((phase == "connected" || phase == "degraded") && connectedAt == null) {
            connectedAt = Instant.now().toString()
        }
        if (phase == "error") {
            // Keep the TUN open and fail closed until the user retries or disconnects.
            statusTask?.cancel(false)
            statusTask = null
        }
        if (phase != previousPhase) {
            logStore.record(
                AndroidLogStore.Event.CONNECTION_PHASE_CHANGED,
                phase = phase,
                mode = activeMode.get(),
                transport = transport,
            )
            updateNotification()
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
                phase = nextPhase
                warning = nextWarning
                errorCode = null
                logStore.record(
                    AndroidLogStore.Event.CONNECTION_PHASE_CHANGED,
                    phase = phase,
                    mode = activeMode.get(),
                    transport = transport,
                )
                updateNotification()
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
            phase = "error"
            errorCode = code
            logStore.record(
                AndroidLogStore.Event.CONNECTION_FAILED,
                phase = phase,
                mode = activeMode.get(),
            )
            warning = message.take(512)
            transport = null
            addressFamily = null
            updateNotification()
            broadcastSnapshot()
        }
    }

    private fun resetSnapshot(nextPhase: String) {
        phase = nextPhase
        warning = null
        errorCode = null
        transport = null
        addressFamily = null
        connectedAt = null
        downloadBytesPerSecond = 0
        uploadBytesPerSecond = 0
        downloadedBytes = 0
        uploadedBytes = 0
        reconnectCount = 0
        activeListeners = emptyList()
        exitIpv4 = null
        exitIpv6 = null
        exitCity = null
        exitCountry = null
        exitCountryCode = null
        exitFlagSvg = null
        flagCacheLookupCode = null
        killSwitchEnabled = false
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
        val fingerprint = snapshotFingerprint()
        if (fingerprint == lastBroadcastFingerprint) return
        lastBroadcastFingerprint = fingerprint
        val snapshot = snapshotBundle()
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
        Bundle().apply {
            putString("phase", phase)
            putString("warning", warning)
            putString("error_code", errorCode)
            putString("transport", transport)
            putString("address_family", addressFamily)
            putString("connected_at", connectedAt)
            putLong("download_bytes_per_second", downloadBytesPerSecond)
            putLong("upload_bytes_per_second", uploadBytesPerSecond)
            putLong("downloaded_bytes", downloadedBytes)
            putLong("uploaded_bytes", uploadedBytes)
            putInt("reconnect_count", reconnectCount)
            putStringArrayList("active_listeners", ArrayList(activeListeners))
            putString("exit_ipv4", exitIpv4)
            putString("exit_ipv6", exitIpv6)
            putString("exit_city", exitCity)
            putString("exit_country", exitCountry)
            putString("exit_country_code", exitCountryCode)
            putString("exit_flag_svg", exitFlagSvg)
            putString(
                "kill_switch_state",
                when {
                    phase == "captivePortalPaused" -> "paused"
                    killSwitchEnabled && tunnel.get() != null -> "active"
                    activeMode.get() == "vpn" -> "inactive"
                    else -> "notApplicable"
                },
            )
            putInt("captive_pause_remaining_seconds", captivePauseRemainingSeconds())
            putBoolean(
                "platform_lockdown",
                Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && isLockdownEnabled,
            )
            putBoolean(
                "always_on",
                Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && isAlwaysOn,
            )
        }

    private fun snapshotFingerprint(): String =
        listOf(
            phase,
            warning,
            errorCode,
            transport,
            addressFamily,
            connectedAt,
            downloadBytesPerSecond,
            uploadBytesPerSecond,
            downloadedBytes,
            uploadedBytes,
            reconnectCount,
            activeListeners.joinToString("\u001f"),
            exitIpv4,
            exitIpv6,
            exitCity,
            exitCountry,
            exitCountryCode,
            exitFlagSvg,
            killSwitchEnabled,
            tunnel.get() != null,
            activeMode.get(),
            captivePauseRemainingSeconds(),
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) isLockdownEnabled else false,
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) isAlwaysOn else false,
        ).joinToString("\u001e")

    private fun updateNotification() {
        val text =
            when (phase) {
                "preparing" -> {
                    "Preparing secure tunnel"
                }

                "connectingH3" -> {
                    "Connecting with HTTP/3"
                }

                "connectingH2" -> {
                    "Connecting with HTTP/2"
                }

                "connected" -> {
                    "Connected${transport?.let { " via ${it.uppercase()}" } ?: ""}"
                }

                "degraded" -> {
                    "Connected with reduced address-family support"
                }

                "reconnecting" -> {
                    "Reconnecting securely"
                }

                "captivePortalPaused" -> {
                    "VPN paused for captive portal (${captivePauseRemainingSeconds()} s)"
                }

                "error" -> {
                    "Network service stopped after an error"
                }

                "disconnecting" -> {
                    "Disconnecting"
                }

                else -> {
                    "Usque VPN"
                }
            }
        getSystemService(NotificationManager::class.java).notify(
            NOTIFICATION_ID,
            buildNotification(text),
        )
    }

    private fun buildNotification(status: String): Notification {
        val launchIntent = packageManager.getLaunchIntentForPackage(packageName)
        val contentIntent =
            PendingIntent.getActivity(
                this,
                0,
                launchIntent,
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        val disconnectIntent =
            PendingIntent.getService(
                this,
                1,
                Intent(this, UsqueVpnService::class.java).setAction(ACTION_DISCONNECT),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        return Notification
            .Builder(this, CHANNEL_ID)
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

    private fun isCurrent(generation: Long): Boolean = !destroyed && connectionGeneration.get() == generation

    @Keep
    fun getUnderlyingNetworkHandle(): Long = underlyingNetwork.get()?.networkHandle ?: 0L

    @Keep
    fun getUnderlyingFamilyMask(): Int = underlyingFamilyMask.get()

    @Keep
    fun getUnderlyingNetworkGeneration(): Long = networkRestartGeneration.get()

    @Keep
    fun resolveUnderlyingHost(host: String): Array<String> {
        val network = underlyingNetwork.get() ?: return emptyArray()
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

private fun JSONObject.optNullableString(name: String): String? =
    if (has(name) && !isNull(name)) optString(name).takeIf(String::isNotBlank) else null
