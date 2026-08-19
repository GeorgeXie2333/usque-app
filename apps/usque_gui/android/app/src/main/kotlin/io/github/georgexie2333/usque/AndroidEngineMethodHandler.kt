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

        fun selectWarpSecretDestination(
            profileId: String,
            result: MethodChannel.Result,
        )

        fun copySensitiveText(
            label: String,
            value: String,
        )

        fun consumeLaunchTarget(): String?

        fun beginZeroTrustLogin(team: String): String

        fun consumeZeroTrustCallback(): String?

        fun cancelZeroTrustLogin()

        fun platformPreferences(): Map<String, Any?>

        fun setStartOnBoot(enabled: Boolean)

        fun requestAddQuickSettingsTile(result: MethodChannel.Result)

        fun openAlwaysOnVpnSettings()

        fun listInstalledApps(): List<Map<String, Any?>>

        fun getAppIcon(packageName: String): ByteArray?

        fun loadPerAppProxy(): Map<String, Any?>

        fun savePerAppProxy(
            enabled: Boolean,
            packageNames: List<String>,
        ): Map<String, Any?>
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

        fun delete(
            profileId: String,
            record: SecureIdentityStore.Record,
        )

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

        fun registerConsumerWarpWithLicense(
            locale: String,
            licenseKey: String,
        ): ByteArray?

        fun registerZeroTrustWarp(
            locale: String,
            team: String,
            callbackUri: String,
        ): ByteArray?

        fun unbindConsumerWarp(warpSecret: ByteArray): Boolean

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

        override fun registerConsumerWarpWithLicense(
            locale: String,
            licenseKey: String,
        ): ByteArray? = NativeEngine.registerConsumerWarpWithLicense(locale, licenseKey)

        override fun registerZeroTrustWarp(
            locale: String,
            team: String,
            callbackUri: String,
        ): ByteArray? = NativeEngine.registerZeroTrustWarp(locale, team, callbackUri)

        override fun unbindConsumerWarp(warpSecret: ByteArray): Boolean = NativeEngine.unbindConsumerWarp(warpSecret)

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

        override fun delete(
            profileId: String,
            record: SecureIdentityStore.Record,
        ) {
            store.delete(profileId, record)
        }

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

            "retry" -> {
                controlClient.requestRetry(result)
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

            "reconfigureActiveProfile" -> {
                reconfigureActiveProfile(call, result)
            }

            "copyLicenseKey" -> {
                copyLicenseKey(call, result)
            }

            "updateLicenseKey" -> {
                replaceLicenseIdentity(call, result, withLicense = true)
            }

            "updateProxyAuth" -> {
                updateProxyAuth(call, result)
            }

            "unbindLicenseKey" -> {
                replaceLicenseIdentity(call, result, withLicense = false)
            }

            "exportWarpSecret" -> {
                exportWarpSecret(call, result)
            }

            "consumeLaunchTarget" -> {
                result.success(activityCommands.consumeLaunchTarget())
            }

            "beginZeroTrustLogin" -> {
                val team = call.argument<String>("team_name")
                if (team == null) {
                    result.error("ZERO_TRUST_TEAM_INVALID", "The organization name is missing.", null)
                } else {
                    runCatching { activityCommands.beginZeroTrustLogin(team) }
                        .onSuccess(result::success)
                        .onFailure {
                            result.error(
                                "ZERO_TRUST_TEAM_INVALID",
                                "Enter one Cloudflare Zero Trust team name.",
                                null,
                            )
                        }
                }
            }

            "consumeZeroTrustCallback" -> {
                result.success(activityCommands.consumeZeroTrustCallback())
            }

            "cancelZeroTrustLogin" -> {
                activityCommands.cancelZeroTrustLogin()
                result.success(null)
            }

            "platformPreferences" -> {
                result.success(activityCommands.platformPreferences())
            }

            "setStartOnBoot" -> {
                val enabled = call.argument<Boolean>("enabled")
                if (enabled == null) {
                    result.error("INVALID_ARGUMENT", "The startup setting is malformed.", null)
                } else {
                    activityCommands.setStartOnBoot(enabled)
                    result.success(null)
                }
            }

            "requestAddQuickSettingsTile" -> {
                activityCommands.requestAddQuickSettingsTile(result)
            }

            "listInstalledApps" -> {
                listInstalledApps(result)
            }

            "getAppIcon" -> {
                getAppIcon(call, result)
            }

            "perAppProxy" -> {
                result.success(activityCommands.loadPerAppProxy())
            }

            "setPerAppProxy" -> {
                setPerAppProxy(call, result)
            }

            "openAlwaysOnVpnSettings" -> {
                activityCommands.openAlwaysOnVpnSettings()
                result.success(null)
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

    private fun listInstalledApps(result: MethodChannel.Result) {
        identityExecutor.execute {
            try {
                val apps = activityCommands.listInstalledApps()
                mainScheduler.post { result.success(apps) }
            } catch (error: Exception) {
                mainScheduler.post {
                    result.error(
                        "PER_APP_CATALOG_FAILED",
                        "Android could not list installed apps.",
                        error.javaClass.simpleName,
                    )
                }
            }
        }
    }

    private fun getAppIcon(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val packageName = call.argument<String>("package_name")
        if (packageName.isNullOrBlank()) {
            result.error("INVALID_ARGUMENT", "The package name is missing.", null)
            return
        }
        identityExecutor.execute {
            try {
                val icon = activityCommands.getAppIcon(packageName)
                mainScheduler.post { result.success(icon) }
            } catch (_: Exception) {
                mainScheduler.post { result.success(null) }
            }
        }
    }

    private fun setPerAppProxy(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val enabled = call.argument<Boolean>("enabled")
        val rawNames = call.argument<List<*>>("package_names")
        if (enabled == null || rawNames == null) {
            result.error("INVALID_ARGUMENT", "The per-app proxy setting is malformed.", null)
            return
        }
        val packageNames = rawNames.mapNotNull { item -> item as? String }
        if (packageNames.size != rawNames.size) {
            result.error("INVALID_ARGUMENT", "The per-app package list is malformed.", null)
            return
        }
        try {
            val saved = activityCommands.savePerAppProxy(enabled, packageNames)
            controlClient.notifyApplyPerApp()
            result.success(saved)
        } catch (error: PerAppProxyStoreException) {
            val message =
                if (error.code == ANDROID_PER_APP_EMPTY) {
                    "Select at least one installed app before enabling per-app proxy."
                } else {
                    "The per-app proxy setting is invalid."
                }
            result.error(error.code, message, null)
        } catch (error: Exception) {
            result.error(
                "PER_APP_STORE_FAILED",
                "Android could not save per-app proxy settings.",
                error.javaClass.simpleName,
            )
        }
    }

    fun finishClearAllData(result: MethodChannel.Result) {
        identityExecutor.execute {
            // Claim and cancellation are one synchronized transition: destroy can cancel before
            // this point, but cannot report cancellation after destructive work has started.
            if (!controlClient.claimInFlightClearAll(result)) {
                return@execute
            }
            try {
                identityStore.clearAll()
                engineBridge.applyProfileCommand(
                    profileConfigPath,
                    """{"command":"clear_all_data"}""",
                ) ?: throw IllegalStateException("Rust did not reset the Profile store")
                maintenanceBridge.clearLocalState()
                mainScheduler.post {
                    if (!controlClient.takeClaimedClearAll(result)) return@post
                    result.success(null)
                }
            } catch (error: Exception) {
                mainScheduler.post {
                    if (!controlClient.takeClaimedClearAll(result)) return@post
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

    private fun reconfigureActiveProfile(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val profile = call.arguments as? Map<*, *>
        if (profile == null) {
            result.error("INVALID_ARGUMENT", "The profile is malformed.", null)
            return
        }
        if (!requireProfileEngine(result)) return
        val normalized = VpnReconfigure.canonicalizeProfileArguments(profile)
        val commandJson =
            flutterValueToJson(
                mapOf(
                    "command" to "reconfigure_active_profile",
                    "profile" to normalized,
                ),
            )
        val profileJson = flutterValueToJson(normalized)
        identityExecutor.execute {
            try {
                engineBridge.applyProfileCommand(profileConfigPath, commandJson)
                    ?: throw IllegalStateException("Rust returned no profile catalog")
                mainScheduler.post {
                    if (!controlClient.requestReconfigure(profileJson, result)) {
                        result.success(null)
                    }
                }
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

    private fun updateProxyAuth(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val profileId = call.argument<String>("profile_id")
        val username = call.argument<String>("username").orEmpty()
        val confirmed = call.argument<Boolean>("confirmed") ?: false
        val passwordBytes =
            when (val password = call.argument<Any>("password")) {
                is ByteArray -> {
                    password
                }

                is String -> {
                    password.toByteArray(Charsets.UTF_8)
                }

                null -> {
                    ByteArray(0)
                }

                else -> {
                    result.error("INVALID_ARGUMENT", "The proxy password is malformed.", null)
                    return
                }
            }
        if (profileId.isNullOrBlank()) {
            passwordBytes.fill(0)
            result.error("INVALID_ARGUMENT", "The profile ID is missing.", null)
            return
        }
        if (!confirmed) {
            passwordBytes.fill(0)
            result.error("CONFIRMATION_REQUIRED", "Saving listener credentials requires confirmation.", null)
            return
        }
        if (username.isEmpty()) {
            if (passwordBytes.isNotEmpty()) {
                passwordBytes.fill(0)
                result.error(
                    "CONFIGURATION_INVALID",
                    "proxy password requires a username",
                    null,
                )
                return
            }
        } else {
            if (!validProxyUsername(username)) {
                passwordBytes.fill(0)
                result.error(
                    "CONFIGURATION_INVALID",
                    "proxy username must be 1 to 255 bytes and cannot contain ':' or NUL",
                    null,
                )
                return
            }
            if (passwordBytes.isEmpty() || passwordBytes.size > 255) {
                passwordBytes.fill(0)
                result.error(
                    "CONFIGURATION_INVALID",
                    "proxy username requires a password",
                    null,
                )
                return
            }
        }
        identityExecutor.execute {
            try {
                if (username.isEmpty()) {
                    identityStore.delete(profileId, SecureIdentityStore.Record.PROXY_PASSWORD)
                } else {
                    identityStore.put(profileId, SecureIdentityStore.Record.PROXY_PASSWORD, passwordBytes)
                }
                mainScheduler.post { result.success(null) }
            } catch (error: Exception) {
                mainScheduler.post {
                    result.error(
                        "CONFIGURATION_INVALID",
                        error.message ?: "Listener credentials could not be saved.",
                        null,
                    )
                }
            } finally {
                passwordBytes.fill(0)
            }
        }
    }

    private fun validProxyUsername(username: String): Boolean {
        val bytes = username.toByteArray(Charsets.UTF_8)
        return bytes.isNotEmpty() &&
            bytes.size <= 255 &&
            !bytes.contains(0) &&
            !username.contains(':')
    }

    private fun copyLicenseKey(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val profileId = call.argument<String>("profile_id")
        if (profileId.isNullOrBlank()) {
            result.error("INVALID_ARGUMENT", "The profile ID is missing.", null)
            return
        }
        identityExecutor.execute {
            var license: ByteArray? = null
            try {
                requireConsumerIdentity(profileId)
                license = identityStore.get(profileId, SecureIdentityStore.Record.LICENSE)
                    ?: throw IllegalStateException("This Profile has no bound License Key")
                val clipboardValue = license.toString(Charsets.UTF_8)
                mainScheduler.post {
                    activityCommands.copySensitiveText("WARP License Key", clipboardValue)
                    result.success(null)
                }
            } catch (error: Exception) {
                mainScheduler.post {
                    result.error(
                        if (error is UnsupportedOperationException) {
                            "IDENTITY_OPERATION_UNSUPPORTED"
                        } else {
                            "LICENSE_NOT_AVAILABLE"
                        },
                        if (error is UnsupportedOperationException) {
                            "License operations are not available for Zero Trust profiles."
                        } else {
                            "This Profile has no License Key to copy."
                        },
                        error.javaClass.simpleName,
                    )
                }
            } finally {
                license?.fill(0)
            }
        }
    }

    private fun exportWarpSecret(
        call: MethodCall,
        result: MethodChannel.Result,
    ) {
        val profileId = call.argument<String>("profile_id")
        if (profileId.isNullOrBlank()) {
            result.error("INVALID_ARGUMENT", "The profile ID is missing.", null)
            return
        }
        identityExecutor.execute {
            try {
                requireConsumerIdentity(profileId)
                mainScheduler.post {
                    activityCommands.selectWarpSecretDestination(profileId, result)
                }
            } catch (error: Exception) {
                mainScheduler.post {
                    result.error(
                        "IDENTITY_OPERATION_UNSUPPORTED",
                        "WARP Secret export is not available for Zero Trust profiles.",
                        error.javaClass.simpleName,
                    )
                }
            }
        }
    }

    private fun replaceLicenseIdentity(
        call: MethodCall,
        result: MethodChannel.Result,
        withLicense: Boolean,
    ) {
        val profileId = call.argument<String>("profile_id")
        val licenseKey = call.argument<String>("license_key")?.trim()
        if (profileId.isNullOrBlank() || (withLicense && licenseKey.isNullOrBlank())) {
            result.error("INVALID_ARGUMENT", "The License request is malformed.", null)
            return
        }
        if (!engineBridge.isLinked()) {
            result.error("ENGINE_UNAVAILABLE", "The Rust identity engine is not linked.", null)
            return
        }
        identityExecutor.execute {
            var oldIdentity: ByteArray? = null
            var oldMetadata: ByteArray? = null
            var oldLicense: ByteArray? = null
            var newIdentity: ByteArray? = null
            var newMetadata: ByteArray? = null
            var newLicense: ByteArray? = null
            var identityReplaced = false
            try {
                requireConsumerIdentity(profileId)
                oldIdentity =
                    identityStore.get(profileId, SecureIdentityStore.Record.WARP_SECRET)
                        ?: throw IllegalStateException("The Profile identity is missing")
                oldMetadata =
                    identityStore.get(profileId, SecureIdentityStore.Record.IDENTITY_METADATA)
                oldLicense = identityStore.get(profileId, SecureIdentityStore.Record.LICENSE)
                val locale = Locale.getDefault().toString()
                newIdentity =
                    if (withLicense) {
                        engineBridge.registerConsumerWarpWithLicense(locale, licenseKey!!)
                    } else {
                        engineBridge.registerConsumerWarp(locale)
                    } ?: throw IllegalStateException("Rust registration returned no identity")
                newLicense = licenseKey?.toByteArray(Charsets.UTF_8)
                newMetadata = consumerIdentityMetadata()

                identityStore.put(
                    profileId,
                    SecureIdentityStore.Record.WARP_SECRET,
                    newIdentity,
                )
                identityReplaced = true
                identityStore.put(
                    profileId,
                    SecureIdentityStore.Record.IDENTITY_METADATA,
                    newMetadata,
                )
                if (withLicense) {
                    identityStore.put(
                        profileId,
                        SecureIdentityStore.Record.LICENSE,
                        newLicense!!,
                    )
                } else {
                    identityStore.delete(profileId, SecureIdentityStore.Record.LICENSE)
                }

                if (engineBridge.unbindConsumerWarp(oldIdentity)) {
                    identityStore.delete(
                        profileId,
                        SecureIdentityStore.Record.PENDING_CLEANUP_SECRET,
                    )
                } else {
                    identityStore.put(
                        profileId,
                        SecureIdentityStore.Record.PENDING_CLEANUP_SECRET,
                        oldIdentity,
                    )
                }
                mainScheduler.post { result.success(null) }
            } catch (error: Exception) {
                if (identityReplaced && oldIdentity != null) {
                    runCatching {
                        identityStore.put(
                            profileId,
                            SecureIdentityStore.Record.WARP_SECRET,
                            oldIdentity,
                        )
                        restoreIdentityRecord(
                            profileId,
                            SecureIdentityStore.Record.IDENTITY_METADATA,
                            oldMetadata,
                        )
                        if (oldLicense != null) {
                            identityStore.put(
                                profileId,
                                SecureIdentityStore.Record.LICENSE,
                                oldLicense,
                            )
                        } else {
                            identityStore.delete(profileId, SecureIdentityStore.Record.LICENSE)
                        }
                    }
                }
                mainScheduler.post {
                    result.error(
                        if (error is UnsupportedOperationException) {
                            "IDENTITY_OPERATION_UNSUPPORTED"
                        } else if (withLicense) {
                            "INVALID_LICENSE_KEY"
                        } else {
                            "LICENSE_UNBIND_FAILED"
                        },
                        if (error is UnsupportedOperationException) {
                            "License operations are not available for Zero Trust profiles."
                        } else if (withLicense) {
                            "The License Key could not be applied; the previous identity remains active."
                        } else {
                            "A replacement free WARP identity could not be created."
                        },
                        error.javaClass.simpleName,
                    )
                }
            } finally {
                oldIdentity?.fill(0)
                oldMetadata?.fill(0)
                oldLicense?.fill(0)
                newIdentity?.fill(0)
                newMetadata?.fill(0)
                newLicense?.fill(0)
            }
        }
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
                        "The GitHub release update check failed.",
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
        val method = call.argument<String>("method") ?: "register"
        val licenseKey = call.argument<String>("license_key")?.trim()
        val team = call.argument<String>("team_name")?.trim()?.lowercase(Locale.ROOT)
        val callbackUri = call.argument<String>("callback_uri")
        if (method !in setOf("register", "registerWithLicense", "zeroTrust")) {
            result.error(
                "FEATURE_REMOVED",
                "New WARP Secret imports are no longer supported.",
                null,
            )
            return
        }
        if (method == "registerWithLicense" && licenseKey.isNullOrBlank()) {
            result.error("INVALID_LICENSE_KEY", "A WARP License Key is required.", null)
            return
        }
        if (
            method == "zeroTrust" &&
            (team.isNullOrBlank() || callbackUri.isNullOrBlank() || callbackUri.length > 64 * 1024)
        ) {
            result.error(
                "ZERO_TRUST_CALLBACK_INVALID",
                "Start the organization login again or paste its complete callback URL.",
                null,
            )
            return
        }
        if (!engineBridge.isLinked()) {
            result.error(
                "ENGINE_UNAVAILABLE",
                "The Rust identity engine is not linked in this build.",
                null,
            )
            return
        }

        identityExecutor.execute {
            var oldIdentity: ByteArray? = null
            var oldMetadata: ByteArray? = null
            var oldLicense: ByteArray? = null
            var newIdentity: ByteArray? = null
            var newMetadata: ByteArray? = null
            var licenseBytes: ByteArray? = null
            var remoteRegistered = false
            var identityStored = false
            var oldProfile: JSONObject? = null
            var endpointUpdated = false
            try {
                val provider = storedIdentityProvider(profileId)
                if (!provider.valid && !provider.repairable) {
                    throw IllegalStateException("Stored identity metadata is invalid")
                }
                if (method == "zeroTrust") {
                    if (provider.provider != "zeroTrust" || provider.organization != team) {
                        throw UnsupportedOperationException("Identity provider change is unsupported")
                    }
                } else if (provider.provider != "consumer") {
                    throw UnsupportedOperationException("Identity provider change is unsupported")
                }

                oldIdentity = identityStore.get(profileId, SecureIdentityStore.Record.WARP_SECRET)
                oldMetadata = identityStore.get(profileId, SecureIdentityStore.Record.IDENTITY_METADATA)
                oldLicense = identityStore.get(profileId, SecureIdentityStore.Record.LICENSE)
                val locale =
                    call
                        .argument<String>("locale")
                        ?.replace('-', '_')
                        ?.takeIf { it.isNotBlank() }
                        ?: Locale.getDefault().toString()
                val endpoint =
                    when (method) {
                        "registerWithLicense" -> {
                            licenseBytes = licenseKey!!.toByteArray(Charsets.UTF_8)
                            newIdentity =
                                engineBridge.registerConsumerWarpWithLicense(locale, licenseKey)
                                    ?: throw IllegalStateException(
                                        "Rust licensed registration returned no identity",
                                    )
                            newMetadata = consumerIdentityMetadata()
                            null
                        }

                        "zeroTrust" -> {
                            val envelopeBytes =
                                engineBridge.registerZeroTrustWarp(locale, team!!, callbackUri!!)
                                    ?: throw IllegalStateException(
                                        "Rust Zero Trust registration returned no identity",
                                    )
                            remoteRegistered = true
                            try {
                                val envelope = JSONObject(envelopeBytes.toString(Charsets.UTF_8))
                                newIdentity =
                                    envelope.getString("warp_secret").toByteArray(Charsets.UTF_8)
                                newMetadata =
                                    envelope
                                        .getString("identity_metadata")
                                        .toByteArray(Charsets.UTF_8)
                                ZeroTrustEndpoint(
                                    ipv4 = envelope.getString("endpoint_v4"),
                                    ipv6 = envelope.getString("endpoint_v6"),
                                    port = envelope.getInt("endpoint_port"),
                                    sni = envelope.getString("sni"),
                                )
                            } finally {
                                envelopeBytes.fill(0)
                            }
                        }

                        else -> {
                            newIdentity =
                                engineBridge.registerConsumerWarp(locale)
                                    ?: throw IllegalStateException("Rust registration returned no identity")
                            newMetadata = consumerIdentityMetadata()
                            null
                        }
                    }
                remoteRegistered = true

                identityStore.put(
                    profileId,
                    SecureIdentityStore.Record.WARP_SECRET,
                    newIdentity,
                )
                identityStored = true
                identityStore.put(
                    profileId,
                    SecureIdentityStore.Record.IDENTITY_METADATA,
                    newMetadata,
                )
                if (licenseBytes != null) {
                    identityStore.put(
                        profileId,
                        SecureIdentityStore.Record.LICENSE,
                        licenseBytes,
                    )
                } else {
                    identityStore.delete(profileId, SecureIdentityStore.Record.LICENSE)
                }

                if (endpoint != null) {
                    oldProfile = loadProfile(profileId)
                    val updated = JSONObject(oldProfile.toString())
                    endpoint.applyTo(updated)
                    engineBridge.applyProfileCommand(
                        profileConfigPath,
                        JSONObject()
                            .put("command", "upsert_profile")
                            .put("profile", updated)
                            .put("identity_provider", "zero_trust")
                            .put("organization", team)
                            .toString(),
                    ) ?: throw IllegalStateException("Rust did not update the Zero Trust endpoint")
                    endpointUpdated = true
                }
                mainScheduler.post { result.success(null) }
            } catch (error: Exception) {
                if (endpointUpdated && oldProfile != null) {
                    runCatching {
                        engineBridge.applyProfileCommand(
                            profileConfigPath,
                            JSONObject()
                                .put("command", "upsert_profile")
                                .put("profile", oldProfile)
                                .put("identity_provider", "zero_trust")
                                .put("organization", team)
                                .toString(),
                        )
                    }
                }
                if (identityStored) {
                    runCatching {
                        restoreIdentityRecord(
                            profileId,
                            SecureIdentityStore.Record.WARP_SECRET,
                            oldIdentity,
                        )
                        restoreIdentityRecord(
                            profileId,
                            SecureIdentityStore.Record.IDENTITY_METADATA,
                            oldMetadata,
                        )
                        restoreIdentityRecord(
                            profileId,
                            SecureIdentityStore.Record.LICENSE,
                            oldLicense,
                        )
                    }
                }
                val code =
                    when {
                        error is UnsupportedOperationException -> {
                            "IDENTITY_PROVIDER_CHANGE_UNSUPPORTED"
                        }

                        method == "zeroTrust" -> {
                            zeroTrustErrorCode(error, remoteRegistered)
                        }

                        method == "registerWithLicense" -> {
                            consumerRegistrationErrorCode(error, withLicense = true)
                        }

                        else -> {
                            "REGISTRATION_FAILED"
                        }
                    }
                mainScheduler.post {
                    result.error(code, identityProvisioningErrorMessage(code), error.javaClass.simpleName)
                }
            } finally {
                oldIdentity?.fill(0)
                oldMetadata?.fill(0)
                oldLicense?.fill(0)
                newIdentity?.fill(0)
                newMetadata?.fill(0)
                licenseBytes?.fill(0)
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
        val licenseKey = (arguments["license_key"] as? String)?.trim()
        val team = (arguments["team_name"] as? String)?.trim()?.lowercase(Locale.ROOT)
        val callbackUri = arguments["callback_uri"] as? String
        if (
            profile == null ||
            profileId.isNullOrBlank() ||
            method !in setOf("register", "registerWithLicense", "zeroTrust") ||
            (method == "registerWithLicense" && licenseKey.isNullOrBlank()) ||
            (
                method == "zeroTrust" &&
                    (
                        team.isNullOrBlank() ||
                            callbackUri.isNullOrBlank() ||
                            callbackUri.length > 64 * 1024
                    )
            ) ||
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
            var metadata: ByteArray? = null
            var licenseBytes: ByteArray? = null
            var remoteRegistered = false
            var committed = false
            try {
                val locale =
                    (arguments["locale"] as? String)
                        ?.replace('-', '_')
                        ?.takeIf { it.isNotBlank() }
                        ?: Locale.getDefault().toString()
                var endpoint: ZeroTrustEndpoint? = null
                when (method) {
                    "registerWithLicense" -> {
                        licenseBytes = licenseKey!!.toByteArray(Charsets.UTF_8)
                        bytes =
                            engineBridge.registerConsumerWarpWithLicense(locale, licenseKey)
                                ?: throw IllegalStateException(
                                    "Rust licensed registration returned no identity",
                                )
                        metadata = consumerIdentityMetadata()
                    }

                    "zeroTrust" -> {
                        val envelopeBytes =
                            engineBridge.registerZeroTrustWarp(locale, team!!, callbackUri!!)
                                ?: throw IllegalStateException(
                                    "Rust Zero Trust registration returned no identity",
                                )
                        remoteRegistered = true
                        try {
                            val envelope = JSONObject(envelopeBytes.toString(Charsets.UTF_8))
                            bytes = envelope.getString("warp_secret").toByteArray(Charsets.UTF_8)
                            metadata =
                                envelope
                                    .getString("identity_metadata")
                                    .toByteArray(Charsets.UTF_8)
                            endpoint =
                                ZeroTrustEndpoint(
                                    ipv4 = envelope.getString("endpoint_v4"),
                                    ipv6 = envelope.getString("endpoint_v6"),
                                    port = envelope.getInt("endpoint_port"),
                                    sni = envelope.getString("sni"),
                                )
                        } finally {
                            envelopeBytes.fill(0)
                        }
                    }

                    else -> {
                        bytes =
                            engineBridge.registerConsumerWarp(locale)
                                ?: throw IllegalStateException("Rust registration returned no identity")
                        metadata = consumerIdentityMetadata()
                    }
                }
                remoteRegistered = true

                engineBridge.applyProfileCommand(
                    profileConfigPath,
                    JSONObject()
                        .put("command", "begin_identity_creation")
                        .put("profile_id", profileId)
                        .toString(),
                ) ?: throw IllegalStateException("Rust did not prepare profile creation")
                prepared = true

                identityStore.put(
                    profileId,
                    SecureIdentityStore.Record.WARP_SECRET,
                    bytes,
                )
                stored = true
                identityStore.put(
                    profileId,
                    SecureIdentityStore.Record.IDENTITY_METADATA,
                    metadata,
                )
                if (licenseBytes != null) {
                    identityStore.put(
                        profileId,
                        SecureIdentityStore.Record.LICENSE,
                        licenseBytes,
                    )
                }
                val committedProfile = JSONObject(profile)
                endpoint?.applyTo(committedProfile)
                val commitCommand =
                    JSONObject()
                        .put("command", "commit_profile_with_identity")
                        .put("profile", committedProfile)
                        .put(
                            "identity_provider",
                            if (method == "zeroTrust") "zero_trust" else "consumer",
                        )
                if (method == "zeroTrust") {
                    commitCommand.put("organization", team)
                }
                val response =
                    engineBridge.applyProfileCommand(
                        profileConfigPath,
                        commitCommand.toString(),
                    ) ?: throw IllegalStateException("Rust did not commit the profile")
                committed = true
                val responseObject = JSONObject(response)
                appendIdentityStatuses(responseObject)
                val catalog = jsonObjectToFlutterMap(responseObject)
                mainScheduler.post { result.success(catalog) }
            } catch (error: Exception) {
                if (committed) {
                    rollbackCommittedProfile(profileId)
                } else {
                    if (stored) {
                        runCatching { identityStore.deleteIdentity(profileId) }
                    }
                }
                if (prepared && !committed) {
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
                        method == "zeroTrust" -> {
                            zeroTrustErrorCode(error, remoteRegistered)
                        }

                        method == "registerWithLicense" -> {
                            consumerRegistrationErrorCode(error, withLicense = true)
                        }

                        method == "register" -> {
                            "REGISTRATION_FAILED"
                        }

                        else -> {
                            "PROFILE_STORE_FAILED"
                        }
                    }
                mainScheduler.post {
                    result.error(
                        code,
                        if (code == "PROFILE_STORE_FAILED") {
                            "The profile and its identity could not be saved safely."
                        } else {
                            identityProvisioningErrorMessage(code)
                        },
                        error.javaClass.simpleName,
                    )
                }
            } finally {
                bytes?.fill(0)
                metadata?.fill(0)
                licenseBytes?.fill(0)
            }
        }
    }

    private fun appendIdentityStatuses(catalog: JSONObject) {
        val statuses = JSONArray()
        val profiles = catalog.optJSONArray("profiles") ?: JSONArray()
        for (index in 0 until profiles.length()) {
            val profile = profiles.getJSONObject(index)
            val profileId = profile.optString("id")
            var license: ByteArray? = null
            var pendingCleanup: ByteArray? = null
            val provider = storedIdentityProvider(profileId, profile)
            val state =
                if (profileId.isBlank()) {
                    "invalid"
                } else {
                    var identity: ByteArray? = null
                    try {
                        identity = identityStore.get(profileId, SecureIdentityStore.Record.WARP_SECRET)
                        when {
                            identity == null -> "missing"

                            engineBridge.validateWarpSecret(identity) == warpSecretOkCode &&
                                provider.valid -> "ready"

                            else -> "invalid"
                        }
                    } catch (_: Exception) {
                        "invalid"
                    } finally {
                        identity?.fill(0)
                    }
                }
            try {
                if (profileId.isNotBlank()) {
                    license = identityStore.get(profileId, SecureIdentityStore.Record.LICENSE)
                    pendingCleanup =
                        identityStore.get(
                            profileId,
                            SecureIdentityStore.Record.PENDING_CLEANUP_SECRET,
                        )
                }
            } catch (_: Exception) {
                // Identity state remains authoritative; optional account metadata degrades safely.
            }
            statuses.put(
                JSONObject()
                    .put("profile_id", profileId)
                    .put("state", state)
                    .put(
                        "license_state",
                        if (provider.provider == "zeroTrust") {
                            "notApplicable"
                        } else if (license == null) {
                            "free"
                        } else {
                            "warpPlus"
                        },
                    ).put(
                        "account_type",
                        if (provider.provider == "zeroTrust") {
                            "Zero Trust"
                        } else if (license == null) {
                            "Free"
                        } else {
                            "WARP+"
                        },
                    ).put("provider", provider.provider)
                    .put("organization", provider.organization ?: "")
                    .put("cleanup_pending", pendingCleanup != null),
            )
            license?.fill(0)
            pendingCleanup?.fill(0)
        }
        catalog.put("identity_statuses", statuses)
    }

    private data class StoredIdentityProvider(
        val provider: String,
        val organization: String? = null,
        val valid: Boolean = true,
        val repairable: Boolean = valid,
    )

    private data class ZeroTrustEndpoint(
        val ipv4: String,
        val ipv6: String,
        val port: Int,
        val sni: String,
    ) {
        fun applyTo(profile: JSONObject) {
            profile
                .put("endpoint_v4", ipv4)
                .put("endpoint_v6", ipv6)
                .put("endpoint_port", port)
                .put("sni", sni)
        }
    }

    private fun consumerIdentityMetadata(): ByteArray =
        """{"version":1,"provider":"consumer"}""".toByteArray(Charsets.UTF_8)

    private fun restoreIdentityRecord(
        profileId: String,
        record: SecureIdentityStore.Record,
        previous: ByteArray?,
    ) {
        if (previous == null) {
            identityStore.delete(profileId, record)
        } else {
            identityStore.put(profileId, record, previous)
        }
    }

    private fun loadProfile(profileId: String): JSONObject {
        val response =
            engineBridge.applyProfileCommand(
                profileConfigPath,
                JSONObject().put("command", "list_profiles").toString(),
            ) ?: throw IllegalStateException("Rust returned no profile catalog")
        val profiles = JSONObject(response).getJSONArray("profiles")
        for (index in 0 until profiles.length()) {
            val profile = profiles.getJSONObject(index)
            if (profile.optString("id") == profileId) return profile
        }
        throw IllegalStateException("Profile does not exist")
    }

    private fun rollbackCommittedProfile(profileId: String) {
        runCatching {
            engineBridge.applyProfileCommand(
                profileConfigPath,
                JSONObject()
                    .put("command", "delete_profile")
                    .put("profile_id", profileId)
                    .toString(),
            ) ?: throw IllegalStateException("Rust did not mark the profile for rollback")
            identityStore.deleteIdentity(profileId)
            engineBridge.applyProfileCommand(
                profileConfigPath,
                JSONObject()
                    .put("command", "complete_identity_deletions")
                    .put("profile_ids", JSONArray().put(profileId))
                    .toString(),
            ) ?: throw IllegalStateException("Rust did not complete the profile rollback")
        }
    }

    private fun zeroTrustErrorCode(
        error: Exception,
        remoteRegistered: Boolean,
    ): String {
        if (remoteRegistered) return "ZERO_TRUST_LOCAL_COMMIT_FAILED"
        val rawMessage = error.message.orEmpty()
        zeroTrustNativeErrorCode(rawMessage)?.let { return it }
        val message = rawMessage.lowercase(Locale.ROOT)
        return when {
            "team name is invalid" in message -> "ZERO_TRUST_TEAM_INVALID"
            "callback is invalid" in message -> "ZERO_TRUST_CALLBACK_INVALID"
            "login expired" in message -> "ZERO_TRUST_LOGIN_EXPIRED"
            "denied this device login" in message -> "ZERO_TRUST_LOGIN_DENIED"
            "contract changed" in message -> "ZERO_TRUST_CONTRACT_CHANGED"
            else -> "ZERO_TRUST_REGISTRATION_FAILED"
        }
    }

    private fun zeroTrustNativeErrorCode(message: String): String? {
        when (message) {
            "USQUE_ZT_TEAM_INVALID" -> return "ZERO_TRUST_TEAM_INVALID"
            "USQUE_ZT_CALLBACK_INVALID" -> return "ZERO_TRUST_CALLBACK_INVALID"
            "USQUE_ZT_LOGIN_EXPIRED" -> return "ZERO_TRUST_LOGIN_EXPIRED"
            "USQUE_ZT_LOGIN_DENIED" -> return "ZERO_TRUST_LOGIN_DENIED"
            "USQUE_ZT_CONTRACT_CHANGED" -> return "ZERO_TRUST_CONTRACT_CHANGED"
            "USQUE_ZT_LOCAL_COMMIT_FAILED" -> return "ZERO_TRUST_LOCAL_COMMIT_FAILED"
            "USQUE_ZT_REGISTRATION_FAILED" -> return "ZERO_TRUST_REGISTRATION_FAILED"
        }
        val parts = message.split(':')
        if (
            parts.size == 3 &&
            parts[0] == "USQUE_ZT_HTTP" &&
            parts[1] in setOf("device_registration", "masque_enrollment", "unknown") &&
            parts[2].toIntOrNull()?.let { it in 100..599 } == true
        ) {
            return "ZERO_TRUST_HTTP_${parts[1].uppercase(Locale.ROOT)}_${parts[2]}"
        }
        if (
            parts.size == 2 &&
            parts[0] == "USQUE_ZT_NETWORK" &&
            parts[1] in setOf("device_registration", "masque_enrollment", "unknown")
        ) {
            return "ZERO_TRUST_NETWORK_${parts[1].uppercase(Locale.ROOT)}"
        }
        val diagnosticReasons =
            setOf(
                "invalid_locale",
                "runtime_initialization",
                "http_client_initialization",
                "terms_not_accepted",
                "invalid_registration_options",
                "invalid_api_url",
                "invalid_device_id",
                "unexpected_license_error",
                "response_too_large",
                "invalid_api_response",
                "request_serialization",
                "identity_contract",
            )
        if (
            parts.size == 2 &&
            parts[0] == "USQUE_ZT_DIAGNOSTIC" &&
            parts[1] in diagnosticReasons
        ) {
            return "ZERO_TRUST_DIAGNOSTIC_${parts[1].uppercase(Locale.ROOT)}"
        }
        return null
    }

    private fun consumerRegistrationErrorCode(
        error: Exception,
        withLicense: Boolean,
    ): String {
        val nativeCode = error.message.orEmpty()
        if (
            withLicense &&
            (
                nativeCode == "USQUE_CONSUMER_INVALID_LICENSE_KEY" ||
                    nativeCode in
                    setOf(
                        "USQUE_CONSUMER_HTTP:400",
                        "USQUE_CONSUMER_HTTP:401",
                        "USQUE_CONSUMER_HTTP:403",
                    )
            )
        ) {
            return "INVALID_LICENSE_KEY"
        }
        return "REGISTRATION_FAILED"
    }

    private fun identityProvisioningErrorMessage(code: String): String {
        if (code.startsWith("ZERO_TRUST_HTTP_")) {
            val status = code.substringAfterLast('_').toIntOrNull()
            val enrollment = "MASQUE_ENROLLMENT" in code
            if (status != null) {
                return if (enrollment) {
                    "The device was registered, but MASQUE enrollment failed (HTTP $status). Start a fresh login; an administrator may need to remove the residual device."
                } else if ("UNKNOWN" in code) {
                    "Cloudflare registration failed (HTTP $status). Start a fresh organization login and try again."
                } else {
                    "Cloudflare rejected device registration (HTTP $status). Start a fresh organization login and try again."
                }
            }
        }
        if (code.startsWith("ZERO_TRUST_DIAGNOSTIC_")) {
            val reason =
                code
                    .removePrefix("ZERO_TRUST_DIAGNOSTIC_")
                    .lowercase(Locale.ROOT)
            return "Experimental Zero Trust registration stopped at '$reason'. Start a fresh organization login."
        }
        return when (code) {
            "IDENTITY_PROVIDER_CHANGE_UNSUPPORTED" -> {
                "A profile cannot switch between Consumer WARP and Zero Trust organizations."
            }

            "ZERO_TRUST_TEAM_INVALID" -> {
                "Enter one valid Cloudflare Zero Trust team name."
            }

            "ZERO_TRUST_CALLBACK_INVALID" -> {
                "Start the organization login again or paste its complete callback URL."
            }

            "ZERO_TRUST_LOGIN_EXPIRED" -> {
                "The organization login expired. Start it again."
            }

            "ZERO_TRUST_LOGIN_DENIED" -> {
                "The organization denied this device login."
            }

            "ZERO_TRUST_CONTRACT_CHANGED" -> {
                "Cloudflare's experimental Zero Trust registration response is incompatible. A residual device may need administrator cleanup."
            }

            "ZERO_TRUST_NETWORK_DEVICE_REGISTRATION" -> {
                "Could not reach Cloudflare while registering the device. Check the network, then start a fresh login."
            }

            "ZERO_TRUST_NETWORK_MASQUE_ENROLLMENT" -> {
                "The device may be registered, but network contact was lost during MASQUE enrollment. Start a fresh login; an administrator may need to remove the residual device."
            }

            "ZERO_TRUST_NETWORK_UNKNOWN" -> {
                "Could not reach Cloudflare during Zero Trust registration. Check the network, then start a fresh login."
            }

            "ZERO_TRUST_LOCAL_COMMIT_FAILED" -> {
                "Registration completed remotely, but local saving failed. An administrator may need to remove the residual device."
            }

            "ZERO_TRUST_REGISTRATION_FAILED" -> {
                "Zero Trust registration failed. Check the network, then start the login again."
            }

            "INVALID_LICENSE_KEY" -> {
                "The WARP License Key could not be applied."
            }

            else -> {
                "Consumer WARP registration failed. Check the network and try again."
            }
        }
    }

    private fun requireConsumerIdentity(profileId: String) {
        val provider = storedIdentityProvider(profileId)
        if (provider.provider == "zeroTrust") {
            throw UnsupportedOperationException("Zero Trust identity operations are not applicable")
        }
        if (!provider.valid) {
            throw IllegalStateException("Stored identity metadata is invalid")
        }
    }

    private fun storedIdentityProvider(
        profileId: String,
        profile: JSONObject? = null,
    ): StoredIdentityProvider {
        if (profileId.isBlank()) return StoredIdentityProvider("consumer", valid = false)
        val storedProfile = profile ?: runCatching { loadProfile(profileId) }.getOrNull()
        val binding = storedProfile?.let(::profileIdentityBinding)
        var bytes: ByteArray? = null
        return try {
            bytes = identityStore.get(profileId, SecureIdentityStore.Record.IDENTITY_METADATA)
            if (bytes == null) {
                return binding ?: when {
                    storedProfile == null -> {
                        StoredIdentityProvider("consumer", valid = false)
                    }

                    isZeroTrustEndpoint(storedProfile) -> {
                        StoredIdentityProvider("zeroTrust", valid = false)
                    }

                    else -> {
                        // Profiles created before identity metadata existed are
                        // legitimate Consumer identities.
                        StoredIdentityProvider("consumer")
                    }
                }
            }
            val metadata = JSONObject(bytes.toString(Charsets.UTF_8))
            val metadataProvider =
                if (metadata.optInt("version") != 1) {
                    StoredIdentityProvider("consumer", valid = false)
                } else {
                    when (metadata.optString("provider")) {
                        "consumer" -> {
                            StoredIdentityProvider("consumer")
                        }

                        "zero_trust" -> {
                            val organization = metadata.optString("organization")
                            val normalized =
                                runCatching { ZeroTrustCallbackSession.normalizeTeam(organization) }
                                    .getOrNull()
                            if (normalized == null || normalized != organization) {
                                StoredIdentityProvider("zeroTrust", valid = false)
                            } else {
                                StoredIdentityProvider("zeroTrust", organization)
                            }
                        }

                        else -> {
                            StoredIdentityProvider("consumer", valid = false)
                        }
                    }
                }
            if (
                binding != null &&
                (
                    !binding.repairable ||
                        binding.provider != metadataProvider.provider ||
                        binding.organization != metadataProvider.organization
                )
            ) {
                binding.copy(valid = false)
            } else {
                metadataProvider
            }
        } catch (_: Exception) {
            binding ?: StoredIdentityProvider("consumer", valid = false)
        } finally {
            bytes?.fill(0)
        }
    }

    private fun profileIdentityBinding(profile: JSONObject): StoredIdentityProvider? {
        val provider = profile.optString("identity_provider")
        val organization = profile.optString("identity_organization")
        return when (provider) {
            "" -> {
                null
            }

            "consumer" -> {
                if (organization.isEmpty()) {
                    StoredIdentityProvider("consumer", valid = false, repairable = true)
                } else {
                    StoredIdentityProvider("consumer", valid = false, repairable = false)
                }
            }

            "zero_trust" -> {
                val normalized =
                    runCatching { ZeroTrustCallbackSession.normalizeTeam(organization) }.getOrNull()
                if (normalized == null || normalized != organization) {
                    StoredIdentityProvider("zeroTrust", valid = false, repairable = false)
                } else {
                    StoredIdentityProvider(
                        "zeroTrust",
                        organization,
                        valid = false,
                        repairable = true,
                    )
                }
            }

            else -> {
                StoredIdentityProvider("consumer", valid = false, repairable = false)
            }
        }
    }

    private fun isZeroTrustEndpoint(profile: JSONObject): Boolean {
        val sni = profile.optString("sni")
        val ipv4 = profile.optString("endpoint_v4")
        val ipv6 = profile.optString("endpoint_v6")
        return sni.equals("zt-masque.cloudflareclient.com", ignoreCase = true) ||
            ipv4.startsWith("162.159.197.") ||
            ipv6.startsWith("2606:4700:102:", ignoreCase = true)
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
        val legacyMode = arguments["mode"] as? String
        if (legacyMode == null || legacyMode !in setOf("vpn", "socks5", "httpProxy")) {
            result.error(
                "INVALID_PROFILE",
                "The Android operating mode is invalid.",
                null,
            )
            return
        }
        val normalized = VpnReconfigure.canonicalizeProfileArguments(arguments)
        val mode = normalized["mode"] as String
        val profileJson = flutterValueToJson(normalized)
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
