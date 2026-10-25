package io.github.georgexie2333.usque

import org.json.JSONObject

/** Allowlisted metadata only; handoff material has its own private byte channel. */
internal object ProtonVpnFields {
    private val commandKeys =
        setOf("action", "job_id", "profile_id", "challenge_id", "node_id", "name", "offset", "network_policy_version")
    private val jobKeys =
        setOf(
            "id",
            "state",
            "phase",
            "failure",
            "api_code",
            "http_status",
            "challenge_id",
            "expires_at",
            "profile_id",
            "account_network",
            "network_phase",
        )
    private val profileKeys =
        setOf(
            "id",
            "name",
            "identity",
            "node_name",
            "country",
            "expires_at",
            "needs_login",
            "server_id",
            "server_count",
            "cleanup_pending",
        )
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
                    "verification_failed",
                    "select_node",
                    "refresh",
                    "logout",
                    "forget",
                    "list",
                    "servers",
                    "choose_server",
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
        val profiles = value.optJSONArray("profiles")
        require(profiles == null || profiles.length() <= 128)
        val publicProfiles =
            (0 until (profiles?.length() ?: 0)).map { index ->
                val source = requireNotNull(profiles).getJSONObject(index)
                fields(source, profileKeys).toMutableMap().apply {
                    val related = source.optJSONArray("related_ids")
                    require(related == null || related.length() <= 128)
                    put(
                        "related_ids",
                        (0 until (related?.length() ?: 0)).map { item ->
                            val id = requireNotNull(related).getString(item)
                            require(
                                java.util.UUID
                                    .fromString(id)
                                    .toString()
                                    .equals(id, ignoreCase = true),
                            )
                            id
                        },
                    )
                }
            }

        fun pageInteger(key: String): Long {
            val item = scalar(value, key) ?: return 0
            require(item is Int || item is Long)
            return (item as Number).toLong().also { require(it in 0..20000) }
        }
        return mapOf(
            "job" to job,
            "profiles" to publicProfiles,
            "error" to scalar(value, "error"),
            "servers" to array(value, "servers", nodeKeys, 128),
            "server_offset" to pageInteger("server_offset"),
            "server_total" to pageInteger("server_total"),
            "server_revision" to scalar(value, "server_revision"),
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
