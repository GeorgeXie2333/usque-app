package io.github.georgexie2333.usque

import android.content.Context
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.io.RandomAccessFile
import java.time.Instant
import java.util.ArrayDeque
import java.util.concurrent.CompletableFuture
import java.util.concurrent.Executor
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** Bounded producers; one writer owns rotation, retention, snapshots and clearing. */
internal class AndroidLogStore internal constructor(
    private val directory: File,
    private val executor: Executor =
        Executors.newSingleThreadExecutor { task ->
            Thread(task, "usque-logs").apply {
                isDaemon =
                    true
            }
        },
    private val queueCapacity: Int = 128,
    private val rotateBytes: Long = ROTATE_BYTES,
    private val appendRecord: (
        File,
        ByteArray,
    ) -> Unit = { file, bytes -> FileOutputStream(file, true).use { it.write(bytes) } },
) {
    enum class Event {
        SERVICE_CREATED,
        SERVICE_DESTROYED,
        CONNECTION_REQUESTED,
        CONNECTION_PHASE_CHANGED,
        CONNECTION_FAILED,
        CONNECTION_STOPPED,
        NETWORK_CHANGED,
        VPN_PERMISSION_REVOKED,
        NATIVE_STOP_REQUESTED,
        NATIVE_STOP_COMPLETED,
        NATIVE_STOP_UNCONFIRMED,
    }

    data class Snapshot(
        val lines: String,
        val health: Map<String, Any?>,
    ) {
        fun toMap(): Map<String, Any?> = mapOf("lines" to lines, "health" to health)
    }

    private sealed interface Command {
        data class Write(
            val line: ByteArray,
        ) : Command

        data class Capture(
            val limit: Int,
            val result: CompletableFuture<Snapshot>,
        ) : Command

        data class Clear(
            val result: CompletableFuture<Boolean>,
        ) : Command
    }

    private val lock = Any()
    private val pending = ArrayDeque<Command>()
    private var scheduled = false
    private var queuedEvents = 0
    private var queuedControls = 0
    private var accepting = true
    private var accepted = 0L
    private var written = 0L
    private var dropped = 0L
    private var writeFailures = 0L
    private var clearFailures = 0L
    private var rotationSequence = 0L

    /** No filesystem access, waiting or arbitrary message text on a producer. */
    fun record(
        event: Event,
        phase: String? = null,
        mode: String? = null,
        transport: String? = null,
        errorType: String? = null,
        connectionInstanceId: String? = null,
        connectionGeneration: Long? = null,
        networkGeneration: Long? = null,
        stopTicket: Long? = null,
    ) {
        val entry =
            JSONObject()
                .put("timestamp", Instant.now().toString())
                .put("level", if (event in WARNING_EVENTS) "WARN" else "INFO")
                .put("event", event.name)
        phase?.takeIf(ALLOWED_PHASES::contains)?.let { entry.put("phase", it) }
        mode?.takeIf(ALLOWED_MODES::contains)?.let { entry.put("mode", it) }
        transport?.takeIf(ALLOWED_TRANSPORTS::contains)?.let { entry.put("transport", it) }
        errorType?.takeIf(::allowedErrorCode)?.let { entry.put("error_code", it) }
        connectionInstanceId?.takeIf(INSTANCE_ID::matches)?.let { entry.put("connection_instance_id", it) }
        connectionGeneration?.takeIf { it >= 0 }?.let { entry.put("connection_generation", it) }
        networkGeneration?.takeIf { it >= 0 }?.let { entry.put("network_generation", it) }
        stopTicket?.takeIf { it >= 0 }?.let { entry.put("stop_ticket", it) }
        val line = (entry.toString() + "\n").toByteArray(Charsets.UTF_8)
        if (line.size > MAX_EVENT_BYTES) {
            synchronized(lock) { dropped++ }
            return
        }
        enqueue(Command.Write(line))
    }

    fun capture(maxBytes: Int = MAX_MESSENGER_BYTES): CompletableFuture<Snapshot> {
        val result = CompletableFuture<Snapshot>()
        enqueue(Command.Capture(maxBytes.coerceIn(0, MAX_DIAGNOSTIC_BYTES), result))
        return result
    }

    fun diagnosticSnapshot(maxBytes: Int = MAX_DIAGNOSTIC_BYTES): String =
        capture(maxBytes).get(CONTROL_TIMEOUT_SECONDS, TimeUnit.SECONDS).lines

    /** Reject future producers immediately; the queued barrier erases all old records. */
    fun clearAndPause(): CompletableFuture<Boolean> {
        synchronized(lock) { accepting = false }
        return clearAsync()
    }

    fun resume() {
        synchronized(lock) { accepting = true }
    }

    fun clear() {
        check(clearAsync().get(CONTROL_TIMEOUT_SECONDS, TimeUnit.SECONDS)) { "Android logs could not be cleared" }
    }

    private fun clearAsync(): CompletableFuture<Boolean> {
        val result = CompletableFuture<Boolean>()
        enqueue(Command.Clear(result))
        return result
    }

    private fun enqueue(command: Command) {
        val launch =
            synchronized(lock) {
                if (command is Command.Write) {
                    if (!accepting || queuedEvents >= queueCapacity) {
                        dropped++
                        command.line.fill(0)
                        return
                    }
                    accepted++
                    queuedEvents++
                } else {
                    if (queuedControls >= MAX_CONTROL_COMMANDS) {
                        reject(command)
                        return
                    }
                    queuedControls++
                }
                pending.addLast(command)
                if (scheduled) false else true.also { scheduled = true }
            }
        if (launch) {
            try {
                executor.execute(::drain)
            } catch (_: Exception) {
                synchronized(lock) {
                    while (pending.isNotEmpty()) {
                        val rejected = pending.removeFirst()
                        if (rejected is Command.Write) {
                            dropped++
                            rejected.line.fill(0)
                        } else {
                            reject(rejected)
                        }
                    }
                    queuedEvents = 0
                    queuedControls = 0
                    scheduled = false
                }
            }
        }
    }

    private fun reject(command: Command) {
        when (command) {
            is Command.Write -> {
                command.line.fill(0)
            }

            is Command.Capture -> {
                command.result.complete(
                    Snapshot(
                        "",
                        mapOf(
                            "available" to false,
                            "barrier_completed" to false,
                        ),
                    ),
                )
            }

            is Command.Clear -> {
                command.result.complete(false)
            }
        }
    }

    private fun drain() {
        while (true) {
            val command =
                synchronized(lock) {
                    if (pending.isEmpty()) {
                        scheduled = false
                        return
                    }
                    pending.removeFirst().also {
                        if (it is Command.Write) queuedEvents-- else queuedControls--
                    }
                }
            when (command) {
                is Command.Write -> {
                    try {
                        withDirectoryLock(directory) {
                            purgeExpired(directory)
                            rotateIfNeeded()
                            val active = activeFile(directory)
                            val originalLength = active.length()
                            try {
                                appendRecord(active, command.line)
                            } catch (error: Exception) {
                                // A failed append must not join a partial record to the next event.
                                runCatching { RandomAccessFile(active, "rw").use { it.setLength(originalLength) } }
                                throw error
                            }
                            enforceTotalSize(directory)
                        }
                        synchronized(lock) { written++ }
                    } catch (_: Exception) {
                        synchronized(lock) { writeFailures++ }
                    } finally {
                        command.line.fill(0)
                    }
                }

                is Command.Capture -> {
                    val health =
                        synchronized(lock) {
                            mapOf(
                                "accepted_count" to accepted,
                                "written_count" to written,
                                "dropped_event_count" to dropped,
                                "write_failure_count" to writeFailures,
                                "clear_failure_count" to clearFailures,
                                "queued_event_count" to queuedEvents,
                                "barrier_completed" to true,
                                "persisted_only" to false,
                            )
                        }
                    command.result.complete(readSnapshot(directory, command.limit, health))
                }

                is Command.Clear -> {
                    val cleared =
                        runCatching {
                            withDirectoryLock(directory) {
                                logFiles(directory).all { !it.exists() || it.delete() }
                            }
                        }.getOrDefault(false)
                    synchronized(lock) {
                        if (cleared) {
                            accepted = 0
                            written = 0
                            dropped = 0
                            writeFailures = 0
                            clearFailures = 0
                        } else {
                            clearFailures++
                        }
                    }
                    command.result.complete(cleared)
                }
            }
        }
    }

    private fun rotateIfNeeded() {
        val active = activeFile(directory)
        if (!active.isFile || active.length() < rotateBytes) return
        val rotated =
            File(
                directory,
                "android-engine-${System.currentTimeMillis()}-${(++rotationSequence).toString().padStart(
                    20,
                    '0',
                )}.jsonl",
            )
        check(active.renameTo(rotated)) { "Android log rotation failed" }
    }

    companion object {
        const val MAX_MESSENGER_BYTES = 128 * 1024
        private const val MAX_EVENT_BYTES = 2 * 1024
        private const val MAX_DIAGNOSTIC_BYTES = 2 * 1024 * 1024
        private const val ROTATE_BYTES = 4L * 1024L * 1024L
        private const val MAX_TOTAL_BYTES = 20L * 1024L * 1024L
        private const val RETENTION_MILLIS = 7L * 24L * 60L * 60L * 1_000L
        private const val MAX_CONTROL_COMMANDS = 8
        private const val CONTROL_TIMEOUT_SECONDS = 5L
        private val owners = mutableMapOf<String, AndroidLogStore>()
        private val directoryLocks = mutableMapOf<String, Any>()
        private val ALLOWED_PHASES =
            setOf(
                "disconnected",
                "preparing",
                "connectingH3",
                "connectingH2",
                "connected",
                "degraded",
                "reconnecting",
                "disconnecting",
                "error",
            )
        private val ALLOWED_MODES = setOf("vpn", "socks5", "httpProxy")
        private val ALLOWED_TRANSPORTS = setOf("h3", "h2", "HTTP/3", "HTTP/2")
        private val WARNING_EVENTS = setOf(Event.CONNECTION_FAILED, Event.NATIVE_STOP_UNCONFIRMED)
        private val EVENT_NAMES = Event.entries.map(Event::name).toSet()
        private val INSTANCE_ID = Regex("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
        private val OPTIONAL_KEYS =
            setOf(
                "phase",
                "mode",
                "transport",
                "error_type",
                "error_code",
                "connection_instance_id",
                "connection_generation",
                "network_generation",
                "stop_ticket",
            )

        private fun directory(context: Context): File = File(context.applicationContext.noBackupFilesDir, "logs")

        fun forContext(context: Context): AndroidLogStore {
            val directory = directory(context)
            return synchronized(owners) { owners.getOrPut(directory.absolutePath) { AndroidLogStore(directory) } }
        }

        fun readPersisted(context: Context): Snapshot =
            readSnapshot(
                directory(context),
                MAX_MESSENGER_BYTES,
                mapOf(
                    "persisted_only" to true,
                    "barrier_completed" to false,
                ),
            )

        fun clearPersisted(context: Context) {
            check(
                withDirectoryLock(
                    directory(context),
                ) { logFiles(directory(context)).all { !it.exists() || it.delete() } },
            ) {
                "Android logs could not be cleared"
            }
        }

        /** Revalidate a Messenger snapshot at the final export boundary. */
        fun fromMap(source: Map<String, Any?>): Snapshot? {
            val lines = source["lines"] as? String ?: return null
            if (lines.toByteArray(Charsets.UTF_8).size > MAX_MESSENGER_BYTES) return null
            val records = lines.lineSequence().filter { it.isNotBlank() }.toList()
            val safeRecords = records.filter(::isSafeDiagnosticLine)
            val safe = safeRecords.joinToString("\n", postfix = if (safeRecords.isEmpty()) "" else "\n")
            val raw = source["health"] as? Map<*, *> ?: emptyMap<Any?, Any?>()
            val health = mutableMapOf<String, Any?>()
            for (key in listOf("available", "barrier_completed", "persisted_only", "truncated")) {
                (raw[key] as? Boolean)?.let { health[key] = it }
            }
            for (key in listOf(
                "accepted_count",
                "written_count",
                "dropped_event_count",
                "write_failure_count",
                "clear_failure_count",
                "queued_event_count",
                "read_failure_count",
                "omitted_line_count",
                "source_bytes",
                "exported_bytes",
            )) {
                (raw[key] as? Number)?.toLong()?.takeIf { it >= 0 }?.let { health[key] = it }
            }
            val rejected = (records.size - safeRecords.size).toLong()
            health["omitted_line_count"] =
                minOf((health["omitted_line_count"] as? Long ?: 0L), Long.MAX_VALUE - rejected) + rejected
            health["exported_bytes"] = safe.toByteArray(Charsets.UTF_8).size.toLong()
            return Snapshot(safe, health)
        }

        private fun allowedErrorCode(code: String): Boolean =
            code in DiagnosticsContract.failureCodes || code in DiagnosticsContract.logEventCodes

        private fun isSafeDiagnosticLine(line: String): Boolean {
            if (line.toByteArray(Charsets.UTF_8).size > MAX_EVENT_BYTES) return false
            return runCatching {
                val source = JSONObject(line)
                val keys = source.keys().asSequence().toSet()
                if (!keys.containsAll(setOf("timestamp", "level", "event")) ||
                    keys.any { it !in OPTIONAL_KEYS && it !in setOf("timestamp", "level", "event") }
                ) {
                    return false
                }
                Instant.parse(source.getString("timestamp"))
                if (source.optString("event") !in EVENT_NAMES ||
                    source.optString("level") !in setOf("INFO", "WARN")
                ) {
                    return false
                }
                for ((key, values) in listOf(
                    "phase" to ALLOWED_PHASES,
                    "mode" to ALLOWED_MODES,
                    "transport" to ALLOWED_TRANSPORTS,
                )) {
                    if (source.has(key) && source.opt(key) !in values) return false
                }
                for (key in listOf("error_code", "error_type")) {
                    if (source.has(key) && (source.opt(key) as? String)?.let(::allowedErrorCode) != true) return false
                }
                if (source.has("connection_instance_id") &&
                    (source.opt("connection_instance_id") as? String)?.let(INSTANCE_ID::matches) != true
                ) {
                    return false
                }
                for (key in listOf("connection_generation", "network_generation", "stop_ticket")) {
                    if (source.has(key) &&
                        ((source.opt(key) as? Number)?.toLong()?.takeIf { it >= 0 }) == null
                    ) {
                        return false
                    }
                }
                true
            }.getOrDefault(false)
        }

        private fun readSnapshot(
            directory: File,
            limit: Int,
            initialHealth: Map<String, Any?>,
        ): Snapshot {
            var sourceBytes = 0L
            var failures = 0L
            var omitted = 0L
            var remaining = limit
            var truncated = false
            val records = ArrayDeque<String>()
            val available =
                runCatching {
                    withDirectoryLock(directory) {
                        purgeExpired(directory)
                        for (file in logFiles(directory).sortedWith(
                            compareByDescending<File> { it == activeFile(directory) }
                                .thenByDescending {
                                    it.lastModified()
                                }.thenByDescending { it.name },
                        )) {
                            val length = file.length()
                            sourceBytes += length
                            if (remaining <= 0) {
                                truncated = truncated || length > 0
                                continue
                            }
                            val bytes =
                                try {
                                    RandomAccessFile(file, "r").use { input ->
                                        val count = minOf(length, remaining.toLong() + MAX_EVENT_BYTES).toInt()
                                        ByteArray(count).also {
                                            input.seek(length - count)
                                            input.readFully(it)
                                        }
                                    }
                                } catch (_: Exception) {
                                    failures++
                                    continue
                                }
                            val text = bytes.toString(Charsets.UTF_8)
                            if (!text.endsWith('\n') && text.isNotEmpty()) omitted++
                            val complete = text.lineSequence().toList().dropLast(if (text.endsWith('\n')) 0 else 1)
                            for (line in complete.asReversed()) {
                                if (line.isBlank()) continue
                                if (!isSafeDiagnosticLine(line)) {
                                    omitted++
                                    continue
                                }
                                val size = (line + "\n").toByteArray(Charsets.UTF_8).size
                                if (size > remaining) {
                                    truncated = true
                                    remaining = 0
                                    break
                                }
                                records.addFirst(line)
                                remaining -= size
                            }
                            truncated = truncated || length > bytes.size
                            bytes.fill(0)
                        }
                    }
                }.isSuccess
            val lines = records.joinToString("\n", postfix = if (records.isEmpty()) "" else "\n")
            return Snapshot(
                lines,
                initialHealth +
                    mapOf(
                        "available" to available,
                        "truncated" to truncated,
                        "read_failure_count" to failures,
                        "omitted_line_count" to omitted,
                        "source_bytes" to sourceBytes,
                        "exported_bytes" to lines.toByteArray(Charsets.UTF_8).size,
                    ),
            )
        }

        private fun activeFile(directory: File): File = File(directory, "android-engine.jsonl")

        private fun logFiles(directory: File): List<File> =
            directory
                .listFiles { file ->
                    file.isFile && file.name.startsWith("android-engine") &&
                        file.name.endsWith(".jsonl")
                }?.toList()
                ?: emptyList()

        private fun purgeExpired(directory: File) {
            val cutoff = System.currentTimeMillis() - RETENTION_MILLIS
            logFiles(directory).filter { it.lastModified() in 1 until cutoff }.forEach { check(it.delete()) }
        }

        private fun enforceTotalSize(directory: File) {
            val files = logFiles(directory).sortedBy(File::lastModified)
            var total = files.sumOf(File::length)
            for (file in files) {
                if (total <= MAX_TOTAL_BYTES) break
                if (file == activeFile(directory)) continue
                val length = file.length()
                check(file.delete())
                total -= length
            }
        }

        private fun <T> withDirectoryLock(
            directory: File,
            action: () -> T,
        ): T {
            val localLock = synchronized(directoryLocks) { directoryLocks.getOrPut(directory.absolutePath) { Any() } }
            return synchronized(localLock) {
                check(directory.isDirectory || directory.mkdirs())
                RandomAccessFile(File(directory, "android-engine.lock"), "rw").channel.use { channel ->
                    channel.lock().use { action() }
                }
            }
        }
    }
}
