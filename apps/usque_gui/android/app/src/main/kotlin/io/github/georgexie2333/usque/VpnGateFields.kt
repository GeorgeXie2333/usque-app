package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject

/** Metadata-only bridge; OpenVPN configuration bytes never enter Flutter. */
internal object VpnGateFields {
    fun decodeStatus(raw: String?): Map<String, Any?>? =
        raw?.takeIf { it.length <= 16 * 1024 }?.let { runCatching { status(JSONObject(it)) }.getOrNull() }

    fun stoppedStatus(source: JSONObject?): String? =
        status(source)
            ?.takeUnless { it["stage"] == "disabled" }
            ?.toMutableMap()
            ?.apply {
                put("stage", "error")
                put("warp_stage", "disconnected")
                put("network", null)
            }?.let { JSONObject(it).toString() }

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

    private fun server(source: JSONObject): Map<String, Any?> =
        fields(source, serverKeys).toMutableMap().apply {
            put(
                "pool",
                source.optJSONObject("pool")?.let {
                    fields(
                        it,
                        setOf(
                            "first_seen_at",
                            "last_seen_at",
                            "present_in_latest_source",
                            "tcp_status",
                            "tcp_checked_at",
                            "tcp_connect_ms",
                            "in_pool",
                        ),
                    )
                },
            )
            put(
                "favorite",
                source.optJSONObject("favorite")?.let {
                    fields(it, setOf("config_sha256", "saved_at_unix_ms", "latest_config_sha256"))
                },
            )
        }

    fun status(source: JSONObject?): Map<String, Any?>? =
        source?.let {
            fields(
                it,
                setOf(
                    "stage",
                    "generation",
                    "failure",
                    "warp_stage",
                    "dns_unavailable",
                    "active_endpoint",
                    "attempt_count",
                    "candidate_count",
                    "attempt_failures",
                ),
            ).toMutableMap().apply {
                put("current_server", it.optJSONObject("current_server")?.let { node -> server(node) })
                put("current_profile", ChainProfileFields.summary(it.optJSONObject("current_profile")))
                put(
                    "warp_observation",
                    it.optJSONObject("warp_observation")?.let { observed ->
                        WarpWireguardFields.response(observed.toString())
                    },
                )
                put(
                    "attempting_endpoint",
                    it.optJSONObject("attempting_endpoint")?.let { endpoint ->
                        fields(endpoint, setOf("host", "port"))
                    },
                )
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
                "favorite_count",
                "source_fetched_at",
            ),
        ).toMutableMap().apply {
            put("servers", List(servers.length()) { server(servers.getJSONObject(it)) })
            put(
                "countries",
                List(countries.length()) {
                    fields(countries.getJSONObject(it), setOf("country_code", "country_name", "server_count"))
                },
            )
            put("status", status(source.optJSONObject("status")))
            put("saved_server", source.optJSONObject("saved_server")?.let { server(it) })
            put(
                "node_progress",
                source.optJSONObject("node_progress")?.let {
                    fields(it, setOf("operation_id", "server_id", "config_sha256", "stage", "error"))
                },
            )
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
