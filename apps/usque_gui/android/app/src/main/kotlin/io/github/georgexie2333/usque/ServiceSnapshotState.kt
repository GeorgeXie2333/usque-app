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
    var transport: String? = null
    var addressFamily: String? = null
    var connectedAt: String? = null
    var downloadBytesPerSecond: Long = 0
    var uploadBytesPerSecond: Long = 0
    var downloadedBytes: Long = 0
    var uploadedBytes: Long = 0
    var reconnectCount: Int = 0
    var activeListeners: List<String> = emptyList()
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

    /** Full field map matching Messenger Bundle keys consumed by MainActivity. */
    data class SnapshotFields(
        val phase: String,
        val warning: String?,
        val errorCode: String?,
        val transport: String?,
        val addressFamily: String?,
        val connectedAt: String?,
        val downloadBytesPerSecond: Long,
        val uploadBytesPerSecond: Long,
        val downloadedBytes: Long,
        val uploadedBytes: Long,
        val reconnectCount: Int,
        val activeListeners: List<String>,
        val exitIpv4: String?,
        val exitIpv6: String?,
        val exitCity: String?,
        val exitCountry: String?,
        val exitCountryCode: String?,
        val exitFlagSvg: String?,
        val killSwitchState: String,
        val platformLockdown: Boolean,
        val alwaysOn: Boolean,
    )

    /**
     * Messenger Bundle wire key names. [toBundle] / [wireEntries] must use these so unit tests
     * can lock the Flutter bridge contract without depending on Android Bundle stubs.
     */
    object WireKeys {
        const val PHASE = "phase"
        const val WARNING = "warning"
        const val ERROR_CODE = "error_code"
        const val TRANSPORT = "transport"
        const val ADDRESS_FAMILY = "address_family"
        const val CONNECTED_AT = "connected_at"
        const val DOWNLOAD_BYTES_PER_SECOND = "download_bytes_per_second"
        const val UPLOAD_BYTES_PER_SECOND = "upload_bytes_per_second"
        const val DOWNLOADED_BYTES = "downloaded_bytes"
        const val UPLOADED_BYTES = "uploaded_bytes"
        const val RECONNECT_COUNT = "reconnect_count"
        const val ACTIVE_LISTENERS = "active_listeners"
        const val EXIT_IPV4 = "exit_ipv4"
        const val EXIT_IPV6 = "exit_ipv6"
        const val EXIT_CITY = "exit_city"
        const val EXIT_COUNTRY = "exit_country"
        const val EXIT_COUNTRY_CODE = "exit_country_code"
        const val EXIT_FLAG_SVG = "exit_flag_svg"
        const val KILL_SWITCH_STATE = "kill_switch_state"
        const val PLATFORM_LOCKDOWN = "platform_lockdown"
        const val ALWAYS_ON = "always_on"
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
        val transport: String? = null,
        val addressFamily: String? = null,
        val downloadBytesPerSecond: Long = 0,
        val uploadBytesPerSecond: Long = 0,
        val downloadedBytes: Long = 0,
        val uploadedBytes: Long = 0,
        val reconnectCount: Int = 0,
        val activeListeners: List<String> = emptyList(),
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
        transport = source.transport
        addressFamily = source.addressFamily
        downloadBytesPerSecond = source.downloadBytesPerSecond.coerceAtLeast(0)
        uploadBytesPerSecond = source.uploadBytesPerSecond.coerceAtLeast(0)
        downloadedBytes = source.downloadedBytes.coerceAtLeast(0)
        uploadedBytes = source.uploadedBytes.coerceAtLeast(0)
        reconnectCount = source.reconnectCount.coerceAtLeast(0)
        activeListeners = source.activeListeners
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
                exitIpv4 = source.optNullableString("exit_ipv4"),
                exitIpv6 = source.optNullableString("exit_ipv6"),
                exitCity = source.optNullableString("exit_city"),
                exitCountry = source.optNullableString("exit_country"),
                exitCountryCode = source.optNullableString("exit_country_code"),
                exitFlagSvg = source.optNullableString("exit_flag_svg"),
            )
    }

    fun snapshotFields(platform: PlatformFlags): SnapshotFields =
        SnapshotFields(
            phase = phase,
            warning = warning,
            errorCode = errorCode,
            transport = transport,
            addressFamily = addressFamily,
            connectedAt = connectedAt,
            downloadBytesPerSecond = downloadBytesPerSecond,
            uploadBytesPerSecond = uploadBytesPerSecond,
            downloadedBytes = downloadedBytes,
            uploadedBytes = uploadedBytes,
            reconnectCount = reconnectCount,
            activeListeners = activeListeners,
            exitIpv4 = exitIpv4,
            exitIpv6 = exitIpv6,
            exitCity = exitCity,
            exitCountry = exitCountry,
            exitCountryCode = exitCountryCode,
            exitFlagSvg = exitFlagSvg,
            killSwitchState = killSwitchState(platform.tunnelOpen, platform.activeMode),
            platformLockdown = platform.platformLockdown,
            alwaysOn = platform.alwaysOn,
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
            WireKeys.TRANSPORT to fields.transport,
            WireKeys.ADDRESS_FAMILY to fields.addressFamily,
            WireKeys.CONNECTED_AT to fields.connectedAt,
            WireKeys.DOWNLOAD_BYTES_PER_SECOND to fields.downloadBytesPerSecond,
            WireKeys.UPLOAD_BYTES_PER_SECOND to fields.uploadBytesPerSecond,
            WireKeys.DOWNLOADED_BYTES to fields.downloadedBytes,
            WireKeys.UPLOADED_BYTES to fields.uploadedBytes,
            WireKeys.RECONNECT_COUNT to fields.reconnectCount,
            WireKeys.ACTIVE_LISTENERS to ArrayList(fields.activeListeners),
            WireKeys.EXIT_IPV4 to fields.exitIpv4,
            WireKeys.EXIT_IPV6 to fields.exitIpv6,
            WireKeys.EXIT_CITY to fields.exitCity,
            WireKeys.EXIT_COUNTRY to fields.exitCountry,
            WireKeys.EXIT_COUNTRY_CODE to fields.exitCountryCode,
            WireKeys.EXIT_FLAG_SVG to fields.exitFlagSvg,
            WireKeys.KILL_SWITCH_STATE to fields.killSwitchState,
            WireKeys.PLATFORM_LOCKDOWN to fields.platformLockdown,
            WireKeys.ALWAYS_ON to fields.alwaysOn,
        )
    }

    fun toBundle(platform: PlatformFlags): Bundle {
        val entries = wireEntries(platform)
        return Bundle().apply {
            putString(WireKeys.PHASE, entries[WireKeys.PHASE] as String?)
            putString(WireKeys.WARNING, entries[WireKeys.WARNING] as String?)
            putString(WireKeys.ERROR_CODE, entries[WireKeys.ERROR_CODE] as String?)
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
            putString(WireKeys.EXIT_IPV4, entries[WireKeys.EXIT_IPV4] as String?)
            putString(WireKeys.EXIT_IPV6, entries[WireKeys.EXIT_IPV6] as String?)
            putString(WireKeys.EXIT_CITY, entries[WireKeys.EXIT_CITY] as String?)
            putString(WireKeys.EXIT_COUNTRY, entries[WireKeys.EXIT_COUNTRY] as String?)
            putString(WireKeys.EXIT_COUNTRY_CODE, entries[WireKeys.EXIT_COUNTRY_CODE] as String?)
            putString(WireKeys.EXIT_FLAG_SVG, entries[WireKeys.EXIT_FLAG_SVG] as String?)
            putString(WireKeys.KILL_SWITCH_STATE, entries[WireKeys.KILL_SWITCH_STATE] as String?)
            putBoolean(WireKeys.PLATFORM_LOCKDOWN, entries[WireKeys.PLATFORM_LOCKDOWN] as Boolean)
            putBoolean(WireKeys.ALWAYS_ON, entries[WireKeys.ALWAYS_ON] as Boolean)
        }
    }

    fun fingerprint(platform: PlatformFlags): String =
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
            platform.tunnelOpen,
            platform.activeMode,
            platform.platformLockdown,
            platform.alwaysOn,
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
