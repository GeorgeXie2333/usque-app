package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject

/** Only metadata leaves the VPN host; the application target is owner-private. */
internal object ScenarioFields {
    private val actions = setOf("list", "save", "delete", "preview", "activate", "preview_restore", "restore")

    fun action(request: String): String {
        require(request.toByteArray(Charsets.UTF_8).size <= 256 * 1024)
        return JSONObject(request).getString("action").also { require(it in actions) }
    }

    fun activates(action: String): Boolean = action == "activate" || action == "restore"

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

            else -> {
                error("SCENARIO_INVALID_REQUEST")
            }
        }

    fun failure(message: String?): String =
        message?.takeIf { it.matches(Regex("SCENARIO_[A-Z_]{1,64}")) } ?: "SCENARIO_UNCONFIRMED"

    /** Use the same read-only authority as startup, including endpoint readiness
     * and any armed identity replacement's rollback record. */
    fun validateIdentity(
        source: JSONObject,
        loadSecret: (String, String) -> ByteArray?,
        inspect: (ByteArray) -> Unit,
    ): String {
        val target = source.optJSONObject("preview_target") ?: error("SCENARIO_UNCONFIRMED")
        val id = target.getString("id")
        val secret =
            runCatching { loadSecret(id, target.toString()) }.getOrNull()
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
