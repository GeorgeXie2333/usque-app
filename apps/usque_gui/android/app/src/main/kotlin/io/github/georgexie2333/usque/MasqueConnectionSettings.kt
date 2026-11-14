package io.github.georgexie2333.usque

/** Native capability only; missing, malformed or unsupported limits stay disabled. */
internal object MasqueConnectionSettings {
    fun maximum(value: String?): Int {
        val count = NetworkQualityFields.capabilityObject(value)?.opt("max_masque_connections") as? Int
        return count?.takeIf { it in 1..16 } ?: 0
    }
}
