package io.github.georgexie2333.usque

import org.json.JSONObject
import java.net.InetAddress

internal enum class IdentityReplacementState {
    None,
    Preparing,
    Armed,
}

/** Trusted persisted startup profile, including replacement/rollback authority.
 * A scene's proposed endpoints are not yet a persisted startup profile. */
internal data class IdentityReplacementAuthority(
    val state: IdentityReplacementState,
    val endpointIpv4: String,
    val endpointIpv6: String,
    val endpointPort: Int,
    val sni: String,
    val endpointReady: Boolean,
    val profileJson: String,
) {
    /** Inputs stay caller-owned; returned bytes are separately owned and must
     * be cleared by the credential consumer. Read-only preflight never repairs
     * or deletes replacement records. */
    fun loadSecret(
        profileJson: String,
        current: ByteArray?,
        encodedRollback: ByteArray?,
        readOnly: Boolean,
        readCurrent: () -> ByteArray?,
        discardRollback: () -> Unit,
    ): ByteArray? {
        if (!matches(profileJson)) return null
        return when (state) {
            IdentityReplacementState.Preparing -> {
                current?.copyOf()
            }

            IdentityReplacementState.Armed -> {
                val rollback = IdentityReplacementRollbackCodec.decode(encodedRollback ?: return null)
                try {
                    rollback.identity?.copyOf()
                } finally {
                    rollback.clear()
                }
            }

            IdentityReplacementState.None -> {
                if (encodedRollback != null && !readOnly) runCatching(discardRollback)
                readCurrent()
            }
        }
    }

    fun matches(profileJson: String): Boolean =
        runCatching {
            val profile = JSONObject(profileJson)

            fun numericAddress(value: String): ByteArray? {
                if (value.isEmpty() ||
                    value.any { character ->
                        !character.isDigit() &&
                            character.lowercaseChar() !in 'a'..'f' &&
                            character != '.' &&
                            character != ':'
                    }
                ) {
                    return null
                }
                return InetAddress.getByName(value).address
            }
            endpointReady &&
                numericAddress(profile.getString("endpoint_v4"))
                    ?.contentEquals(numericAddress(endpointIpv4) ?: return@runCatching false) == true &&
                numericAddress(profile.getString("endpoint_v6"))
                    ?.contentEquals(numericAddress(endpointIpv6) ?: return@runCatching false) == true &&
                profile.getInt("endpoint_port") == endpointPort &&
                profile.getString("sni").equals(sni, ignoreCase = true)
        }.getOrDefault(false)

    companion object {
        fun fromCatalog(
            profileId: String,
            catalogJson: String,
        ): IdentityReplacementAuthority? =
            runCatching {
                val catalog = JSONObject(catalogJson)
                val profiles = catalog.optJSONArray("profiles") ?: return@runCatching null
                val profile =
                    (0 until profiles.length())
                        .asSequence()
                        .mapNotNull(profiles::optJSONObject)
                        .firstOrNull { it.optString("id") == profileId } ?: return@runCatching null
                val pendingValues = catalog.optJSONArray("pending_identity_replacements") ?: return@runCatching null
                val pending = (0 until pendingValues.length()).any { pendingValues.optString(it) == profileId }
                val state =
                    if (!pending) {
                        IdentityReplacementState.None
                    } else {
                        val armedValues = catalog.optJSONArray("armed_identity_replacements") ?: return@runCatching null
                        val armed = (0 until armedValues.length()).any { armedValues.optString(it) == profileId }
                        if (armed) IdentityReplacementState.Armed else IdentityReplacementState.Preparing
                    }
                IdentityReplacementAuthority(
                    state = state,
                    endpointIpv4 = profile.getString("endpoint_v4"),
                    endpointIpv6 = profile.getString("endpoint_v6"),
                    endpointPort = profile.getInt("endpoint_port"),
                    sni = profile.getString("sni"),
                    endpointReady = profile.optBoolean("zero_trust_endpoint_ready", true),
                    profileJson = profile.toString(),
                )
            }.getOrNull()
    }
}
