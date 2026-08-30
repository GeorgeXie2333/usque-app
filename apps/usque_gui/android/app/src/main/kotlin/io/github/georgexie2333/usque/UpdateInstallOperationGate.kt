package io.github.georgexie2333.usque

import java.util.concurrent.atomic.AtomicLong

/** Invalidates callbacks from update work that outlives its owning Activity. */
internal class UpdateInstallOperationGate {
    private val generation = AtomicLong(0L)

    fun begin(): Long = generation.incrementAndGet()

    fun isActive(token: Long): Boolean = generation.get() == token

    fun finish(token: Long) {
        generation.compareAndSet(token, token + 1L)
    }

    fun invalidateAll() {
        generation.incrementAndGet()
    }
}
