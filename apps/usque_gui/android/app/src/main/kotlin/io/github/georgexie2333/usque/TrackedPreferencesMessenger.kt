package io.github.georgexie2333.usque

import io.flutter.plugin.common.BinaryMessenger
import java.nio.ByteBuffer
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Preferences handlers synchronously finish their commit/runBlocking operation.
 * Receive on the platform thread, hold the process, then dispatch the copied
 * message. A lease acquired inside Flutter's background handler would miss work
 * that was already accepted but had not started when the Activity was destroyed.
 */
internal class TrackedPreferencesMessenger(
    private val delegate: BinaryMessenger,
    private val holdWork: () -> AutoCloseable = UiProcessReclaimer::holdBackgroundWork,
    private val awaitCleanup: (List<ExecutorService>) -> Unit = UiProcessReclaimer::awaitCleanup,
    private val createExecutor: () -> ExecutorService = {
        Executors.newSingleThreadExecutor { task ->
            Thread(task, "usque-preferences").apply { isDaemon = true }
        }
    },
) : BinaryMessenger {
    private class TrackedQueue(
        val executor: ExecutorService,
    ) : BinaryMessenger.TaskQueue

    private class SingleReply(
        private val delegate: BinaryMessenger.BinaryReply,
    ) : BinaryMessenger.BinaryReply {
        private val completed = AtomicBoolean(false)

        override fun reply(reply: ByteBuffer?) {
            if (completed.compareAndSet(false, true)) delegate.reply(reply)
        }
    }

    private val queues = mutableListOf<TrackedQueue>()
    private val channels = mutableSetOf<String>()
    private var accepting = true
    private var cleanupFinished = false

    override fun makeBackgroundTaskQueue(): BinaryMessenger.TaskQueue =
        makeBackgroundTaskQueue(BinaryMessenger.TaskQueueOptions())

    override fun makeBackgroundTaskQueue(options: BinaryMessenger.TaskQueueOptions): BinaryMessenger.TaskQueue {
        val queue = TrackedQueue(createExecutor())
        // The preferences plugin requests serial queues. Keeping that policy
        // also orders writes if a future API supplies different queue options.
        if (cleanupFinished) {
            queue.executor.shutdown()
        } else {
            queues.add(queue)
        }
        return queue
    }

    override fun setMessageHandler(
        channel: String,
        handler: BinaryMessenger.BinaryMessageHandler?,
    ) = setMessageHandler(channel, handler, null)

    override fun setMessageHandler(
        channel: String,
        handler: BinaryMessenger.BinaryMessageHandler?,
        taskQueue: BinaryMessenger.TaskQueue?,
    ) {
        if (handler == null || !accepting) {
            channels.remove(channel)
            delegate.setMessageHandler(channel, null)
            return
        }
        val queue = taskQueue?.let { requested -> queues.firstOrNull { it === requested } }
        require(taskQueue == null || queue != null) { "Unknown preferences task queue" }
        channels.add(channel)
        // No delegate task queue: acquire the lease before returning from the
        // platform callback, while the original native buffer is still valid.
        delegate.setMessageHandler(channel) { message, reply ->
            receive(message, reply, handler, queue?.executor)
        }
    }

    private fun receive(
        message: ByteBuffer?,
        reply: BinaryMessenger.BinaryReply,
        handler: BinaryMessenger.BinaryMessageHandler,
        executor: ExecutorService?,
    ) {
        val singleReply = SingleReply(reply)
        if (!accepting) {
            singleReply.reply(null)
            return
        }
        val lease = holdWork()
        val released = AtomicBoolean(false)
        val release = { if (released.compareAndSet(false, true)) lease.close() }
        try {
            val owned = message?.let(::copyMessage)
            val work =
                Runnable {
                    try {
                        handler.onMessage(owned, singleReply)
                    } catch (error: Throwable) {
                        if (error is Error) throw error
                    } finally {
                        try {
                            // Also completes rejected/malformed synchronous
                            // handlers. A prior successful reply is preserved.
                            singleReply.reply(null)
                        } finally {
                            release()
                        }
                    }
                }
            if (executor == null) {
                work.run()
            } else {
                executor.execute(work)
            }
        } catch (error: Throwable) {
            try {
                singleReply.reply(null)
            } finally {
                release()
            }
            if (error is Error) throw error
        }
    }

    fun stopAccepting() {
        if (!accepting) return
        accepting = false
        channels.toList().forEach { delegate.setMessageHandler(it, null) }
        channels.clear()
    }

    fun finishCleanup() {
        if (cleanupFinished) return
        stopAccepting()
        cleanupFinished = true
        val retired = queues.map { it.executor }
        retired.forEach { it.shutdown() }
        awaitCleanup(retired)
        queues.clear()
    }

    override fun send(
        channel: String,
        message: ByteBuffer?,
    ) = delegate.send(channel, message)

    override fun send(
        channel: String,
        message: ByteBuffer?,
        callback: BinaryMessenger.BinaryReply?,
    ) = delegate.send(channel, message, callback)

    override fun enableBufferingIncomingMessages() = delegate.enableBufferingIncomingMessages()

    override fun disableBufferingIncomingMessages() = delegate.disableBufferingIncomingMessages()

    private fun copyMessage(message: ByteBuffer): ByteBuffer {
        val copy = ByteBuffer.allocateDirect(message.remaining()).order(message.order())
        copy.put(message.duplicate())
        copy.flip()
        return copy
    }
}
