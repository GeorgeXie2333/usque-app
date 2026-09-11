package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject

/** Metadata-only bridge; OpenVPN configuration bytes never enter Flutter. */
internal object VpnGateFields {
    fun decodeStatus(raw: String?): Map<String, Any?>? =
        raw?.takeIf { it.length <= 16 * 1024 }?.let { runCatching { status(JSONObject(it)) }.getOrNull() }

    private val serverKeys =
        setOf(
            "id",
            "hostname",
            "ip",
            "country_code",
            "country_name",
            "score",
            "ping_ms",
            "speed_bps",
            "num_vpn_sessions",
            "config_sha256",
            "unsupported_reason",
        )

    private fun fields(
        source: JSONObject,
        keys: Set<String>,
    ): Map<String, Any?> =
        source
            .keys()
            .asSequence()
            .filter { it in keys }
            .associateWith { convert(source.opt(it)) }

    fun status(source: JSONObject?): Map<String, Any?>? =
        source?.let {
            fields(it, setOf("stage", "generation", "failure", "warp_stage")).toMutableMap().apply {
                put("current_server", it.optJSONObject("current_server")?.let { server -> fields(server, serverKeys) })
                put(
                    "network",
                    it.optJSONObject("network")?.let { network ->
                        fields(network, setOf("ipv4", "ipv6", "dns_servers", "mtu"))
                    },
                )
            }
        }

    fun directory(raw: String): Map<String, Any?> {
        require(raw.length <= 512 * 1024)
        val source = JSONObject(raw)
        val servers = source.getJSONArray("servers")
        val countries = source.getJSONArray("countries")
        require(servers.length() <= 100 && countries.length() <= 676)
        return fields(
            source,
            setOf(
                "total",
                "source_server_count",
                "fetched_at_unix_ms",
                "source_url",
                "refresh_stage",
                "refresh_failures",
                "cached",
            ),
        ).toMutableMap().apply {
            put("servers", List(servers.length()) { fields(servers.getJSONObject(it), serverKeys) })
            put(
                "countries",
                List(countries.length()) {
                    fields(countries.getJSONObject(it), setOf("country_code", "country_name", "server_count"))
                },
            )
            put("status", status(source.optJSONObject("status")))
            put("saved_server", source.optJSONObject("saved_server")?.let { fields(it, serverKeys) })
        }
    }

    private fun convert(value: Any?): Any? =
        when (value) {
            null, JSONObject.NULL -> null
            is JSONArray -> List(value.length()) { convert(value.opt(it)) }
            is String, is Number, is Boolean -> value
            else -> null
        }
}
