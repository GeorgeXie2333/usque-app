package io.github.georgexie2333.usque

/** A stale lifetime request cannot retire a different, still-owned connection. */
internal object ControlServiceStart {
    enum class Decision {
        RETAIN_CURRENT,
        RETAIN_REPLACEMENT,
        STOP_IDLE,
    }

    fun decide(
        expectedGeneration: Long,
        currentGeneration: Long,
        connectionWanted: Boolean,
        runtimeActive: Boolean,
        tunOwned: Boolean,
        handoffRetained: Boolean,
        reconfiguring: Boolean,
        cleanupPending: Boolean,
    ): Decision =
        if (!connectionWanted && !runtimeActive && !tunOwned && !handoffRetained && !reconfiguring && !cleanupPending) {
            Decision.STOP_IDLE
        } else if (expectedGeneration == currentGeneration) {
            Decision.RETAIN_CURRENT
        } else {
            Decision.RETAIN_REPLACEMENT
        }

    fun requestOwnership(
        alreadyStarted: Boolean,
        start: () -> Unit,
    ): Boolean {
        if (alreadyStarted) return true
        return try {
            start()
            true
        } catch (_: RuntimeException) {
            false
        }
    }
}
