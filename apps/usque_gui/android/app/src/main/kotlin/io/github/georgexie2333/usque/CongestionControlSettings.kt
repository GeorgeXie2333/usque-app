package io.github.georgexie2333.usque

/** Configuration names and session binding only; never changes system TCP. */
internal object CongestionControlSettings {
    val algorithms = listOf("cubic", "reno", "bbr", "bbr3")

    fun token(value: Any?): String? = (value as? String)?.takeIf { it in algorithms }

    fun capabilities(value: String?): List<String> {
        val source = NetworkQualityFields.capabilityObject(value)?.optJSONArray("h3_congestion_control_algorithms")
        return (0 until minOf(source?.length() ?: 0, 32)).mapNotNull { token(source?.opt(it)) }.distinct()
    }
}
