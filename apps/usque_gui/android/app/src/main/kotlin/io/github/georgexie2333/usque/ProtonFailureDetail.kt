package io.github.georgexie2333.usque

/** Provider numeric context only. Remote descriptions and identifiers are never admitted. */
internal object ProtonFailureDetail {
    private val agent = Regex("^proton agent ([0-9]{1,10})$")
    private val api = Regex("^proton api ([0-9]{1,10}) http ([1-5][0-9]{2})$")

    fun isSafe(value: String): Boolean {
        val match = agent.matchEntire(value) ?: api.matchEntire(value) ?: return false
        return match.groupValues[1].toULongOrNull()?.let { it <= UInt.MAX_VALUE.toULong() } == true
    }
}
