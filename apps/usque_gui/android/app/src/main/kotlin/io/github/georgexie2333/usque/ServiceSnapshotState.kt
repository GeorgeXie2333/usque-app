package io.github.georgexie2333.usque

import android.os.Bundle
import org.json.JSONObject
import java.time.Instant

/**
 * Mutable connection snapshot held by [UsqueVpnService]. Pure merge/fingerprint/kill-switch
 * and notification text logic lives here so JVM unit tests can cover wire fields without
 * standing up the full VPN service.
 *
 * @param clockMillis injectable clock for captive-portal countdown tests.
 */
internal class ServiceSnapshotState(
    private val clockMillis: () -> Long = System::currentTimeMillis,
) {
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
    var captivePauseDeadlineUnixMillis: Long = 0L
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
        val captivePauseRemainingSeconds: Int,
        val platformLockdown: Boolean,
        val alwaysOn: Boolean,
    )

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

    fun clearCaptivePause() {
        captivePauseDeadlineUnixMillis = 0L
    }

    fun setCaptivePauseDeadline(deadlineUnixMillis: Long) {
        captivePauseDeadlineUnixMillis = deadlineUnixMillis
    }

    fun scheduleCaptivePauseFromNow(seconds: Int) {
        captivePauseDeadlineUnixMillis =
            clockMillis() +
            java.util.concurrent.TimeUnit.SECONDS
                .toMillis(seconds.toLong())
    }

    fun captivePauseRemainingSeconds(): Int {
        if (captivePauseDeadlineUnixMillis <= 0L) return 0
        val remaining = captivePauseDeadlineUnixMillis - clockMillis()
        return if (remaining <= 0L) {
            0
        } else {
            ((remaining + 999L) / 1_000L).coerceAtMost(600L).toInt()
        }
    }

    fun killSwitchState(
        tunnelOpen: Boolean,
        activeMode: String?,
    ): String =
        when {
            phase == "captivePortalPaused" -> "paused"
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
            captivePauseRemainingSeconds = captivePauseRemainingSeconds(),
            platformLockdown = platform.platformLockdown,
            alwaysOn = platform.alwaysOn,
        )

    fun toBundle(platform: PlatformFlags): Bundle {
        val fields = snapshotFields(platform)
        return Bundle().apply {
            putString("phase", fields.phase)
            putString("warning", fields.warning)
            putString("error_code", fields.errorCode)
            putString("transport", fields.transport)
            putString("address_family", fields.addressFamily)
            putString("connected_at", fields.connectedAt)
            putLong("download_bytes_per_second", fields.downloadBytesPerSecond)
            putLong("upload_bytes_per_second", fields.uploadBytesPerSecond)
            putLong("downloaded_bytes", fields.downloadedBytes)
            putLong("uploaded_bytes", fields.uploadedBytes)
            putInt("reconnect_count", fields.reconnectCount)
            putStringArrayList("active_listeners", ArrayList(fields.activeListeners))
            putString("exit_ipv4", fields.exitIpv4)
            putString("exit_ipv6", fields.exitIpv6)
            putString("exit_city", fields.exitCity)
            putString("exit_country", fields.exitCountry)
            putString("exit_country_code", fields.exitCountryCode)
            putString("exit_flag_svg", fields.exitFlagSvg)
            putString("kill_switch_state", fields.killSwitchState)
            putInt("captive_pause_remaining_seconds", fields.captivePauseRemainingSeconds)
            putBoolean("platform_lockdown", fields.platformLockdown)
            putBoolean("always_on", fields.alwaysOn)
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
            captivePauseRemainingSeconds(),
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
