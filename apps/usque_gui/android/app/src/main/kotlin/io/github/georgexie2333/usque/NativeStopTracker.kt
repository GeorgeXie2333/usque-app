package io.github.georgexie2333.usque

/** A UI disconnect is not evidence that the native worker finished cleanup. */
internal class NativeStopTracker {
    private var nextTicket = 0L
    private var lastResult = 0L
    private var pending = 0
    private var unconfirmed = false

    @Synchronized
    fun begin(): Long {
        pending++
        return ++nextTicket
    }

    @Synchronized
    fun complete(
        ticket: Long,
        confirmed: Boolean,
    ) {
        check(pending > 0)
        pending--
        if (ticket >= lastResult) {
            lastResult = ticket
            unconfirmed = !confirmed
        }
    }

    @Synchronized
    fun pendingCleanup(): Boolean = pending != 0 || unconfirmed
}
