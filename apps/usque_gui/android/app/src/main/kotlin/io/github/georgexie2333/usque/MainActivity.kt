package io.github.georgexie2333.usque

import android.Manifest
import android.app.Activity
import android.app.StatusBarManager
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.drawable.Icon
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.os.PersistableBundle
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import io.flutter.embedding.android.FlutterFragmentActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodChannel
import java.io.File
import java.util.concurrent.Executors

/**
 * Flutter host: channel adapters, VPN permission, and document-picker results.
 * Engine method logic lives in [AndroidEngineMethodHandler]; Binder control
 * traffic lives in [VpnControlClient].
 */
class MainActivity : FlutterFragmentActivity() {
    internal companion object {
        const val CHANNEL = "io.github.georgexie2333.usque/engine"
        const val EVENT_CHANNEL = "io.github.georgexie2333.usque/engine_events"
        const val CREATE_DIAGNOSTICS_REQUEST = 1049
        const val ACTION_SHORTCUT_CONNECT = "io.github.georgexie2333.usque.SHORTCUT_CONNECT"
        const val ACTION_SHORTCUT_DISCONNECT = "io.github.georgexie2333.usque.SHORTCUT_DISCONNECT"
        const val ACTION_SHORTCUT_PROFILES = "io.github.georgexie2333.usque.SHORTCUT_PROFILES"
    }

    private val identityExecutor = Executors.newSingleThreadExecutor()
    private val identityStore by lazy { SecureIdentityStore(this) }
    private val profileConfigPath by lazy {
        File(noBackupFilesDir, "usque_config/profiles-v2.json").absolutePath
    }
    private val pendingVpnConnection = VpnPermissionRequestQueue()
    private var pendingDiagnosticsResult: MethodChannel.Result? = null
    private var pendingWarpSecretResult: MethodChannel.Result? = null
    private var pendingWarpSecretProfileId: String? = null
    private var pendingLaunchTarget: String? = null
    private val zeroTrustCallbackSession = ZeroTrustCallbackSession()
    private var eventSink: EventChannel.EventSink? = null

    private lateinit var controlClient: VpnControlClient
    private lateinit var methodHandler: AndroidEngineMethodHandler

    private val vpnPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { activityResult ->
            finishVpnPermissionRequest(activityResult.resultCode == Activity.RESULT_OK)
        }

    private val warpSecretDestinationLauncher =
        registerForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { destination ->
            finishWarpSecretExport(destination)
        }

    private val notificationPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) {
            // Notification permission never gates VPN or proxy connectivity.
        }

    private val activityCommands =
        object : AndroidEngineMethodHandler.ActivityCommands {
            override fun cancelPendingVpnConnection(
                code: String,
                message: String,
            ) {
                pendingVpnConnection.cancel(code, message)
            }

            override fun connectAfterValidation(
                profileJson: String,
                mode: String,
                result: MethodChannel.Result,
            ) {
                connectWithPermission(profileJson, mode, result)
            }

            override fun selectDiagnosticsDestination(result: MethodChannel.Result) {
                this@MainActivity.selectDiagnosticsDestination(result)
            }

            override fun selectWarpSecretDestination(
                profileId: String,
                result: MethodChannel.Result,
            ) {
                this@MainActivity.selectWarpSecretDestination(profileId, result)
            }

            override fun copySensitiveText(
                label: String,
                value: String,
            ) {
                val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                val clip = ClipData.newPlainText(label, value)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    clip.description.extras =
                        PersistableBundle().apply {
                            putBoolean("android.content.extra.IS_SENSITIVE", true)
                        }
                }
                clipboard.setPrimaryClip(clip)
            }

            override fun consumeLaunchTarget(): String? = pendingLaunchTarget.also { pendingLaunchTarget = null }

            override fun beginZeroTrustLogin(team: String): String = zeroTrustCallbackSession.begin(team)

            override fun consumeZeroTrustCallback(): String? = zeroTrustCallbackSession.consume()

            override fun cancelZeroTrustLogin() {
                zeroTrustCallbackSession.cancel()
            }

            override fun platformPreferences(): Map<String, Any?> {
                val preferences =
                    createDeviceProtectedStorageContext().getSharedPreferences(
                        UsqueVpnService.RECOVERY_PREFERENCES,
                        MODE_PRIVATE,
                    )
                return mapOf(
                    "start_on_boot" to
                        preferences.getBoolean(UsqueVpnService.START_ON_BOOT, false),
                    "close_to_tray" to true,
                )
            }

            override fun setStartOnBoot(enabled: Boolean) {
                createDeviceProtectedStorageContext()
                    .getSharedPreferences(UsqueVpnService.RECOVERY_PREFERENCES, MODE_PRIVATE)
                    .edit { putBoolean(UsqueVpnService.START_ON_BOOT, enabled) }
            }

            override fun requestAddQuickSettingsTile(result: MethodChannel.Result) {
                if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
                    result.success(null)
                    return
                }
                val statusBar = getSystemService(StatusBarManager::class.java)
                statusBar.requestAddTileService(
                    android.content.ComponentName(this@MainActivity, UsqueTileService::class.java),
                    "Usque",
                    Icon.createWithResource(this@MainActivity, R.drawable.ic_stat_usque),
                    mainExecutor,
                ) {
                    result.success(null)
                }
            }
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        // FlutterFragmentActivity may invoke configureFlutterEngine during
        // super.onCreate; wire control + method handlers first.
        ensureEngineComponents()
        super.onCreate(savedInstanceState)
        configureQuickSettingsTileAvailability()
        AndroidShortcutController.sync(this)
        handleIncomingIntent(intent)
    }

    private fun configureQuickSettingsTileAvailability() {
        val state =
            if (packageManager.hasSystemFeature(PackageManager.FEATURE_LEANBACK)) {
                PackageManager.COMPONENT_ENABLED_STATE_DISABLED
            } else {
                PackageManager.COMPONENT_ENABLED_STATE_DEFAULT
            }
        packageManager.setComponentEnabledSetting(
            ComponentName(this, UsqueTileService::class.java),
            state,
            PackageManager.DONT_KILL_APP,
        )
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleIncomingIntent(intent)
    }

    private fun handleIncomingIntent(intent: Intent?) {
        handleZeroTrustCallbackIntent(intent)
        handleShortcutIntent(intent)
    }

    private fun handleZeroTrustCallbackIntent(intent: Intent?) {
        if (intent?.action != Intent.ACTION_VIEW) return
        val callback = intent.data ?: return
        // Never retain the Access assertion on the Activity's launch Intent.
        intent.data = null
        zeroTrustCallbackSession.accept(callback.toString())
    }

    private fun handleShortcutIntent(intent: Intent?) {
        when (intent?.action) {
            ACTION_SHORTCUT_CONNECT -> {
                ContextCompat.startForegroundService(
                    this,
                    Intent(this, UsqueVpnService::class.java)
                        .setAction(UsqueVpnService.ACTION_CONNECT_LAST),
                )
            }

            ACTION_SHORTCUT_DISCONNECT -> {
                ContextCompat.startForegroundService(
                    this,
                    Intent(this, UsqueVpnService::class.java)
                        .setAction(UsqueVpnService.ACTION_DISCONNECT),
                )
            }

            ACTION_SHORTCUT_PROFILES -> {
                pendingLaunchTarget = "profiles"
            }
        }
    }

    private fun ensureEngineComponents() {
        if (::controlClient.isInitialized && ::methodHandler.isInitialized) {
            return
        }
        controlClient = VpnControlClient.create(this)
        controlClient.eventListener =
            VpnControlClient.EventListener { snapshot ->
                eventSink?.success(snapshot)
            }
        methodHandler =
            AndroidEngineMethodHandler(
                profileConfigPath = profileConfigPath,
                identityStore = AndroidEngineMethodHandler.SecureIdentityStoreAdapter(identityStore),
                identityExecutor = identityExecutor,
                mainScheduler = VpnControlClient.HandlerMainScheduler(android.os.Handler(mainLooper)),
                controlClient = controlClient,
                activityCommands = activityCommands,
                maintenanceBridge = AndroidEngineMethodHandler.AndroidMaintenanceAdapter(this),
            )
        controlClient.clearAllAcknowledgedListener =
            VpnControlClient.ClearAllAcknowledgedListener { result ->
                methodHandler.finishClearAllData(result)
            }
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        ensureEngineComponents()
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, CHANNEL)
            .setMethodCallHandler { call, result ->
                methodHandler.handle(call, result)
            }
        EventChannel(flutterEngine.dartExecutor.binaryMessenger, EVENT_CHANNEL)
            .setStreamHandler(
                object : EventChannel.StreamHandler {
                    override fun onListen(
                        arguments: Any?,
                        events: EventChannel.EventSink,
                    ) {
                        eventSink = events
                        controlClient.setEventsWanted(true)
                    }

                    override fun onCancel(arguments: Any?) {
                        controlClient.setEventsWanted(false)
                        eventSink = null
                    }
                },
            )
    }

    override fun onStart() {
        super.onStart()
        ensureEngineComponents()
        controlClient.bind()
    }

    override fun onDestroy() {
        pendingVpnConnection.cancel(
            "VPN_PERMISSION_CANCELLED",
            "The Android UI closed before VPN permission was granted.",
        )
        pendingDiagnosticsResult?.error(
            "DIAGNOSTICS_CANCELLED",
            "The Android UI closed before the diagnostic bundle was saved.",
            null,
        )
        pendingDiagnosticsResult = null
        pendingWarpSecretResult?.error(
            "SENSITIVE_OUTPUT_CANCELLED",
            "The Android UI closed before the WARP Secret was saved.",
            null,
        )
        pendingWarpSecretResult = null
        pendingWarpSecretProfileId = null
        eventSink = null
        if (::controlClient.isInitialized) {
            controlClient.destroy()
        }
        identityExecutor.shutdownNow()
        super.onDestroy()
    }

    @Deprecated("The Storage Access Framework result is bridged to Flutter.")
    override fun onActivityResult(
        requestCode: Int,
        resultCode: Int,
        data: Intent?,
    ) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != CREATE_DIAGNOSTICS_REQUEST) return
        val result = pendingDiagnosticsResult ?: return
        pendingDiagnosticsResult = null
        if (resultCode != Activity.RESULT_OK) {
            result.success(null)
            return
        }
        val destination = data?.data
        if (destination == null) {
            result.error(
                "DIAGNOSTICS_DESTINATION_FAILED",
                "The Android document provider returned no destination.",
                null,
            )
            return
        }
        ensureEngineComponents()
        val snapshot = controlClient.lastSnapshot.toMap()
        val mainHandler = android.os.Handler(mainLooper)
        identityExecutor.execute {
            try {
                AndroidMaintenance.writeDiagnostics(this, destination, snapshot)
                mainHandler.post { result.success(destination.toString()) }
            } catch (error: Exception) {
                mainHandler.post {
                    result.error(
                        "DIAGNOSTICS_EXPORT_FAILED",
                        "Android could not write the diagnostic bundle.",
                        error.javaClass.simpleName,
                    )
                }
            }
        }
    }

    private fun selectDiagnosticsDestination(result: MethodChannel.Result) {
        if (pendingDiagnosticsResult != null) {
            result.error(
                "DIAGNOSTICS_IN_PROGRESS",
                "Another diagnostic export is already waiting for a destination.",
                null,
            )
            return
        }
        pendingDiagnosticsResult = result
        val intent =
            Intent(Intent.ACTION_CREATE_DOCUMENT)
                .addCategory(Intent.CATEGORY_OPENABLE)
                .setType("application/zip")
                .putExtra(Intent.EXTRA_TITLE, "usque-diagnostics.zip")
        try {
            @Suppress("DEPRECATION")
            startActivityForResult(intent, CREATE_DIAGNOSTICS_REQUEST)
        } catch (error: Exception) {
            pendingDiagnosticsResult = null
            result.error(
                "DIAGNOSTICS_DESTINATION_FAILED",
                "No Android document provider is available.",
                error.javaClass.simpleName,
            )
        }
    }

    private fun selectWarpSecretDestination(
        profileId: String,
        result: MethodChannel.Result,
    ) {
        if (pendingWarpSecretResult != null) {
            result.error(
                "SENSITIVE_OUTPUT_IN_PROGRESS",
                "Another WARP Secret export is already waiting for a destination.",
                null,
            )
            return
        }
        pendingWarpSecretResult = result
        pendingWarpSecretProfileId = profileId
        try {
            warpSecretDestinationLauncher.launch("usque-warp-secret.json")
        } catch (error: Exception) {
            pendingWarpSecretResult = null
            pendingWarpSecretProfileId = null
            result.error(
                "SENSITIVE_OUTPUT_FAILED",
                "No Android document provider is available.",
                error.javaClass.simpleName,
            )
        }
    }

    private fun finishWarpSecretExport(destination: android.net.Uri?) {
        val result = pendingWarpSecretResult ?: return
        val profileId = pendingWarpSecretProfileId
        pendingWarpSecretResult = null
        pendingWarpSecretProfileId = null
        if (destination == null) {
            result.success(null)
            return
        }
        if (profileId == null) {
            result.error("SENSITIVE_OUTPUT_FAILED", "The Profile identity is unavailable.", null)
            return
        }
        val mainHandler = android.os.Handler(mainLooper)
        identityExecutor.execute {
            var secret: ByteArray? = null
            try {
                secret =
                    identityStore.get(profileId, SecureIdentityStore.Record.WARP_SECRET)
                        ?: throw IllegalStateException("The Profile identity is missing")
                contentResolver.openOutputStream(destination, "wt").use { output ->
                    checkNotNull(output) { "The document provider returned no output stream" }
                    output.write(secret)
                    output.flush()
                }
                mainHandler.post { result.success(destination.toString()) }
            } catch (error: Exception) {
                mainHandler.post {
                    result.error(
                        "SENSITIVE_OUTPUT_FAILED",
                        "Android could not save the WARP Secret.",
                        error.javaClass.simpleName,
                    )
                }
            } finally {
                secret?.fill(0)
            }
        }
    }

    private fun connectWithPermission(
        profileJson: String,
        mode: String,
        result: MethodChannel.Result,
    ) {
        maybeRequestNotificationPermission()
        if (pendingVpnConnection.hasPending) {
            result.error(
                "VPN_PERMISSION_IN_PROGRESS",
                "Another VPN permission request is already in progress.",
                null,
            )
            return
        }
        if (mode == "vpn") {
            val permissionIntent =
                try {
                    VpnService.prepare(this)
                } catch (error: Exception) {
                    result.error(
                        "VPN_PERMISSION_LAUNCH_FAILED",
                        "Android could not prepare the VPN permission request.",
                        error.javaClass.simpleName,
                    )
                    return
                }
            if (permissionIntent != null) {
                check(pendingVpnConnection.offer(profileJson, result))
                try {
                    vpnPermissionLauncher.launch(permissionIntent)
                } catch (error: Exception) {
                    val pending = pendingVpnConnection.take()
                    pending?.result?.error(
                        "VPN_PERMISSION_LAUNCH_FAILED",
                        "Android could not open the VPN permission dialog.",
                        error.javaClass.simpleName,
                    )
                }
                return
            }
        }
        startNetworkService(profileJson, mode, result)
    }

    private fun maybeRequestNotificationPermission() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
        if (
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        val preferences = getSharedPreferences("usque_ui_permissions", MODE_PRIVATE)
        if (preferences.getBoolean("notification_requested", false)) return
        preferences.edit { putBoolean("notification_requested", true) }
        runCatching { notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS) }
    }

    private fun finishVpnPermissionRequest(granted: Boolean) {
        val pending = pendingVpnConnection.take() ?: return
        if (!granted) {
            pending.result.error(
                "VPN_PERMISSION_DENIED",
                "VPN permission was not granted.",
                null,
            )
            return
        }
        val permissionStillRequired =
            try {
                VpnService.prepare(this) != null
            } catch (error: Exception) {
                pending.result.error(
                    "VPN_PERMISSION_LAUNCH_FAILED",
                    "Android could not verify VPN permission.",
                    error.javaClass.simpleName,
                )
                return
            }
        if (permissionStillRequired) {
            pending.result.error(
                "VPN_PERMISSION_DENIED",
                "Android did not grant VPN permission.",
                null,
            )
            return
        }
        startNetworkService(pending.profileJson, "vpn", pending.result)
    }

    private fun startNetworkService(
        profileJson: String,
        mode: String,
        result: MethodChannel.Result,
    ) {
        val intent =
            Intent(this, UsqueVpnService::class.java)
                .setAction(UsqueVpnService.ACTION_CONNECT)
                .putExtra(UsqueVpnService.EXTRA_PROFILE_JSON, profileJson)
        try {
            ContextCompat.startForegroundService(this, intent)
        } catch (error: Exception) {
            result.error(
                "ENGINE_START_FAILED",
                "Android could not start the network service.",
                error.javaClass.simpleName,
            )
            return
        }
        result.success(
            mapOf(
                "phase" to "preparing",
                "warning" to
                    if (mode == "vpn") {
                        "Waiting for the native VPN engine."
                    } else {
                        "Starting the local proxy service."
                    },
            ),
        )
    }
}
