package io.github.georgexie2333.usque

import org.json.JSONObject

/** Allowlisted metadata only; handoff material has its own private byte channel. */
internal object ProtonVpnFields {
    private val commandKeys = setOf("action", "job_id", "profile_id", "challenge_id", "node_id", "name")
    private val jobKeys =
        setOf("id", "state", "phase", "failure", "api_code", "http_status", "challenge_id", "expires_at", "profile_id")
    private val profileKeys = setOf("id", "name", "identity", "node_name", "country", "expires_at", "needs_login")
    private val nodeKeys = setOf("id", "name", "country", "city", "load")

    fun command(raw: String): String {
        require(raw.length in 1..4096)
        val value = JSONObject(raw)
        require(value.keys().asSequence().all { it in commandKeys })
        require(
            value.optString("action") in
                setOf(
                    "guest",
                    "login",
                    "get",
                    "cancel",
                    "totp",
                    "handoff",
                    "verify",
                    "select_node",
                    "refresh",
                    "logout",
                    "forget",
                    "list",
                ),
        )
        return value.toString()
    }

    fun response(raw: ByteArray): Map<String, Any?> {
        require(raw.size <= 256 * 1024)
        val value = JSONObject(raw.toString(Charsets.UTF_8))
        val job =
            value.optJSONObject("job")?.let { source ->
                require(
                    source.optString("state") in
                        setOf(
                            "running",
                            "waiting_totp",
                            "waiting_verification",
                            "waiting_node",
                            "completed",
                            "cancelled",
                            "failed",
                        ),
                )
                fields(source, jobKeys).toMutableMap().apply {
                    put("nodes", array(source, "nodes", nodeKeys, 512))
                }
            }
        return mapOf(
            "job" to job,
            "profiles" to array(value, "profiles", profileKeys, 128),
            "error" to scalar(value, "error"),
        )
    }

    private fun array(
        source: JSONObject,
        key: String,
        keys: Set<String>,
        limit: Int,
    ): List<Map<String, Any?>> {
        val values = source.optJSONArray(key) ?: return emptyList()
        require(values.length() <= limit)
        return (0 until values.length()).map { fields(values.getJSONObject(it), keys) }
    }

    private fun fields(
        source: JSONObject,
        keys: Set<String>,
    ): Map<String, Any?> = keys.associateWith { scalar(source, it) }

    private fun scalar(
        source: JSONObject,
        key: String,
    ): Any? {
        val value = source.opt(key)
        if (value == null || value == JSONObject.NULL) return null
        require(value is String || value is Boolean || value is Int || value is Long)
        if (value is String) require(value.length <= 512 && value.none { it.isISOControl() })
        return value
    }
}
