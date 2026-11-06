package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject

/** Only metadata leaves the VPN host; the application target is owner-private. */
internal object ScenarioFields {
    private val actions =
        setOf("list", "save", "delete", "preview", "activate", "preview_restore", "restore", "preview_reset", "reset")

    fun action(request: String): String {
        require(request.toByteArray(Charsets.UTF_8).size <= 256 * 1024)
        return JSONObject(request).getString("action").also { require(it in actions) }
    }

    fun activates(action: String): Boolean = action == "activate" || action == "restore" || action == "reset"

    fun previewRequest(request: JSONObject): JSONObject =
        when (request.getString("action")) {
            "activate" -> {
                JSONObject()
                    .put("action", "preview")
                    .put("id", request.getString("id"))
                    .put("expected_revision", request.getString("expected_revision"))
            }

            "restore" -> {
                JSONObject().put("action", "preview_restore")
            }

            "reset" -> {
                JSONObject().put("action", "preview_reset")
            }

            else -> {
                error("SCENARIO_INVALID_REQUEST")
            }
        }

    fun failure(message: String?): String =
        message?.takeIf { it.matches(Regex("SCENARIO_[A-Z_]{1,64}")) } ?: "SCENARIO_UNCONFIRMED"

    /** Verify credentials using the account's persisted startup profile. Native
     * preview validation owns the proposed scene endpoints and its stale token;
     * they must not be mistaken for an already committed startup profile. */
    fun validateIdentity(
        source: JSONObject,
        loadProfile: (String) -> String?,
        loadSecret: (String, String) -> ByteArray?,
        inspect: (ByteArray) -> Unit,
    ): String {
        val target = source.optJSONObject("preview_target") ?: error("SCENARIO_UNCONFIRMED")
        val id = target.getString("id")
        val persisted =
            runCatching {
                loadProfile(id)?.takeIf { JSONObject(it).getString("id") == id }
            }.getOrNull() ?: error("SCENARIO_ACCOUNT_UNAVAILABLE")
        val secret =
            runCatching { loadSecret(id, persisted) }.getOrNull()
                ?: error("SCENARIO_ACCOUNT_UNAVAILABLE")
        try {
            runCatching { inspect(secret) }.getOrElse { error("SCENARIO_ACCOUNT_UNAVAILABLE") }
        } finally {
            secret.fill(0)
        }
        return id
    }

    fun decode(value: String): Map<String, Any?> {
        require(value.toByteArray(Charsets.UTF_8).size <= 384 * 1024)
        val source = JSONObject(value)
        source.remove("target")
        source.remove("preview_target")
        source.remove("network_settings")
        return objectMap(source)
    }

    /** Saved templates remain readable without manufacturing a live owner or receipt.
     * Only drift computed from the saved configuration accompanies them. */
    fun persistedOnly(value: String): Map<String, Any?> {
        require(value.toByteArray(Charsets.UTF_8).size <= 384 * 1024)
        val source = JSONObject(value)
        val savedState = source.optJSONObject("state")
        val state = JSONObject().put("apply_status", "unknown")
        for (key in listOf("modified", "template_updated", "bound_account_missing")) {
            if (savedState?.opt(key) == true) state.put(key, true)
        }
        val projected =
            JSONObject()
                .put("runtime_observed", false)
                .put("state", state)
        for (key in listOf(
            "scenarios",
            "invalid_scenario_ids",
            "catalogue_revision",
            "selection",
            "current_settings",
            "current_account_id",
            "current_settings_error",
        )) {
            if (source.has(key)) projected.put(key, source.get(key))
        }
        return objectMap(projected)
    }

    private fun objectMap(source: JSONObject): Map<String, Any?> =
        source.keys().asSequence().associateWith { convert(source.get(it)) }

    private fun convert(value: Any?): Any? =
        when (value) {
            null, JSONObject.NULL -> null
            is JSONObject -> objectMap(value)
            is JSONArray -> (0 until value.length()).map { convert(value.get(it)) }
            else -> value
        }
}
