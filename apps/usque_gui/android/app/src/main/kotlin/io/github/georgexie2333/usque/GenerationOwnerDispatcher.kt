package io.github.georgexie2333.usque

import java.util.concurrent.CompletableFuture

internal class GenerationSupersededException(
    val generation: Long,
) : IllegalStateException("Connection generation $generation is no longer current.")

/** Serializes platform mutation with generation changes on the thread that owns both. */
internal class GenerationOwnerDispatcher(
    private val isCurrent: (Long) -> Boolean,
    private val dispatch: (() -> Unit) -> Unit,
) {
    /**
     * [action] must perform establishment and publish ownership without suspending or awaiting.
     * Canceling the future revokes queued work; an action that has started retains its effects.
     */
    fun <T> submit(
        generation: Long,
        action: () -> T,
    ): CompletableFuture<T> {
        val result = CompletableFuture<T>()
        try {
            dispatch {
                if (result.isDone) return@dispatch
                try {
                    if (!isCurrent(generation)) {
                        result.completeExceptionally(GenerationSupersededException(generation))
                    } else if (!result.isDone) {
                        result.complete(action())
                    }
                } catch (error: Throwable) {
                    result.completeExceptionally(error)
                }
            }
        } catch (error: Throwable) {
            result.completeExceptionally(error)
        }
        return result
    }
}
