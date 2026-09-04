package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject

/** On-demand native timeline only; never appended to snapshot/event messages. */
internal object NativeTimelineFields {
    const val MAX_JSON_BYTES = 192 * 1024

    fun decode(value: String?): Map<String, Any?>? {
        if (value == null || value.length > MAX_JSON_BYTES || value.toByteArray(Charsets.UTF_8).size > MAX_JSON_BYTES) {
            return null
        }
        return runCatching {
            val source = JSONObject(value)
            if (source.optInt("schema_version") != 1 || source.optJSONArray("events") == null) return null
            val safe = AndroidMaintenance.sanitizeConnectionTimeline(objectMap(source, 0), includeLiveTimestamps = true)
            objectMap(safe, 0)
        }.getOrNull()
    }

    private fun objectMap(
        source: JSONObject,
        depth: Int,
    ): Map<String, Any?> {
        if (depth > 4) return emptyMap()
        return source
            .keys()
            .asSequence()
            .take(32)
            .associateWith { key -> convert(source.opt(key), depth + 1) }
    }

    private fun convert(
        value: Any?,
        depth: Int,
    ): Any? =
        when (value) {
            is JSONObject -> {
                objectMap(value, depth)
            }

            is JSONArray -> {
                if (depth > 4) {
                    emptyList<Any?>()
                } else {
                    (maxOf(0, value.length() - 256) until value.length()).map { convert(value.opt(it), depth + 1) }
                }
            }

            is Number, is Boolean, is String -> {
                value
            }

            else -> {
                null
            }
        }
}
