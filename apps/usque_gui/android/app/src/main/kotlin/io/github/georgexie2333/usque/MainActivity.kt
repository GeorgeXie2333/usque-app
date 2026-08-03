package io.github.georgexie2333.usque

import android.app.Activity
import android.content.Intent
import android.net.VpnService
import android.os.Bundle
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
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
    private companion object {
        const val CHANNEL = "io.github.georgexie2333.usque/engine"
        const val EVENT_CHANNEL = "io.github.georgexie2333.usque/engine_events"
        const val CREATE_DIAGNOSTICS_REQUEST = 1049
    }

    private val identityExecutor = Executors.newSingleThreadExecutor()
    private val identityStore by lazy { SecureIdentityStore(this) }
    private val profileConfigPath by lazy {
        File(noBackupFilesDir, "usque_config/profiles-v2.json").absolutePath
    }
    private val pendingVpnConnection = VpnPermissionRequestQueue()
    private var pendingDiagnosticsResult: MethodChannel.Result? = null
    private var eventSink: EventChannel.EventSink? = null

    private lateinit var controlClient: VpnControlClient
    private lateinit var methodHandler: AndroidEngineMethodHandler

    private val vpnPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { activityResult ->
            finishVpnPermissionRequest(activityResult.resultCode == Activity.RESULT_OK)
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
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        // FlutterFragmentActivity may invoke configureFlutterEngine during
        // super.onCreate; wire control + method handlers first.
        ensureEngineComponents()
        super.onCreate(savedInstanceState)
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

    private fun connectWithPermission(
        profileJson: String,
        mode: String,
        result: MethodChannel.Result,
    ) {
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
