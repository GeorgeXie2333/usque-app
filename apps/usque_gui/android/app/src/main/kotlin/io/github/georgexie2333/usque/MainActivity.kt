package io.github.georgexie2333.usque

import android.app.Activity
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.net.VpnService
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import android.os.RemoteException
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import io.flutter.embedding.android.FlutterFragmentActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.Locale
import java.util.concurrent.Executors

class MainActivity : FlutterFragmentActivity() {
    private companion object {
        const val CHANNEL = "io.github.georgexie2333.usque/engine"
        const val EVENT_CHANNEL = "io.github.georgexie2333.usque/engine_events"
        const val SNAPSHOT_TIMEOUT_MILLIS = 2_000L
        const val CLEAR_ALL_TIMEOUT_MILLIS = 45_000L
        const val CREATE_DIAGNOSTICS_REQUEST = 1049
        const val DEFAULT_IDENTITY_PROFILE = "8c30b771-9ebd-457a-b67b-bbc74a1ddba6"
    }

    private val mainHandler = Handler(Looper.getMainLooper())
    private val identityExecutor = Executors.newSingleThreadExecutor()
    private val identityStore by lazy { SecureIdentityStore(this) }
    private val profileConfigPath by lazy {
        File(noBackupFilesDir, "usque_config/profiles-v2.json").absolutePath
    }
    private val pendingSnapshots = mutableMapOf<Int, MethodChannel.Result>()
    private val pendingClearAll = mutableMapOf<Int, MethodChannel.Result>()
    private var nextSnapshotId = 1
    private var controlMessenger: Messenger? = null
    private var controlBound = false
    private var eventSink: EventChannel.EventSink? = null
    private var pendingDiagnosticsResult: MethodChannel.Result? = null
    private var pendingDisconnectResult: MethodChannel.Result? = null
    private val pendingVpnConnection = VpnPermissionRequestQueue()
    private var lastSnapshot: Map<String, Any?> = disconnectedSnapshot()
    private val vpnPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { activityResult ->
            finishVpnPermissionRequest(activityResult.resultCode == Activity.RESULT_OK)
        }

    private val replyMessenger =
        Messenger(
            Handler(Looper.getMainLooper()) { message ->
                when (message.what) {
                    UsqueVpnService.MSG_SNAPSHOT -> {
                        val clearResult = pendingClearAll.remove(message.arg1)
                        if (clearResult != null) {
                            val errorCode = message.data.getString("control_error_code")
                            if (errorCode != null) {
                                clearResult.error(
                                    errorCode,
                                    message.data.getString("control_error_message")
                                        ?: "The Android VPN process rejected the operation.",
                                    null,
                                )
                            } else {
                                finishClearAllData(clearResult)
                            }
                            return@Handler true
                        }
                        val result = pendingSnapshots.remove(message.arg1) ?: return@Handler true
                        val errorCode = message.data.getString("control_error_code")
                        if (errorCode != null) {
                            result.error(
                                errorCode,
                                message.data.getString("control_error_message")
                                    ?: "The Android VPN process rejected the operation.",
                                null,
                            )
                        } else {
                            result.success(snapshotFromBundle(message.data))
                        }
                        true
                    }
                    UsqueVpnService.MSG_EVENT -> {
                        eventSink?.success(snapshotFromBundle(message.data))
                        true
                    }
                    else -> false
                }
            },
        )

    private val controlConnection =
        object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
                controlMessenger = binder?.let(::Messenger)
                if (eventSink != null) registerForEvents()
                pendingDisconnectResult?.let { result ->
                    pendingDisconnectResult = null
                    requestDisconnect(result)
                }
            }

            override fun onServiceDisconnected(name: ComponentName?) {
                controlMessenger = null
            }

            override fun onBindingDied(name: ComponentName?) {
                controlMessenger = null
                if (controlBound) {
                    unbindService(this)
                    controlBound = false
                }
                bindControlService()
            }

            override fun onNullBinding(name: ComponentName?) {
                controlMessenger = null
            }
        }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, CHANNEL)
            .setMethodCallHandler(::handleEngineCall)
        EventChannel(flutterEngine.dartExecutor.binaryMessenger, EVENT_CHANNEL)
            .setStreamHandler(
                object : EventChannel.StreamHandler {
                    override fun onListen(arguments: Any?, events: EventChannel.EventSink) {
                        eventSink = events
                        registerForEvents()
                    }

                    override fun onCancel(arguments: Any?) {
                        unregisterForEvents()
                        eventSink = null
                    }
                },
            )
    }

    override fun onStart() {
        super.onStart()
        bindControlService()
    }

    override fun onDestroy() {
        pendingVpnConnection.cancel(
            "VPN_PERMISSION_CANCELLED",
            "The Android UI closed before VPN permission was granted.",
        )
        pendingSnapshots.values.forEach { result ->
            result.error(
                "ENGINE_IPC_CLOSED",
                "The Android UI closed before the VPN process replied.",
                null,
            )
        }
        pendingSnapshots.clear()
        pendingClearAll.values.forEach { result ->
            result.error(
                "CLEAR_ALL_CANCELLED",
                "The Android UI closed before local data could be cleared.",
                null,
            )
        }
        pendingClearAll.clear()
        pendingDiagnosticsResult?.error(
            "DIAGNOSTICS_CANCELLED",
            "The Android UI closed before the diagnostic bundle was saved.",
            null,
        )
        pendingDiagnosticsResult = null
        pendingDisconnectResult?.error(
            "ENGINE_IPC_CLOSED",
            "The Android UI closed before the connection could be stopped.",
            null,
        )
        pendingDisconnectResult = null
        unregisterForEvents()
        eventSink = null
        if (controlBound) {
            unbindService(controlConnection)
            controlBound = false
        }
        controlMessenger = null
        identityExecutor.shutdownNow()
        super.onDestroy()
    }

    private fun handleEngineCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "snapshot" -> requestSnapshot(result)
            "disconnect" -> {
                pendingVpnConnection.cancel(
                    "VPN_PERMISSION_CANCELLED",
                    "The VPN connection request was cancelled.",
                )
                requestDisconnect(result)
            }
            "connect" -> connect(call, result)
            "provisionIdentity" -> provisionIdentity(call, result)
            "createProfileWithIdentity" -> createProfileWithIdentity(call, result)
            "importLegacyProfiles" -> importLegacyProfiles(call, result)
            "upsertProfile" -> upsertProfile(call, result)
            "deleteProfile" -> deleteProfile(call, result)
            "setActiveProfile" -> setActiveProfile(call, result)
            "pauseCaptivePortal" -> pauseCaptivePortal(call, result)
            "exportDiagnostics" -> selectDiagnosticsDestination(result)
            "checkForUpdates" -> checkForUpdates(call, result)
            "clearAllData" -> clearAllData(call, result)
            else -> result.notImplemented()
        }
    }

    private fun importLegacyProfiles(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val arguments = call.arguments as? Map<*, *>
        val profiles = arguments?.get("profiles") as? List<*>
        val activeProfileId = arguments?.get("active_profile_id") as? String
        if (profiles == null || activeProfileId == null) {
            result.error(
                "INVALID_ARGUMENT",
                "The legacy profile catalog is malformed.",
                null,
            )
            return
        }
        runProfileCommand(
            JSONObject()
                .put("command", "import_legacy_profiles")
                .put("profiles", JSONArray(profiles))
                .put("active_profile_id", activeProfileId),
            result,
            returnCatalog = true,
        )
    }

    private fun upsertProfile(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val profile = call.arguments as? Map<*, *>
        if (profile == null) {
            result.error("INVALID_ARGUMENT", "The profile is malformed.", null)
            return
        }
        runProfileCommand(
            JSONObject()
                .put("command", "upsert_profile")
                .put("profile", JSONObject(profile)),
            result,
        )
    }

    private fun deleteProfile(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val profileId = call.argument<String>("profile_id")
        if (profileId == null) {
            result.error("INVALID_ARGUMENT", "The profile ID is missing.", null)
            return
        }
        runProfileCommand(
            JSONObject()
                .put("command", "delete_profile")
                .put("profile_id", profileId),
            result,
        )
    }

    private fun setActiveProfile(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val profileId = call.argument<String>("profile_id")
        if (profileId == null) {
            result.error("INVALID_ARGUMENT", "The profile ID is missing.", null)
            return
        }
        runProfileCommand(
            JSONObject()
                .put("command", "set_active_profile")
                .put("profile_id", profileId),
            result,
        )
    }

    private fun runProfileCommand(
        command: JSONObject,
        result: MethodChannel.Result,
        returnCatalog: Boolean = false,
    ) {
        if (!NativeEngine.isLinked()) {
            result.error(
                "ENGINE_UNAVAILABLE",
                "The Rust profile store is not linked in this build.",
                null,
            )
            return
        }
        identityExecutor.execute {
            try {
                var response =
                    NativeEngine.applyProfileCommand(profileConfigPath, command.toString())
                        ?: throw IllegalStateException("Rust returned no profile catalog")
                var responseObject = JSONObject(response)
                val pending = responseObject.optJSONArray("pending_identity_deletions")
                if (pending != null && pending.length() > 0) {
                    val completed = JSONArray()
                    for (index in 0 until pending.length()) {
                        val profileId = pending.getString(index)
                        identityStore.deleteIdentity(profileId)
                        completed.put(profileId)
                    }
                    response =
                        NativeEngine.applyProfileCommand(
                            profileConfigPath,
                            JSONObject()
                                .put("command", "complete_identity_deletions")
                                .put("profile_ids", completed)
                                .toString(),
                        ) ?: throw IllegalStateException("Rust did not acknowledge identity cleanup")
                    responseObject = JSONObject(response)
                }
                val pendingCreations = responseObject.optJSONArray("pending_identity_creations")
                if (pendingCreations != null && pendingCreations.length() > 0) {
                    val completed = JSONArray()
                    for (index in 0 until pendingCreations.length()) {
                        val profileId = pendingCreations.getString(index)
                        identityStore.deleteIdentity(profileId)
                        completed.put(profileId)
                    }
                    response =
                        NativeEngine.applyProfileCommand(
                            profileConfigPath,
                            JSONObject()
                                .put("command", "complete_identity_creations")
                                .put("profile_ids", completed)
                                .toString(),
                        ) ?: throw IllegalStateException("Rust did not acknowledge identity rollback")
                    responseObject = JSONObject(response)
                }
                val catalog =
                    if (returnCatalog) {
                        appendIdentityStatuses(responseObject)
                        jsonObjectToFlutterMap(responseObject)
                    } else {
                        null
                    }
                mainHandler.post { result.success(catalog) }
            } catch (error: Exception) {
                mainHandler.post {
                    result.error(
                        "PROFILE_STORE_FAILED",
                        "The Rust profile store rejected this operation.",
                        error.javaClass.simpleName,
                    )
                }
            }
        }
    }

    private fun jsonObjectToFlutterMap(source: JSONObject): Map<String, Any?> =
        source.keys().asSequence().associateWith { key ->
            jsonValueToFlutter(source.get(key))
        }

    private fun jsonValueToFlutter(value: Any?): Any? =
        when (value) {
            null, JSONObject.NULL -> null
            is JSONObject -> jsonObjectToFlutterMap(value)
            is JSONArray ->
                List(value.length()) { index ->
                    jsonValueToFlutter(value.get(index))
                }
            is Boolean, is Int, is Long, is Double, is String -> value
            is Number -> value.toDouble()
            else -> value.toString()
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
        val snapshot = lastSnapshot.toMap()
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

    private fun checkForUpdates(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        if (!NativeEngine.isLinked()) {
            result.error(
                "ENGINE_UNAVAILABLE",
                "The Rust update checker is not linked in this build.",
                null,
            )
            return
        }
        val manual = call.argument<Boolean>("manual") ?: true
        identityExecutor.execute {
            try {
                val update = AndroidMaintenance.checkForUpdates(this, manual)
                mainHandler.post { result.success(update) }
            } catch (error: Exception) {
                mainHandler.post {
                    result.error(
                        "UPDATE_CHECK_FAILED",
                        "The GitHub prerelease update check failed.",
                        error.javaClass.simpleName,
                    )
                }
            }
        }
    }

    private fun clearAllData(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        if (call.argument<Boolean>("confirmed") != true) {
            result.error(
                "CONFIRMATION_REQUIRED",
                "Clear All Data requires an explicit confirmation.",
                null,
            )
            return
        }
        pendingVpnConnection.cancel(
            "VPN_PERMISSION_CANCELLED",
            "The VPN connection request was cancelled while clearing local data.",
        )
        val service = controlMessenger
        if (service == null) {
            bindControlService()
            result.error(
                "ENGINE_IPC_UNAVAILABLE",
                "The Android network process is not ready. Try again.",
                null,
            )
            return
        }
        val requestId = nextSnapshotId
        nextSnapshotId = if (nextSnapshotId == Int.MAX_VALUE) 1 else nextSnapshotId + 1
        pendingClearAll[requestId] = result
        try {
            service.send(
                Message.obtain(null, UsqueVpnService.MSG_CLEAR_ALL_DATA).apply {
                    arg1 = requestId
                    replyTo = replyMessenger
                    data = Bundle().apply { putBoolean("confirmed", true) }
                },
            )
        } catch (_: RemoteException) {
            pendingClearAll.remove(requestId)
            controlMessenger = null
            result.error(
                "ENGINE_IPC_UNAVAILABLE",
                "The Android network process could not receive the clear request.",
                null,
            )
            return
        }
        mainHandler.postDelayed(
            {
                pendingClearAll.remove(requestId)?.error(
                    "ENGINE_IPC_TIMEOUT",
                    "The Android network process did not disconnect in time.",
                    null,
                )
            },
            CLEAR_ALL_TIMEOUT_MILLIS,
        )
    }

    private fun finishClearAllData(result: MethodChannel.Result) {
        identityExecutor.execute {
            try {
                identityStore.clearAll()
                NativeEngine.applyProfileCommand(
                    profileConfigPath,
                    JSONObject().put("command", "clear_all_data").toString(),
                ) ?: throw IllegalStateException("Rust did not reset the Profile store")
                AndroidMaintenance.clearLocalState(this)
                mainHandler.post { result.success(null) }
            } catch (error: Exception) {
                mainHandler.post {
                    result.error(
                        "CLEAR_ALL_FAILED",
                        "Android could not clear all local Usque data.",
                        error.javaClass.simpleName,
                    )
                }
            }
        }
    }

    private fun provisionIdentity(call: MethodCall, result: MethodChannel.Result) {
        if (call.argument<Boolean>("terms_accepted") != true) {
            result.error(
                "TERMS_NOT_ACCEPTED",
                "Cloudflare terms must be accepted before Consumer WARP registration.",
                null,
            )
            return
        }
        val profileId = call.argument<String>("profile_id") ?: DEFAULT_IDENTITY_PROFILE
        val secret = call.argument<String>("warp_secret")
        if (!NativeEngine.isLinked()) {
            result.error(
                "ENGINE_UNAVAILABLE",
                "The Rust identity engine is not linked in this build.",
                null,
            )
            return
        }

        identityExecutor.execute {
            val bytes =
                try {
                    if (secret.isNullOrBlank()) {
                        val locale =
                            call.argument<String>("locale")
                                ?.replace('-', '_')
                                ?.takeIf { it.isNotBlank() }
                                ?: Locale.getDefault().toString()
                        NativeEngine.registerConsumerWarp(locale)
                            ?: throw IllegalStateException("Rust registration returned no identity")
                    } else {
                        secret.toByteArray(Charsets.UTF_8).also { candidate ->
                            if (NativeEngine.validateWarpSecret(candidate) != NativeEngine.OK) {
                                candidate.fill(0)
                                throw IllegalArgumentException("Invalid WARP Secret")
                            }
                        }
                    }
                } catch (error: Exception) {
                    mainHandler.post {
                        result.error(
                            if (secret.isNullOrBlank()) {
                                "REGISTRATION_FAILED"
                            } else {
                                "INVALID_WARP_SECRET"
                            },
                            if (secret.isNullOrBlank()) {
                                "Consumer WARP registration failed. Check the network and try again."
                            } else {
                                "The WARP Secret is malformed or contains unsupported identity material."
                            },
                            error.javaClass.simpleName,
                        )
                    }
                    return@execute
                }

            try {
                identityStore.put(
                    profileId,
                    SecureIdentityStore.Record.WARP_SECRET,
                    bytes,
                )
                mainHandler.post { result.success(null) }
            } catch (error: Exception) {
                mainHandler.post {
                    result.error(
                        "SECURE_STORAGE_FAILED",
                        "Android Keystore could not persist the WARP identity.",
                        error.javaClass.simpleName,
                    )
                }
            } finally {
                bytes.fill(0)
            }
        }
    }

    private fun createProfileWithIdentity(call: MethodCall, result: MethodChannel.Result) {
        val arguments = call.arguments as? Map<*, *>
        val profile = arguments?.get("profile") as? Map<*, *>
        val profileId = profile?.get("id") as? String
        val method = arguments?.get("method") as? String
        val secret = arguments?.get("warp_secret") as? String
        if (
            profile == null ||
                profileId.isNullOrBlank() ||
                method !in setOf("register", "importSecret") ||
                arguments?.get("terms_accepted") != true
        ) {
            result.error("INVALID_ARGUMENT", "The profile identity request is malformed.", null)
            return
        }
        if (!NativeEngine.isLinked()) {
            result.error("ENGINE_UNAVAILABLE", "The Rust identity engine is not linked.", null)
            return
        }

        identityExecutor.execute {
            var prepared = false
            var stored = false
            var bytes: ByteArray? = null
            try {
                NativeEngine.applyProfileCommand(
                    profileConfigPath,
                    JSONObject()
                        .put("command", "begin_identity_creation")
                        .put("profile_id", profileId)
                        .toString(),
                ) ?: throw IllegalStateException("Rust did not prepare profile creation")
                prepared = true

                val provisionedIdentity =
                    if (method == "register") {
                        if (!secret.isNullOrBlank()) {
                            throw IllegalArgumentException("Registration must not contain a Secret")
                        }
                        val locale =
                            (arguments["locale"] as? String)
                                ?.replace('-', '_')
                                ?.takeIf { it.isNotBlank() }
                                ?: Locale.getDefault().toString()
                        NativeEngine.registerConsumerWarp(locale)
                            ?: throw IllegalStateException("Rust registration returned no identity")
                    } else {
                        val value = secret?.takeIf { it.isNotBlank() }
                            ?: throw IllegalArgumentException("A WARP Secret is required")
                        value.toByteArray(Charsets.UTF_8).also { candidate ->
                            if (NativeEngine.validateWarpSecret(candidate) != NativeEngine.OK) {
                                candidate.fill(0)
                                throw IllegalArgumentException("Invalid WARP Secret")
                            }
                        }
                    }

                bytes = provisionedIdentity
                identityStore.put(
                    profileId,
                    SecureIdentityStore.Record.WARP_SECRET,
                    provisionedIdentity,
                )
                stored = true
                val response =
                    NativeEngine.applyProfileCommand(
                        profileConfigPath,
                        JSONObject()
                            .put("command", "commit_profile_with_identity")
                            .put("profile", JSONObject(profile))
                            .toString(),
                    ) ?: throw IllegalStateException("Rust did not commit the profile")
                val responseObject = JSONObject(response)
                appendIdentityStatuses(responseObject)
                val catalog = jsonObjectToFlutterMap(responseObject)
                mainHandler.post { result.success(catalog) }
            } catch (error: Exception) {
                if (stored) {
                    runCatching { identityStore.deleteIdentity(profileId) }
                }
                if (prepared) {
                    runCatching {
                        NativeEngine.applyProfileCommand(
                            profileConfigPath,
                            JSONObject()
                                .put("command", "complete_identity_creations")
                                .put("profile_ids", JSONArray().put(profileId))
                                .toString(),
                        )
                    }
                }
                val code =
                    when {
                        method == "register" && error !is IllegalArgumentException ->
                            "REGISTRATION_FAILED"
                        error is IllegalArgumentException -> "INVALID_WARP_SECRET"
                        else -> "PROFILE_STORE_FAILED"
                    }
                mainHandler.post {
                    result.error(
                        code,
                        when (code) {
                            "REGISTRATION_FAILED" ->
                                "Consumer WARP registration failed. Check the network and try again."
                            "INVALID_WARP_SECRET" ->
                                "The WARP Secret is malformed or missing."
                            else -> "The profile and its identity could not be saved safely."
                        },
                        error.javaClass.simpleName,
                    )
                }
            } finally {
                bytes?.fill(0)
            }
        }
    }

    private fun appendIdentityStatuses(catalog: JSONObject) {
        val statuses = JSONArray()
        val profiles = catalog.optJSONArray("profiles") ?: JSONArray()
        for (index in 0 until profiles.length()) {
            val profileId = profiles.getJSONObject(index).optString("id")
            val state =
                if (profileId.isBlank()) {
                    "invalid"
                } else {
                    var identity: ByteArray? = null
                    try {
                        identity = identityStore.get(profileId, SecureIdentityStore.Record.WARP_SECRET)
                        when {
                            identity == null -> "missing"
                            NativeEngine.validateWarpSecret(identity) == NativeEngine.OK -> "ready"
                            else -> "invalid"
                        }
                    } catch (_: Exception) {
                        "invalid"
                    } finally {
                        identity?.fill(0)
                    }
                }
            statuses.put(
                JSONObject()
                    .put("profile_id", profileId)
                    .put("state", state),
            )
        }
        catalog.put("identity_statuses", statuses)
    }

    private fun connect(call: MethodCall, result: MethodChannel.Result) {
        if (!NativeEngine.isReady()) {
            result.error(
                "ENGINE_UNAVAILABLE",
                "The Rust data channel is not available; no VPN interface was created.",
                null,
            )
            return
        }

        val arguments = call.arguments
        if (arguments !is Map<*, *>) {
            result.error(
                "INVALID_PROFILE",
                "The Flutter profile payload must be a map.",
                null,
            )
            return
        }
        val mode = arguments["mode"] as? String
        if (mode == null || mode !in setOf("vpn", "socks5", "httpProxy")) {
            result.error(
                "INVALID_PROFILE",
                "The Android operating mode is invalid.",
                null,
            )
            return
        }
        val profileJson = JSONObject(arguments).toString()
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

    private fun bindControlService() {
        if (controlBound) return
        controlBound =
            bindService(
                Intent(this, UsqueVpnService::class.java)
                    .setAction(UsqueVpnService.ACTION_CONTROL),
                controlConnection,
                Context.BIND_AUTO_CREATE,
            )
    }

    private fun requestSnapshot(result: MethodChannel.Result) {
        val service = controlMessenger
        if (service == null) {
            bindControlService()
            result.success(disconnectedSnapshot())
            return
        }

        val requestId = nextSnapshotId
        nextSnapshotId = if (nextSnapshotId == Int.MAX_VALUE) 1 else nextSnapshotId + 1
        pendingSnapshots[requestId] = result
        val message =
            Message.obtain(null, UsqueVpnService.MSG_SNAPSHOT).apply {
                arg1 = requestId
                replyTo = replyMessenger
            }
        try {
            service.send(message)
        } catch (_: RemoteException) {
            pendingSnapshots.remove(requestId)
            controlMessenger = null
            result.error(
                "ENGINE_IPC_UNAVAILABLE",
                "The Android VPN process could not receive the status request.",
                null,
            )
            return
        }

        mainHandler.postDelayed(
            {
                pendingSnapshots.remove(requestId)?.error(
                    "ENGINE_IPC_TIMEOUT",
                    "The Android VPN process did not reply in time.",
                    null,
                )
            },
            SNAPSHOT_TIMEOUT_MILLIS,
        )
    }

    private fun requestDisconnect(result: MethodChannel.Result) {
        val service = controlMessenger
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
            bindControlService()
            mainHandler.postDelayed(
                {
                    if (pendingDisconnectResult === result) {
                        pendingDisconnectResult = null
                        result.error(
                            "ENGINE_IPC_TIMEOUT",
                            "The Android VPN process did not accept the disconnect request in time.",
                            null,
                        )
                    }
                },
                SNAPSHOT_TIMEOUT_MILLIS,
            )
            return
        }

        val requestId = nextSnapshotId
        nextSnapshotId = if (nextSnapshotId == Int.MAX_VALUE) 1 else nextSnapshotId + 1
        pendingSnapshots[requestId] = result
        val message =
            Message.obtain(null, UsqueVpnService.MSG_DISCONNECT).apply {
                arg1 = requestId
                replyTo = replyMessenger
            }
        try {
            service.send(message)
        } catch (_: RemoteException) {
            pendingSnapshots.remove(requestId)
            controlMessenger = null
            result.error(
                "ENGINE_IPC_UNAVAILABLE",
                "The Android VPN process could not receive the disconnect request.",
                null,
            )
            return
        }

        mainHandler.postDelayed(
            {
                pendingSnapshots.remove(requestId)?.error(
                    "ENGINE_IPC_TIMEOUT",
                    "The Android VPN process did not disconnect in time.",
                    null,
                )
            },
            SNAPSHOT_TIMEOUT_MILLIS,
        )
    }

    private fun pauseCaptivePortal(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val seconds = call.argument<Int>("seconds") ?: 600
        if (seconds !in 1..600) {
            result.error(
                "INVALID_ARGUMENT",
                "Captive Portal Pause must be between 1 and 600 seconds.",
                null,
            )
            return
        }
        val service = controlMessenger
        if (service == null) {
            bindControlService()
            result.error(
                "ENGINE_IPC_UNAVAILABLE",
                "The Android VPN process is not ready.",
                null,
            )
            return
        }
        val requestId = nextSnapshotId
        nextSnapshotId = if (nextSnapshotId == Int.MAX_VALUE) 1 else nextSnapshotId + 1
        pendingSnapshots[requestId] = result
        val message =
            Message.obtain(null, UsqueVpnService.MSG_PAUSE_CAPTIVE_PORTAL).apply {
                arg1 = requestId
                replyTo = replyMessenger
                data = Bundle().apply { putInt("seconds", seconds) }
            }
        try {
            service.send(message)
        } catch (_: RemoteException) {
            pendingSnapshots.remove(requestId)
            controlMessenger = null
            result.error(
                "ENGINE_IPC_UNAVAILABLE",
                "The Android VPN process could not receive the pause request.",
                null,
            )
            return
        }
        mainHandler.postDelayed(
            {
                pendingSnapshots.remove(requestId)?.error(
                    "ENGINE_IPC_TIMEOUT",
                    "The Android VPN process did not acknowledge the pause in time.",
                    null,
                )
            },
            SNAPSHOT_TIMEOUT_MILLIS,
        )
    }

    private fun registerForEvents() {
        sendEventControlMessage(UsqueVpnService.MSG_REGISTER_EVENTS)
    }

    private fun unregisterForEvents() {
        sendEventControlMessage(UsqueVpnService.MSG_UNREGISTER_EVENTS)
    }

    private fun sendEventControlMessage(what: Int) {
        val service = controlMessenger ?: return
        try {
            service.send(
                Message.obtain(null, what).apply {
                    replyTo = replyMessenger
                },
            )
        } catch (_: RemoteException) {
            controlMessenger = null
        }
    }

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
            "captive_pause_remaining_seconds" to
                bundle.getInt("captive_pause_remaining_seconds"),
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

    private fun disconnectedSnapshot(): Map<String, Any> =
        mapOf(
            "phase" to "disconnected",
            "download_bytes_per_second" to 0,
            "upload_bytes_per_second" to 0,
            "downloaded_bytes" to 0,
            "uploaded_bytes" to 0,
        )
}
