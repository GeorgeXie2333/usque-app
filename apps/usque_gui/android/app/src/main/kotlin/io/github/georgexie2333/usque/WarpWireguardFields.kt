package io.github.georgexie2333.usque

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
            "VPN_GATE_REQUEST_INVALID", "WARP_GENERATION_INVALID", "invalid_request" -> "invalid_request"
            else -> "unavailable"
        }

    private val jobKeys = setOf("id", "state", "failure", "profile_id")

    fun response(raw: String): Map<String, Any?> {
        require(raw.length <= 4096)
        val source = JSONObject(raw)
        val job = source.opt("job")
        require(job == null || job == JSONObject.NULL || job is JSONObject)
        val status =
            (job as? JSONObject)?.let { value ->
                require(boundedString(value, "id")?.isNotBlank() == true)
                require(boundedString(value, "state") in setOf("running", "completed", "cancelled", "failed"))
                jobKeys.associateWith { boundedString(value, it) }
            }
        return mapOf("error" to boundedString(source, "error"), "job" to status)
    }

    private fun boundedString(
        source: JSONObject,
        key: String,
    ): String? {
        val value = source.opt(key)
        if (value == null || value == JSONObject.NULL) return null
        require(value is String && value.length <= 512)
        return value
    }
}
