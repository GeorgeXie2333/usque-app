package io.github.georgexie2333.usque

import android.os.Bundle
import org.json.JSONObject
import java.time.Instant

/**
 * Mutable connection snapshot held by [UsqueVpnService]. Pure merge/fingerprint/kill-switch
 * and notification text logic lives here so JVM unit tests can cover wire fields without
 * standing up the full VPN service.
 */
internal class ServiceSnapshotState {
    var phase: String = "disconnected"
    var warning: String? = null
    var errorCode: String? = null
    var failure: FailureFields? = null
    var transport: String? = null
    var addressFamily: String? = null
    var connectedAt: String? = null
    var downloadBytesPerSecond: Long = 0
    var uploadBytesPerSecond: Long = 0
    var downloadedBytes: Long = 0
    var uploadedBytes: Long = 0
    var reconnectCount: Int = 0
    var activeListeners: List<String> = emptyList()
    var activeFrontends: List<String> = emptyList()
    var tunnelIpv4Available: Boolean = false
    var tunnelIpv6Available: Boolean = false
    var exitIpv4: String? = null
    var exitIpv6: String? = null
    var exitCity: String? = null
    var exitCountry: String? = null
    var exitCountryCode: String? = null
    var exitFlagSvg: String? = null
    var flagCacheLookupCode: String? = null
    var killSwitchEnabled: Boolean = false
    private var lastBroadcastFingerprint: String? = null

    data class PlatformFlags(
        val tunnelOpen: Boolean,
        val activeMode: String?,
        val platformLockdown: Boolean,
        val alwaysOn: Boolean,
        val tunFdValid: Boolean = tunnelOpen,
        val underlyingNetworkPresent: Boolean = false,
        val underlyingFamilyMask: Int = 0,
        val networkGeneration: Long = 0,
        val dnsServerCount: Int = 0,
        val nativeRuntimeActive: Boolean = false,
        val foregroundNotificationActive: Boolean = false,
        val pendingCleanup: Boolean = false,
    )

    data class FlagCacheWrite(
        val countryCode: String,
        val svg: String,
    )

    data class NativeMergeResult(
        val phaseChanged: Boolean,
        val enteredError: Boolean,
        val cacheWrite: FlagCacheWrite? = null,
        val cacheLookupCountryCode: String? = null,
    )

    data class FailureFields(
        val code: String,
        val stage: String,
        val transport: String? = null,
        val addressFamily: String? = null,
        val retryable: Boolean = false,
        val fallbackAllowed: Boolean = false,
        val severity: String = "error",
        val remediationKey: String = "retry",
        val sanitizedDetail: String? = null,
    )

    /** Full field map matching Messenger Bundle keys consumed by MainActivity. */
    data class SnapshotFields(
        val phase: String,
        val warning: String?,
        val errorCode: String?,
        val failure: FailureFields?,
        val transport: String?,
        val addressFamily: String?,
        val connectedAt: String?,
        val downloadBytesPerSecond: Long,
        val uploadBytesPerSecond: Long,
        val downloadedBytes: Long,
        val uploadedBytes: Long,
        val reconnectCount: Int,
        val activeListeners: List<String>,
        val activeFrontends: List<String>,
        val tunnelIpv4Available: Boolean,
        val tunnelIpv6Available: Boolean,
        val exitIpv4: String?,
        val exitIpv6: String?,
        val exitCity: String?,
        val exitCountry: String?,
        val exitCountryCode: String?,
        val exitFlagSvg: String?,
        val killSwitchState: String,
        val platformLockdown: Boolean,
        val alwaysOn: Boolean,
        val tunFdValid: Boolean,
        val underlyingNetworkPresent: Boolean,
        val underlyingFamilyMask: Int,
        val networkGeneration: Long,
        val dnsServerCount: Int,
        val nativeRuntimeActive: Boolean,
        val foregroundNotificationActive: Boolean,
        val pendingCleanup: Boolean,
    )

    /**
     * Messenger Bundle wire key names. [toBundle] / [wireEntries] must use these so unit tests
     * can lock the Flutter bridge contract without depending on Android Bundle stubs.
     */
    object WireKeys {
        const val PHASE = "phase"
        const val WARNING = "warning"
        const val ERROR_CODE = "error_code"
        const val FAILURE_CODE = "failure_code"
        const val FAILURE_STAGE = "failure_stage"
        const val FAILURE_TRANSPORT = "failure_transport"
        const val FAILURE_ADDRESS_FAMILY = "failure_address_family"
        const val FAILURE_RETRYABLE = "failure_retryable"
        const val FAILURE_FALLBACK_ALLOWED = "failure_fallback_allowed"
        const val FAILURE_SEVERITY = "failure_severity"
        const val FAILURE_REMEDIATION_KEY = "failure_remediation_key"
        const val FAILURE_SANITIZED_DETAIL = "failure_sanitized_detail"
        const val TRANSPORT = "transport"
        const val ADDRESS_FAMILY = "address_family"
        const val CONNECTED_AT = "connected_at"
        const val DOWNLOAD_BYTES_PER_SECOND = "download_bytes_per_second"
        const val UPLOAD_BYTES_PER_SECOND = "upload_bytes_per_second"
        const val DOWNLOADED_BYTES = "downloaded_bytes"
        const val UPLOADED_BYTES = "uploaded_bytes"
        const val RECONNECT_COUNT = "reconnect_count"
        const val ACTIVE_LISTENERS = "active_listeners"
        const val ACTIVE_FRONTENDS = "active_frontends"
        const val TUNNEL_IPV4_AVAILABLE = "tunnel_ipv4_available"
        const val TUNNEL_IPV6_AVAILABLE = "tunnel_ipv6_available"
        const val EXIT_IPV4 = "exit_ipv4"
        const val EXIT_IPV6 = "exit_ipv6"
        const val EXIT_CITY = "exit_city"
        const val EXIT_COUNTRY = "exit_country"
        const val EXIT_COUNTRY_CODE = "exit_country_code"
        const val EXIT_FLAG_SVG = "exit_flag_svg"
        const val KILL_SWITCH_STATE = "kill_switch_state"
        const val PLATFORM_LOCKDOWN = "platform_lockdown"
        const val ALWAYS_ON = "always_on"
        const val VPN_SERVICE_STATE = "vpn_service_state"
        const val VPN_PROCESS_STATE = "vpn_process_state"
        const val TUN_FD_VALID = "tun_fd_valid"
        const val TUN_INTERFACE_PRESENT = "tun_interface_present"
        const val UNDERLYING_NETWORK_PRESENT = "underlying_network_present"
        const val UNDERLYING_FAMILY_MASK = "underlying_family_mask"
        const val NETWORK_GENERATION = "network_generation"
        const val DNS_SERVER_COUNT = "dns_server_count"
        const val NATIVE_RUNTIME_STATE = "native_runtime_state"
        const val FOREGROUND_NOTIFICATION_STATE = "foreground_notification_state"
        const val PENDING_CLEANUP = "pending_cleanup"
    }

    /**
     * Resets connection snapshot fields for a new lifecycle phase.
     *
     * **Not cleared here (intentional):**
     * - [lastBroadcastFingerprint] — retained so an identical post-reset snapshot (e.g. second
     *   `disconnected` broadcast with zeroed counters) is still fingerprint-deduped. Reply and
     *   register-events paths bypass dedup and always send a live Bundle.
     */
    fun reset(nextPhase: String) {
        phase = nextPhase
        warning = null
        errorCode = null
        failure = null
        transport = null
        addressFamily = null
        connectedAt = null
        downloadBytesPerSecond = 0
        uploadBytesPerSecond = 0
        downloadedBytes = 0
        uploadedBytes = 0
        reconnectCount = 0
        activeListeners = emptyList()
        activeFrontends = emptyList()
        tunnelIpv4Available = false
        tunnelIpv6Available = false
        exitIpv4 = null
        exitIpv6 = null
        exitCity = null
        exitCountry = null
        exitCountryCode = null
        exitFlagSvg = null
        flagCacheLookupCode = null
        killSwitchEnabled = false
    }

    fun killSwitchState(
        tunnelOpen: Boolean,
        activeMode: String?,
    ): String =
        when {
            killSwitchEnabled && tunnelOpen -> "active"
            activeMode == "vpn" -> "inactive"
            else -> "notApplicable"
        }

    fun notificationText(): String =
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

    /**
     * Platform-free native snapshot fields. Unit tests construct this directly; the service
     * adapts [JSONObject] via [fromNativeJson].
     */
    data class NativeSnapshotFields(
        val phase: String = "error",
        val warning: String? = null,
        val errorCode: String? = null,
        val failure: FailureFields? = null,
        val transport: String? = null,
        val addressFamily: String? = null,
        val downloadBytesPerSecond: Long = 0,
        val uploadBytesPerSecond: Long = 0,
        val downloadedBytes: Long = 0,
        val uploadedBytes: Long = 0,
        val reconnectCount: Int = 0,
        val activeListeners: List<String> = emptyList(),
        val activeFrontends: List<String> = emptyList(),
        val tunnelIpv4Available: Boolean = false,
        val tunnelIpv6Available: Boolean = false,
        val exitIpv4: String? = null,
        val exitIpv6: String? = null,
        val exitCity: String? = null,
        val exitCountry: String? = null,
        val exitCountryCode: String? = null,
        val exitFlagSvg: String? = null,
    )

    fun applyNativeSnapshot(source: JSONObject): NativeMergeResult = applyNativeSnapshot(fromNativeJson(source))

    fun applyNativeSnapshot(source: NativeSnapshotFields): NativeMergeResult {
        val previousPhase = phase
        phase = source.phase
        warning = source.warning
        errorCode = source.errorCode
        failure = source.failure
        transport = source.transport
        addressFamily = source.addressFamily
        downloadBytesPerSecond = source.downloadBytesPerSecond.coerceAtLeast(0)
        uploadBytesPerSecond = source.uploadBytesPerSecond.coerceAtLeast(0)
        downloadedBytes = source.downloadedBytes.coerceAtLeast(0)
        uploadedBytes = source.uploadedBytes.coerceAtLeast(0)
        reconnectCount = source.reconnectCount.coerceAtLeast(0)
        activeListeners = source.activeListeners
        activeFrontends = source.activeFrontends
        tunnelIpv4Available = source.tunnelIpv4Available
        tunnelIpv6Available = source.tunnelIpv6Available
        exitIpv4 = source.exitIpv4
        exitIpv6 = source.exitIpv6
        exitCity = source.exitCity
        exitCountry = source.exitCountry
        exitCountryCode = source.exitCountryCode

        var cacheWrite: FlagCacheWrite? = null
        var cacheLookup: String? = null
        val nativeFlag = source.exitFlagSvg
        if (nativeFlag != null && nativeFlag != exitFlagSvg) {
            exitFlagSvg = nativeFlag
            val countryCode = exitCountryCode
            if (countryCode != null) {
                cacheWrite = FlagCacheWrite(countryCode, nativeFlag)
            }
        } else if (
            nativeFlag == null &&
            exitFlagSvg == null &&
            exitCountryCode != null &&
            flagCacheLookupCode != exitCountryCode
        ) {
            val countryCode = requireNotNull(exitCountryCode)
            flagCacheLookupCode = countryCode
            cacheLookup = countryCode
        }
        if ((phase == "connected" || phase == "degraded") && connectedAt == null) {
            connectedAt = Instant.now().toString()
        }
        return NativeMergeResult(
            phaseChanged = phase != previousPhase,
            enteredError = phase == "error",
            cacheWrite = cacheWrite,
            cacheLookupCountryCode = cacheLookup,
        )
    }

    companion object {
        fun fromNativeJson(source: JSONObject): NativeSnapshotFields =
            NativeSnapshotFields(
                phase = source.optString("phase", "error"),
                warning = source.optNullableString("warning"),
                errorCode = source.optNullableString("error_code"),
                failure = source.optJSONObject("failure")?.let(::failureFromNativeJson),
                transport = source.optNullableString("transport"),
                addressFamily = source.optNullableString("address_family"),
                downloadBytesPerSecond = source.optLong("download_bytes_per_second", 0),
                uploadBytesPerSecond = source.optLong("upload_bytes_per_second", 0),
                downloadedBytes = source.optLong("downloaded_bytes", 0),
                uploadedBytes = source.optLong("uploaded_bytes", 0),
                reconnectCount = source.optInt("reconnect_count", 0),
                activeListeners =
                    source.optJSONArray("active_listeners")?.let { listeners ->
                        List(listeners.length()) { index -> listeners.getString(index) }
                    } ?: emptyList(),
                activeFrontends =
                    source.optJSONArray("active_frontends")?.let { frontends ->
                        List(frontends.length()) { index -> frontends.getString(index) }
                            .filter(FRONTEND_KINDS::contains)
                            .distinct()
                    } ?: emptyList(),
                tunnelIpv4Available = source.optBoolean("tunnel_ipv4_available", false),
                tunnelIpv6Available = source.optBoolean("tunnel_ipv6_available", false),
                exitIpv4 = source.optNullableString("exit_ipv4"),
                exitIpv6 = source.optNullableString("exit_ipv6"),
                exitCity = source.optNullableString("exit_city"),
                exitCountry = source.optNullableString("exit_country"),
                exitCountryCode = source.optNullableString("exit_country_code"),
                exitFlagSvg = source.optNullableString("exit_flag_svg"),
            )

        private fun failureFromNativeJson(source: JSONObject): FailureFields? {
            val code = source.optNullableString("code")?.takeIf(SAFE_CODE::matches) ?: return null
            val stage = source.optNullableString("stage")?.takeIf(SAFE_TOKEN::matches) ?: return null
            return FailureFields(
                code = code,
                stage = stage,
                transport = source.optNullableString("transport")?.takeIf(SAFE_TOKEN::matches),
                addressFamily =
                    source.optNullableString("address_family")?.takeIf(SAFE_TOKEN::matches),
                retryable = source.optBoolean("retryable", false),
                fallbackAllowed = source.optBoolean("fallback_allowed", false),
                severity =
                    source.optNullableString("severity")?.takeIf(SAFE_TOKEN::matches) ?: "error",
                remediationKey =
                    source.optNullableString("remediation_key")?.takeIf(SAFE_TOKEN::matches)
                        ?: "retry",
                sanitizedDetail =
                    source.optNullableString("sanitized_detail")?.takeIf(::safeFailureDetail),
            )
        }

        private fun safeFailureDetail(value: String): Boolean =
            value.length <= 64 &&
                FAILURE_DETAIL_PREFIXES.any { prefix ->
                    value.removePrefix(prefix).let { suffix ->
                        suffix.length < value.length &&
                            suffix.isNotEmpty() &&
                            suffix.all { character -> character in '0'..'9' }
                    }
                }

        private val SAFE_CODE = Regex("^[A-Z][A-Z0-9_]{1,63}$")
        private val SAFE_TOKEN = Regex("^[a-z][a-z0-9_]{0,63}$")
        private val FAILURE_DETAIL_PREFIXES =
            setOf("attempt ", "status ", "generation ", "queue depth ")
        private val FRONTEND_KINDS = setOf("socks5", "http")
    }

    fun snapshotFields(platform: PlatformFlags): SnapshotFields =
        SnapshotFields(
            phase = phase,
            warning = warning,
            errorCode = errorCode,
            failure = failure,
            transport = transport,
            addressFamily = addressFamily,
            connectedAt = connectedAt,
            downloadBytesPerSecond = downloadBytesPerSecond,
            uploadBytesPerSecond = uploadBytesPerSecond,
            downloadedBytes = downloadedBytes,
            uploadedBytes = uploadedBytes,
            reconnectCount = reconnectCount,
            activeListeners = activeListeners,
            activeFrontends = activeFrontends,
            tunnelIpv4Available = tunnelIpv4Available,
            tunnelIpv6Available = tunnelIpv6Available,
            exitIpv4 = exitIpv4,
            exitIpv6 = exitIpv6,
            exitCity = exitCity,
            exitCountry = exitCountry,
            exitCountryCode = exitCountryCode,
            exitFlagSvg = exitFlagSvg,
            killSwitchState = killSwitchState(platform.tunnelOpen, platform.activeMode),
            platformLockdown = platform.platformLockdown,
            alwaysOn = platform.alwaysOn,
            tunFdValid = platform.tunFdValid,
            underlyingNetworkPresent = platform.underlyingNetworkPresent,
            underlyingFamilyMask = platform.underlyingFamilyMask,
            networkGeneration = platform.networkGeneration,
            dnsServerCount = platform.dnsServerCount,
            nativeRuntimeActive = platform.nativeRuntimeActive,
            foregroundNotificationActive = platform.foregroundNotificationActive,
            pendingCleanup = platform.pendingCleanup,
        )

    /**
     * Platform-free wire map using [WireKeys]. Unit tests assert every Messenger key string
     * and value here; [toBundle] is a thin Android adapter over this map.
     */
    fun wireEntries(platform: PlatformFlags): Map<String, Any?> {
        val fields = snapshotFields(platform)
        return linkedMapOf(
            WireKeys.PHASE to fields.phase,
            WireKeys.WARNING to fields.warning,
            WireKeys.ERROR_CODE to fields.errorCode,
            WireKeys.FAILURE_CODE to fields.failure?.code,
            WireKeys.FAILURE_STAGE to fields.failure?.stage,
            WireKeys.FAILURE_TRANSPORT to fields.failure?.transport,
            WireKeys.FAILURE_ADDRESS_FAMILY to fields.failure?.addressFamily,
            WireKeys.FAILURE_RETRYABLE to (fields.failure?.retryable ?: false),
            WireKeys.FAILURE_FALLBACK_ALLOWED to (fields.failure?.fallbackAllowed ?: false),
            WireKeys.FAILURE_SEVERITY to fields.failure?.severity,
            WireKeys.FAILURE_REMEDIATION_KEY to fields.failure?.remediationKey,
            WireKeys.FAILURE_SANITIZED_DETAIL to fields.failure?.sanitizedDetail,
            WireKeys.TRANSPORT to fields.transport,
            WireKeys.ADDRESS_FAMILY to fields.addressFamily,
            WireKeys.CONNECTED_AT to fields.connectedAt,
            WireKeys.DOWNLOAD_BYTES_PER_SECOND to fields.downloadBytesPerSecond,
            WireKeys.UPLOAD_BYTES_PER_SECOND to fields.uploadBytesPerSecond,
            WireKeys.DOWNLOADED_BYTES to fields.downloadedBytes,
            WireKeys.UPLOADED_BYTES to fields.uploadedBytes,
            WireKeys.RECONNECT_COUNT to fields.reconnectCount,
            WireKeys.ACTIVE_LISTENERS to ArrayList(fields.activeListeners),
            WireKeys.ACTIVE_FRONTENDS to ArrayList(fields.activeFrontends),
            WireKeys.TUNNEL_IPV4_AVAILABLE to fields.tunnelIpv4Available,
            WireKeys.TUNNEL_IPV6_AVAILABLE to fields.tunnelIpv6Available,
            WireKeys.EXIT_IPV4 to fields.exitIpv4,
            WireKeys.EXIT_IPV6 to fields.exitIpv6,
            WireKeys.EXIT_CITY to fields.exitCity,
            WireKeys.EXIT_COUNTRY to fields.exitCountry,
            WireKeys.EXIT_COUNTRY_CODE to fields.exitCountryCode,
            WireKeys.EXIT_FLAG_SVG to fields.exitFlagSvg,
            WireKeys.KILL_SWITCH_STATE to fields.killSwitchState,
            WireKeys.PLATFORM_LOCKDOWN to fields.platformLockdown,
            WireKeys.ALWAYS_ON to fields.alwaysOn,
            WireKeys.VPN_SERVICE_STATE to "running",
            WireKeys.VPN_PROCESS_STATE to "reachable",
            WireKeys.TUN_FD_VALID to fields.tunFdValid,
            WireKeys.TUN_INTERFACE_PRESENT to fields.tunFdValid,
            WireKeys.UNDERLYING_NETWORK_PRESENT to fields.underlyingNetworkPresent,
            WireKeys.UNDERLYING_FAMILY_MASK to fields.underlyingFamilyMask,
            WireKeys.NETWORK_GENERATION to fields.networkGeneration,
            WireKeys.DNS_SERVER_COUNT to fields.dnsServerCount,
            WireKeys.NATIVE_RUNTIME_STATE to
                if (fields.nativeRuntimeActive) "running" else "stopped",
            WireKeys.FOREGROUND_NOTIFICATION_STATE to
                if (fields.foregroundNotificationActive) "active" else "inactive",
            WireKeys.PENDING_CLEANUP to fields.pendingCleanup,
        )
    }

    fun toBundle(platform: PlatformFlags): Bundle {
        val entries = wireEntries(platform)
        return Bundle().apply {
            putString(WireKeys.PHASE, entries[WireKeys.PHASE] as String?)
            putString(WireKeys.WARNING, entries[WireKeys.WARNING] as String?)
            putString(WireKeys.ERROR_CODE, entries[WireKeys.ERROR_CODE] as String?)
            putString(WireKeys.FAILURE_CODE, entries[WireKeys.FAILURE_CODE] as String?)
            putString(WireKeys.FAILURE_STAGE, entries[WireKeys.FAILURE_STAGE] as String?)
            putString(WireKeys.FAILURE_TRANSPORT, entries[WireKeys.FAILURE_TRANSPORT] as String?)
            putString(
                WireKeys.FAILURE_ADDRESS_FAMILY,
                entries[WireKeys.FAILURE_ADDRESS_FAMILY] as String?,
            )
            putBoolean(WireKeys.FAILURE_RETRYABLE, entries[WireKeys.FAILURE_RETRYABLE] as Boolean)
            putBoolean(
                WireKeys.FAILURE_FALLBACK_ALLOWED,
                entries[WireKeys.FAILURE_FALLBACK_ALLOWED] as Boolean,
            )
            putString(WireKeys.FAILURE_SEVERITY, entries[WireKeys.FAILURE_SEVERITY] as String?)
            putString(
                WireKeys.FAILURE_REMEDIATION_KEY,
                entries[WireKeys.FAILURE_REMEDIATION_KEY] as String?,
            )
            putString(
                WireKeys.FAILURE_SANITIZED_DETAIL,
                entries[WireKeys.FAILURE_SANITIZED_DETAIL] as String?,
            )
            putString(WireKeys.TRANSPORT, entries[WireKeys.TRANSPORT] as String?)
            putString(WireKeys.ADDRESS_FAMILY, entries[WireKeys.ADDRESS_FAMILY] as String?)
            putString(WireKeys.CONNECTED_AT, entries[WireKeys.CONNECTED_AT] as String?)
            putLong(
                WireKeys.DOWNLOAD_BYTES_PER_SECOND,
                entries[WireKeys.DOWNLOAD_BYTES_PER_SECOND] as Long,
            )
            putLong(
                WireKeys.UPLOAD_BYTES_PER_SECOND,
                entries[WireKeys.UPLOAD_BYTES_PER_SECOND] as Long,
            )
            putLong(WireKeys.DOWNLOADED_BYTES, entries[WireKeys.DOWNLOADED_BYTES] as Long)
            putLong(WireKeys.UPLOADED_BYTES, entries[WireKeys.UPLOADED_BYTES] as Long)
            putInt(WireKeys.RECONNECT_COUNT, entries[WireKeys.RECONNECT_COUNT] as Int)
            @Suppress("UNCHECKED_CAST")
            putStringArrayList(
                WireKeys.ACTIVE_LISTENERS,
                entries[WireKeys.ACTIVE_LISTENERS] as ArrayList<String>,
            )
            @Suppress("UNCHECKED_CAST")
            putStringArrayList(
                WireKeys.ACTIVE_FRONTENDS,
                entries[WireKeys.ACTIVE_FRONTENDS] as ArrayList<String>,
            )
            putBoolean(
                WireKeys.TUNNEL_IPV4_AVAILABLE,
                entries[WireKeys.TUNNEL_IPV4_AVAILABLE] as Boolean,
            )
            putBoolean(
                WireKeys.TUNNEL_IPV6_AVAILABLE,
                entries[WireKeys.TUNNEL_IPV6_AVAILABLE] as Boolean,
            )
            putString(WireKeys.EXIT_IPV4, entries[WireKeys.EXIT_IPV4] as String?)
            putString(WireKeys.EXIT_IPV6, entries[WireKeys.EXIT_IPV6] as String?)
            putString(WireKeys.EXIT_CITY, entries[WireKeys.EXIT_CITY] as String?)
            putString(WireKeys.EXIT_COUNTRY, entries[WireKeys.EXIT_COUNTRY] as String?)
            putString(WireKeys.EXIT_COUNTRY_CODE, entries[WireKeys.EXIT_COUNTRY_CODE] as String?)
            putString(WireKeys.EXIT_FLAG_SVG, entries[WireKeys.EXIT_FLAG_SVG] as String?)
            putString(WireKeys.KILL_SWITCH_STATE, entries[WireKeys.KILL_SWITCH_STATE] as String?)
            putBoolean(WireKeys.PLATFORM_LOCKDOWN, entries[WireKeys.PLATFORM_LOCKDOWN] as Boolean)
            putBoolean(WireKeys.ALWAYS_ON, entries[WireKeys.ALWAYS_ON] as Boolean)
            putString(WireKeys.VPN_SERVICE_STATE, entries[WireKeys.VPN_SERVICE_STATE] as String)
            putString(WireKeys.VPN_PROCESS_STATE, entries[WireKeys.VPN_PROCESS_STATE] as String)
            putBoolean(WireKeys.TUN_FD_VALID, entries[WireKeys.TUN_FD_VALID] as Boolean)
            putBoolean(
                WireKeys.TUN_INTERFACE_PRESENT,
                entries[WireKeys.TUN_INTERFACE_PRESENT] as Boolean,
            )
            putBoolean(
                WireKeys.UNDERLYING_NETWORK_PRESENT,
                entries[WireKeys.UNDERLYING_NETWORK_PRESENT] as Boolean,
            )
            putInt(
                WireKeys.UNDERLYING_FAMILY_MASK,
                entries[WireKeys.UNDERLYING_FAMILY_MASK] as Int,
            )
            putLong(
                WireKeys.NETWORK_GENERATION,
                entries[WireKeys.NETWORK_GENERATION] as Long,
            )
            putInt(WireKeys.DNS_SERVER_COUNT, entries[WireKeys.DNS_SERVER_COUNT] as Int)
            putString(
                WireKeys.NATIVE_RUNTIME_STATE,
                entries[WireKeys.NATIVE_RUNTIME_STATE] as String,
            )
            putString(
                WireKeys.FOREGROUND_NOTIFICATION_STATE,
                entries[WireKeys.FOREGROUND_NOTIFICATION_STATE] as String,
            )
            putBoolean(WireKeys.PENDING_CLEANUP, entries[WireKeys.PENDING_CLEANUP] as Boolean)
        }
    }

    fun fingerprint(platform: PlatformFlags): String =
        listOf(
            phase,
            warning,
            errorCode,
            failure?.code,
            failure?.stage,
            failure?.transport,
            failure?.addressFamily,
            failure?.retryable,
            failure?.fallbackAllowed,
            failure?.severity,
            failure?.remediationKey,
            failure?.sanitizedDetail,
            transport,
            addressFamily,
            connectedAt,
            downloadBytesPerSecond,
            uploadBytesPerSecond,
            downloadedBytes,
            uploadedBytes,
            reconnectCount,
            activeListeners.joinToString("\u001f"),
            activeFrontends.joinToString("\u001f"),
            tunnelIpv4Available,
            tunnelIpv6Available,
            exitIpv4,
            exitIpv6,
            exitCity,
            exitCountry,
            exitCountryCode,
            exitFlagSvg,
            killSwitchEnabled,
            platform.tunnelOpen,
            platform.activeMode,
            platform.platformLockdown,
            platform.alwaysOn,
            platform.tunFdValid,
            platform.underlyingNetworkPresent,
            platform.underlyingFamilyMask,
            platform.networkGeneration,
            platform.dnsServerCount,
            platform.nativeRuntimeActive,
            platform.foregroundNotificationActive,
            platform.pendingCleanup,
        ).joinToString("\u001e")

    /**
     * Records a broadcast fingerprint when it changed. Returns true if callers should emit.
     */
    fun markBroadcastIfChanged(platform: PlatformFlags): Boolean {
        val next = fingerprint(platform)
        if (next == lastBroadcastFingerprint) return false
        lastBroadcastFingerprint = next
        return true
    }

    /**
     * Returns a Bundle when the fingerprint changed since the last broadcast; otherwise null.
     */
    fun takeBroadcastBundle(platform: PlatformFlags): Bundle? {
        if (!markBroadcastIfChanged(platform)) return null
        return toBundle(platform)
    }

    /** Visible for tests that assert fingerprint stability without side effects. */
    internal fun lastBroadcastFingerprintForTest(): String? = lastBroadcastFingerprint
}

internal fun JSONObject.optNullableString(name: String): String? =
    if (has(name) && !isNull(name)) optString(name).takeIf(String::isNotBlank) else null
