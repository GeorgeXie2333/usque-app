package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject

/** Metadata-only replies. Never forward arbitrary native JSON or secret fields. */
internal object WarpWireguardFields {
    // JNI exception text may contain configuration details. Forward only this
    // fixed vocabulary; registration HTTP failures arrive in bounded metadata.
    fun failureCode(code: String?): String =
        when (code) {
            "WARP_IDENTITY_REQUIRED", "identity_required" -> "identity_required"
            "VPN_GATE_IDENTITY_INVALID", "identity_invalid" -> "identity_invalid"
            "CHAIN_CRYPTO_UNAVAILABLE", "secure_storage_failed" -> "secure_storage_failed"
            "VPN_GATE_REQUEST_INVALID", "WARP_SCAN_INVALID", "invalid_request" -> "invalid_request"
            else -> "unavailable"
        }

    private val keys =
        setOf(
            "job",
            "history",
            "results",
            "next_cursor",
            "error",
            "id",
            "kind",
            "state",
            "mode",
            "ipv6",
            "completed",
            "total",
            "working",
            "countries",
            "created_at",
            "failure",
            "profile_id",
            "index",
            "endpoint",
            "host",
            "port",
            "checked_at",
            "ipv4",
            "exit_ip",
            "country",
            "colo",
            "response_ms",
        )

    fun response(raw: String): Map<String, Any?> {
        require(raw.length <= 256 * 1024)
        return objectValue(JSONObject(raw), 0)
    }

    private fun objectValue(
        source: JSONObject,
        depth: Int,
    ): Map<String, Any?> {
        require(depth <= 4)
        return source
            .keys()
            .asSequence()
            .filter { it in keys }
            .associateWith { value(source.opt(it), depth + 1) }
    }

    private fun value(
        source: Any?,
        depth: Int,
    ): Any? =
        when (source) {
            null, JSONObject.NULL -> {
                null
            }

            is JSONObject -> {
                objectValue(source, depth)
            }

            is JSONArray -> {
                require(source.length() <= 256 && depth <= 4)
                List(source.length()) { value(source.opt(it), depth + 1) }
            }

            is String -> {
                source.also { require(it.length <= 512) }
            }

            is Number, is Boolean -> {
                source
            }

            else -> {
                null
            }
        }
}
