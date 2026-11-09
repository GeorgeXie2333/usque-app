package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject

/** Full-size native capability object, without any device or account data. */
internal fun expandedNativeCapabilities(): String =
    JSONObject(NetworkQualityFields.capabilities(null).mapValues { true })
        .put("h3_congestion_control_algorithms", JSONArray(listOf("cubic", "reno", "bbr", "bbr3")))
        .put("tls_fingerprints", JSONArray(listOf("default", "chrome155")))
        .put("max_masque_connections", 16)
        .put("future_capability", true)
        .toString()
