package io.github.georgexie2333.usque

import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import org.json.JSONArray
import org.json.JSONObject
import java.util.Locale
import java.util.concurrent.Executor

/**
 * Flutter engine method dispatch: argument validation and coordination of
 * profile, identity, update, diagnostics, and clear-all commands.
 * VPN permission / Activity results remain on [MainActivity].
 */
internal class AndroidEngineMethodHandler(
    private val profileConfigPath: String,
    private val identityStore: IdentityStore,
    private val identityExecutor: Executor,
    private val mainScheduler: VpnControlClient.MainScheduler,
    private val controlClient: VpnControlClient,
    private val activityCommands: ActivityCommands,
    private val engineBridge: EngineBridge = DefaultEngineBridge,
    private val maintenanceBridge: MaintenanceBridge,
    private val defaultIdentityProfile: String = DEFAULT_IDENTITY_PROFILE,
    private val warpSecretOkCode: Int = NativeEngine.OK,
) {
    companion object {
        const val DEFAULT_IDENTITY_PROFILE = "8c30b771-9ebd-457a-b67b-bbc74a1ddba6"
    }

    /**
     * Activity-owned flows that require UI / permission surfaces.
     */
    interface ActivityCommands {
        fun cancelPendingVpnConnection(
            code: String,
            message: String,
        )

        fun connectAfterValidation(
            profileJson: String,
            mode: String,
            result: MethodChannel.Result,
        )

        fun selectDiagnosticsDestination(result: MethodChannel.Result)
    }

    /**
     * Identity vault surface — injectable for JVM tests.
     */
    interface IdentityStore {
        fun put(
            profileId: String,
            record: SecureIdentityStore.Record,
            value: ByteArray,
        )

        fun get(
            profileId: String,
            record: SecureIdentityStore.Record,
        ): ByteArray?

        fun deleteIdentity(profileId: String)

        fun clearAll()
    }

    /**
     * Native engine surface — injectable for JVM tests.
     */
    interface EngineBridge {
        fun isLinked(): Boolean

        fun isReady(): Boolean

        fun applyProfileCommand(
            configPath: String,
            requestJson: String,
        ): String?

        fun registerConsumerWarp(locale: String): ByteArray?

        fun validateWarpSecret(secret: ByteArray): Int
    }

    interface MaintenanceBridge {
        fun checkForUpdates(manual: Boolean): Map<String, Any?>

        fun clearLocalState()
    }

    private object DefaultEngineBridge : EngineBridge {
        override fun isLinked(): Boolean = NativeEngine.isLinked()

        override fun isReady(): Boolean = NativeEngine.isReady()

        override fun applyProfileCommand(
            configPath: String,
            requestJson: String,
        ): String? = NativeEngine.applyProfileCommand(configPath, requestJson)

        override fun registerConsumerWarp(locale: String): ByteArray? = NativeEngine.registerConsumerWarp(locale)

        override fun validateWarpSecret(secret: ByteArray): Int = NativeEngine.validateWarpSecret(secret)
    }

    internal class SecureIdentityStoreAdapter(
        private val store: SecureIdentityStore,
    ) : IdentityStore {
        override fun put(
            profileId: String,
            record: SecureIdentityStore.Record,
            value: ByteArray,
        ) {
            store.put(profileId, record, value)
        }

        override fun get(
            profileId: String,
            record: SecureIdentityStore.Record,
        ): ByteArray? = store.get(profileId, record)

        override fun deleteIdentity(profileId: String) {
            store.deleteIdentity(profileId)
        }

        override fun clearAll() {
            store.clearAll()
        }
    }

    internal class AndroidMaintenanceAdapter(
        private val context: android.content.Context,
    ) : MaintenanceBridge {
        override fun checkForUpdates(manual: Boolean): Map<String, Any?> =
            AndroidMaintenance.checkForUpdates(context, manual)

        override fun clearLocalState() {
            AndroidMaintenance.clearLocalState(context)
        }
    }

    fun handle(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        when (call.method) {
            "snapshot" -> {
                controlClient.requestSnapshot(result)
            }

            "disconnect" -> {
                activityCommands.cancelPendingVpnConnection(
                    "VPN_PERMISSION_CANCELLED",
                    "The VPN connection request was cancelled.",
                )
                controlClient.requestDisconnect(result)
            }

            "connect" -> {
                connect(call, result)
            }

            "provisionIdentity" -> {
                provisionIdentity(call, result)
            }

            "createProfileWithIdentity" -> {
                createProfileWithIdentity(call, result)
            }

            "importLegacyProfiles" -> {
                importLegacyProfiles(call, result)
            }

            "upsertProfile" -> {
                upsertProfile(call, result)
            }

            "deleteProfile" -> {
                deleteProfile(call, result)
            }

            "setActiveProfile" -> {
                setActiveProfile(call, result)
            }

            "pauseCaptivePortal" -> {
                pauseCaptivePortal(call, result)
            }

            "exportDiagnostics" -> {
                activityCommands.selectDiagnosticsDestination(result)
            }

            "checkForUpdates" -> {
                checkForUpdates(call, result)
            }

            "clearAllData" -> {
                clearAllData(call, result)
            }

            else -> {
                result.notImplemented()
            }
        }
    }

    fun finishClearAllData(result: MethodChannel.Result) {
        identityExecutor.execute {
            try {
                identityStore.clearAll()
                engineBridge.applyProfileCommand(
                    profileConfigPath,
                    """{"command":"clear_all_data"}""",
                ) ?: throw IllegalStateException("Rust did not reset the Profile store")
                maintenanceBridge.clearLocalState()
                mainScheduler.post {
                    // Destroy may have already completed with CLEAR_ALL_CANCELLED.
                    if (!controlClient.takeInFlightClearAll(result)) return@post
                    result.success(null)
                }
            } catch (error: Exception) {
                mainScheduler.post {
                    if (!controlClient.takeInFlightClearAll(result)) return@post
                    result.error(
                        "CLEAR_ALL_FAILED",
                        "Android could not clear all local Usque data.",
                        error.javaClass.simpleName,
                    )
                }
            }
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
        if (!requireProfileEngine(result)) return
        runProfileCommand(
            flutterValueToJson(
                mapOf(
                    "command" to "import_legacy_profiles",
                    "profiles" to profiles,
                    "active_profile_id" to activeProfileId,
                ),
            ),
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
        if (!requireProfileEngine(result)) return
        runProfileCommand(
            flutterValueToJson(
                mapOf(
                    "command" to "upsert_profile",
                    "profile" to profile,
                ),
            ),
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
        if (!requireProfileEngine(result)) return
        runProfileCommand(
            flutterValueToJson(
                mapOf(
                    "command" to "delete_profile",
                    "profile_id" to profileId,
                ),
            ),
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
        if (!requireProfileEngine(result)) return
        runProfileCommand(
            flutterValueToJson(
                mapOf(
                    "command" to "set_active_profile",
                    "profile_id" to profileId,
                ),
            ),
            result,
        )
    }

    private fun requireProfileEngine(result: MethodChannel.Result): Boolean {
        if (engineBridge.isLinked()) return true
        result.error(
            "ENGINE_UNAVAILABLE",
            "The Rust profile store is not linked in this build.",
            null,
        )
        return false
    }

    private fun runProfileCommand(
        commandJson: String,
        result: MethodChannel.Result,
        returnCatalog: Boolean = false,
    ) {
        if (!requireProfileEngine(result)) return
        identityExecutor.execute {
            try {
                var response =
                    engineBridge.applyProfileCommand(profileConfigPath, commandJson)
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
                        engineBridge.applyProfileCommand(
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
                        engineBridge.applyProfileCommand(
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
                mainScheduler.post { result.success(catalog) }
            } catch (error: Exception) {
                mainScheduler.post {
                    result.error(
                        "PROFILE_STORE_FAILED",
                        "The Rust profile store rejected this operation.",
                        error.javaClass.simpleName,
                    )
                }
            }
        }
    }

    private fun checkForUpdates(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        if (!engineBridge.isLinked()) {
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
                val update = maintenanceBridge.checkForUpdates(manual)
                mainScheduler.post { result.success(update) }
            } catch (error: Exception) {
                mainScheduler.post {
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
        activityCommands.cancelPendingVpnConnection(
            "VPN_PERMISSION_CANCELLED",
            "The VPN connection request was cancelled while clearing local data.",
        )
        controlClient.requestClearAllData(result)
    }

    private fun provisionIdentity(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        if (call.argument<Boolean>("terms_accepted") != true) {
            result.error(
                "TERMS_NOT_ACCEPTED",
                "Cloudflare terms must be accepted before Consumer WARP registration.",
                null,
            )
            return
        }
        val profileId = call.argument<String>("profile_id") ?: defaultIdentityProfile
        val secret = call.argument<String>("warp_secret")
        if (!engineBridge.isLinked()) {
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
                            call
                                .argument<String>("locale")
                                ?.replace('-', '_')
                                ?.takeIf { it.isNotBlank() }
                                ?: Locale.getDefault().toString()
                        engineBridge.registerConsumerWarp(locale)
                            ?: throw IllegalStateException("Rust registration returned no identity")
                    } else {
                        secret.toByteArray(Charsets.UTF_8).also { candidate ->
                            if (engineBridge.validateWarpSecret(candidate) != warpSecretOkCode) {
                                candidate.fill(0)
                                throw IllegalArgumentException("Invalid WARP Secret")
                            }
                        }
                    }
                } catch (error: Exception) {
                    mainScheduler.post {
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
                mainScheduler.post { result.success(null) }
            } catch (error: Exception) {
                mainScheduler.post {
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

    private fun createProfileWithIdentity(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val arguments =
            call.arguments as? Map<*, *> ?: run {
                result.error("INVALID_ARGUMENT", "The profile identity request is malformed.", null)
                return
            }
        val profile = arguments["profile"] as? Map<*, *>
        val profileId = profile?.get("id") as? String
        val method = arguments["method"] as? String
        val secret = arguments["warp_secret"] as? String
        if (
            profile == null ||
            profileId.isNullOrBlank() ||
            method !in setOf("register", "importSecret") ||
            arguments["terms_accepted"] != true
        ) {
            result.error("INVALID_ARGUMENT", "The profile identity request is malformed.", null)
            return
        }
        if (!engineBridge.isLinked()) {
            result.error("ENGINE_UNAVAILABLE", "The Rust identity engine is not linked.", null)
            return
        }

        identityExecutor.execute {
            var prepared = false
            var stored = false
            var bytes: ByteArray? = null
            try {
                engineBridge.applyProfileCommand(
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
                        engineBridge.registerConsumerWarp(locale)
                            ?: throw IllegalStateException("Rust registration returned no identity")
                    } else {
                        val value =
                            secret?.takeIf { it.isNotBlank() }
                                ?: throw IllegalArgumentException("A WARP Secret is required")
                        value.toByteArray(Charsets.UTF_8).also { candidate ->
                            if (engineBridge.validateWarpSecret(candidate) != warpSecretOkCode) {
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
                    engineBridge.applyProfileCommand(
                        profileConfigPath,
                        JSONObject()
                            .put("command", "commit_profile_with_identity")
                            .put("profile", JSONObject(profile))
                            .toString(),
                    ) ?: throw IllegalStateException("Rust did not commit the profile")
                val responseObject = JSONObject(response)
                appendIdentityStatuses(responseObject)
                val catalog = jsonObjectToFlutterMap(responseObject)
                mainScheduler.post { result.success(catalog) }
            } catch (error: Exception) {
                if (stored) {
                    runCatching { identityStore.deleteIdentity(profileId) }
                }
                if (prepared) {
                    runCatching {
                        engineBridge.applyProfileCommand(
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
                        method == "register" && error !is IllegalArgumentException -> {
                            "REGISTRATION_FAILED"
                        }

                        error is IllegalArgumentException -> {
                            "INVALID_WARP_SECRET"
                        }

                        else -> {
                            "PROFILE_STORE_FAILED"
                        }
                    }
                mainScheduler.post {
                    result.error(
                        code,
                        when (code) {
                            "REGISTRATION_FAILED" -> {
                                "Consumer WARP registration failed. Check the network and try again."
                            }

                            "INVALID_WARP_SECRET" -> {
                                "The WARP Secret is malformed or missing."
                            }

                            else -> {
                                "The profile and its identity could not be saved safely."
                            }
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
                            engineBridge.validateWarpSecret(identity) == warpSecretOkCode -> "ready"
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

    private fun connect(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        if (!engineBridge.isReady()) {
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
        val profileJson = flutterValueToJson(arguments)
        activityCommands.connectAfterValidation(profileJson, mode, result)
    }

    /**
     * Encode Flutter method maps without org.json so JVM unit tests do not hit
     * Android framework stubs. Null-valued map keys are omitted (same as
     * `JSONObject(Map)`); list null elements encode as JSON null.
     */
    internal fun flutterValueToJson(value: Any?): String =
        when (value) {
            null -> {
                "null"
            }

            is Boolean -> {
                value.toString()
            }

            is Number -> {
                value.toString()
            }

            is String -> {
                jsonQuote(value)
            }

            is Map<*, *> -> {
                value.entries
                    .filter { (_, entryValue) -> entryValue != null }
                    .joinToString(prefix = "{", postfix = "}") { (key, entryValue) ->
                        "${jsonQuote(key.toString())}:${flutterValueToJson(entryValue)}"
                    }
            }

            is List<*> -> {
                value.joinToString(prefix = "[", postfix = "]") { entry ->
                    flutterValueToJson(entry)
                }
            }

            is Array<*> -> {
                value.joinToString(prefix = "[", postfix = "]") { entry ->
                    flutterValueToJson(entry)
                }
            }

            else -> {
                jsonQuote(value.toString())
            }
        }

    private fun jsonQuote(value: String): String {
        val escaped =
            buildString(value.length + 2) {
                for (char in value) {
                    when (char) {
                        '\\' -> {
                            append("\\\\")
                        }

                        '"' -> {
                            append("\\\"")
                        }

                        '\b' -> {
                            append("\\b")
                        }

                        '\u000C' -> {
                            append("\\f")
                        }

                        '\n' -> {
                            append("\\n")
                        }

                        '\r' -> {
                            append("\\r")
                        }

                        '\t' -> {
                            append("\\t")
                        }

                        else -> {
                            if (char.code < 0x20) {
                                append("\\u")
                                append(char.code.toString(16).padStart(4, '0'))
                            } else {
                                append(char)
                            }
                        }
                    }
                }
            }
        return "\"$escaped\""
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
        controlClient.requestPauseCaptivePortal(seconds, result)
    }

    private fun jsonObjectToFlutterMap(source: JSONObject): Map<String, Any?> =
        source.keys().asSequence().associateWith { key ->
            jsonValueToFlutter(source.get(key))
        }

    private fun jsonValueToFlutter(value: Any?): Any? =
        when (value) {
            null, JSONObject.NULL -> {
                null
            }

            is JSONObject -> {
                jsonObjectToFlutterMap(value)
            }

            is JSONArray -> {
                List(value.length()) { index ->
                    jsonValueToFlutter(value.get(index))
                }
            }

            is Boolean, is Int, is Long, is Double, is String -> {
                value
            }

            is Number -> {
                value.toDouble()
            }

            else -> {
                value.toString()
            }
        }
}
