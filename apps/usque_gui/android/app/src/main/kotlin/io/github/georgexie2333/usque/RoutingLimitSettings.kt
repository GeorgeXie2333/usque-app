package io.github.georgexie2333.usque

/** Native routing limits; missing, malformed or out-of-range values report 0. */
internal object RoutingLimitSettings {
    private const val MAX_RULES_PER_KIND = 1 shl 16
    private const val MAX_TARGET_BYTES = 64 shl 20

    fun capabilities(value: String?): Map<String, Int> {
        val source = NetworkQualityFields.capabilityObject(value)

        fun bounded(
            key: String,
            maximum: Int,
        ): Int = (source?.opt(key) as? Int)?.takeIf { it in 1..maximum } ?: 0
        return mapOf(
            "max_routing_rules_per_kind" to bounded("max_routing_rules_per_kind", MAX_RULES_PER_KIND),
            "max_routing_target_bytes" to bounded("max_routing_target_bytes", MAX_TARGET_BYTES),
        )
    }
}
