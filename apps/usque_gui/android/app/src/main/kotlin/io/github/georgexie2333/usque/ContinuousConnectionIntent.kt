package io.github.georgexie2333.usque

import org.json.JSONObject

internal object ContinuousConnectionIntent {
    fun continuous(
        requested: String,
        current: String?,
    ): Boolean {
        val source = JSONObject(requested)
        return if (source.has("continuous")) {
            source.optBoolean("continuous")
        } else {
            current?.let { JSONObject(it).optBoolean("continuous") } == true
        }
    }

    fun inherit(
        requested: String,
        current: String?,
    ): String = JSONObject(requested).put("continuous", continuous(requested, current)).toString()

    fun ready(
        phase: String,
        nativeActive: Boolean,
        tunnelEnabled: Boolean,
        tunAttached: Boolean,
        reconfiguring: Boolean,
    ): Boolean =
        nativeActive && !reconfiguring && phase in setOf("connected", "degraded") &&
            (!tunnelEnabled || tunAttached)
}
