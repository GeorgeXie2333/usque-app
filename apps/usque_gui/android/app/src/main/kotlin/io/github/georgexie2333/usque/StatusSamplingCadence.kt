package io.github.georgexie2333.usque

/** Monotonic fixed-rate deadlines. Late executions consume one beat, not a backlog. */
internal class StatusSamplingCadence(
    private val intervalMillis: Long,
) {
    private var nextDue: Long? = null

    init {
        require(intervalMillis > 0)
    }

    fun takeDue(nowMillis: Long): Boolean {
        val due = nextDue ?: nowMillis
        // Scheduler/elapsedRealtime rounding must not drop an ordinary tick
        // that is a few milliseconds early. This does not move the deadline.
        val effectiveNow = nowMillis + minOf(50L, intervalMillis / 10)
        if (effectiveNow < due) return false
        nextDue = due + ((effectiveNow - due) / intervalMillis + 1) * intervalMillis
        return true
    }

    fun delayUntilNext(nowMillis: Long): Long {
        var due = nextDue ?: nowMillis
        if (due < nowMillis) {
            due += ((nowMillis - due) / intervalMillis + 1) * intervalMillis
            nextDue = due
        }
        return due - nowMillis
    }
}
