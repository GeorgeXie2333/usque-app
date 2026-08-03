package io.github.georgexie2333.usque

import android.content.Context
import java.io.File
import java.io.FileOutputStream
import java.time.Instant

/**
 * Small privacy-first local event log. Callers can only provide enumerated
 * events and tightly constrained state tokens; profiles, identities, hostnames,
 * IP addresses, listener addresses, and exception messages have no input path.
 */
internal class AndroidLogStore internal constructor(
    private val directory: File,
) {
    constructor(context: Context) :
        this(File(context.applicationContext.noBackupFilesDir, LOG_DIRECTORY))

    enum class Event {
        SERVICE_CREATED,
        SERVICE_DESTROYED,
        CONNECTION_REQUESTED,
        CONNECTION_PHASE_CHANGED,
        CONNECTION_FAILED,
        CONNECTION_STOPPED,
        NETWORK_CHANGED,
        CAPTIVE_PORTAL_PAUSED,
        CAPTIVE_PORTAL_RESUMED,
        VPN_PERMISSION_REVOKED,
    }

    @Synchronized
    fun record(
        event: Event,
        phase: String? = null,
        mode: String? = null,
        transport: String? = null,
        errorType: String? = null,
    ) {
        try {
            if (!directory.isDirectory && !directory.mkdirs()) return
            purgeExpired()
            rotateIfNeeded()
            val entry =
                buildString {
                    append("{\"timestamp\":\"")
                    append(Instant.now())
                    append("\",\"level\":\"")
                    append(if (event == Event.CONNECTION_FAILED) "WARN" else "INFO")
                    append("\",\"event\":\"")
                    append(event.name)
                    append('"')
                    phase?.takeIf(ALLOWED_PHASES::contains)?.let {
                        append(",\"phase\":\"").append(it).append('"')
                    }
                    mode?.takeIf(ALLOWED_MODES::contains)?.let {
                        append(",\"mode\":\"").append(it).append('"')
                    }
                    transport?.takeIf(ALLOWED_TRANSPORTS::contains)?.let {
                        append(",\"transport\":\"").append(it).append('"')
                    }
                    safeToken(errorType)?.let {
                        append(",\"error_type\":\"").append(it).append('"')
                    }
                    append('}')
                }
            val line = (entry + "\n").toByteArray(Charsets.UTF_8)
            if (line.size > MAX_EVENT_BYTES) return
            FileOutputStream(activeFile(), true).use { output -> output.write(line) }
            enforceTotalSize()
        } catch (_: Exception) {
            // Logging is diagnostic only and must never break VPN behavior.
        }
    }

    @Synchronized
    fun diagnosticSnapshot(maxBytes: Int = MAX_DIAGNOSTIC_BYTES): String {
        if (maxBytes <= 0 || !directory.isDirectory) return ""
        val output = StringBuilder()
        var remaining = maxBytes
        logFiles()
            .sortedByDescending(File::lastModified)
            .forEach { file ->
                if (remaining <= 0) return@forEach
                val bytes =
                    try {
                        file.readBytes()
                    } catch (_: Exception) {
                        return@forEach
                    }
                val tail =
                    if (bytes.size <= remaining) {
                        bytes
                    } else {
                        bytes.copyOfRange(bytes.size - remaining, bytes.size)
                    }
                tail
                    .toString(Charsets.UTF_8)
                    .lineSequence()
                    .filter(::isSafeDiagnosticLine)
                    .forEach { line ->
                        val encoded = (line + "\n").toByteArray(Charsets.UTF_8)
                        if (encoded.size <= remaining) {
                            output.append(line).append('\n')
                            remaining -= encoded.size
                        }
                    }
                bytes.fill(0)
                if (tail !== bytes) tail.fill(0)
            }
        return output.toString()
    }

    @Synchronized
    fun clear() {
        logFiles().forEach(File::delete)
    }

    private fun activeFile(): File = File(directory, ACTIVE_FILE)

    private fun rotateIfNeeded() {
        val active = activeFile()
        if (!active.isFile || active.length() < ROTATE_BYTES) return
        val rotated = File(directory, "android-engine-${System.currentTimeMillis()}.jsonl")
        if (!active.renameTo(rotated)) {
            active.delete()
        }
    }

    private fun purgeExpired() {
        val cutoff = System.currentTimeMillis() - RETENTION_MILLIS
        logFiles().filter { it.lastModified() in 1 until cutoff }.forEach(File::delete)
    }

    private fun enforceTotalSize() {
        val files = logFiles().sortedByDescending(File::lastModified).toMutableList()
        var total = files.sumOf(File::length)
        files.asReversed().forEach { file ->
            if (total <= MAX_TOTAL_BYTES || file == activeFile()) return@forEach
            val length = file.length()
            if (file.delete()) total -= length
        }
    }

    private fun logFiles(): List<File> =
        directory
            .listFiles { file ->
                file.isFile &&
                    file.name.startsWith("android-engine") &&
                    file.name.endsWith(".jsonl")
            }?.toList() ?: emptyList()

    private fun isSafeDiagnosticLine(line: String): Boolean {
        if (line.isBlank() || line.toByteArray(Charsets.UTF_8).size > MAX_EVENT_BYTES) return false
        val match = LOG_LINE.matchEntire(line) ?: return false
        return match.groups["event"]?.value in EVENT_NAMES &&
            match.groups["phase"]?.value.let { it == null || it in ALLOWED_PHASES } &&
            match.groups["mode"]?.value.let { it == null || it in ALLOWED_MODES } &&
            match.groups["transport"]?.value.let { it == null || it in ALLOWED_TRANSPORTS } &&
            match.groups["error"]?.value.let { it == null || safeToken(it) == it }
    }

    private fun safeToken(value: String?): String? =
        value
            ?.takeIf { it.length in 1..64 }
            ?.takeIf { token -> token.all { it.isLetterOrDigit() || it == '_' || it == '-' } }

    private companion object {
        const val LOG_DIRECTORY = "logs"
        const val ACTIVE_FILE = "android-engine.jsonl"
        const val MAX_EVENT_BYTES = 2 * 1024
        const val ROTATE_BYTES = 4L * 1024L * 1024L
        const val MAX_TOTAL_BYTES = 20L * 1024L * 1024L
        const val MAX_DIAGNOSTIC_BYTES = 2 * 1024 * 1024
        const val RETENTION_MILLIS = 7L * 24L * 60L * 60L * 1_000L
        val ALLOWED_PHASES =
            setOf(
                "disconnected",
                "preparing",
                "connectingH3",
                "connectingH2",
                "connected",
                "degraded",
                "reconnecting",
                "disconnecting",
                "captivePortalPaused",
                "error",
            )
        val ALLOWED_MODES = setOf("vpn", "socks5", "httpProxy")
        val ALLOWED_TRANSPORTS = setOf("h3", "h2", "HTTP/3", "HTTP/2")
        val EVENT_NAMES = Event.entries.map(Event::name).toSet()
        val LOG_LINE =
            Regex(
                """^\{"timestamp":"[0-9T:.+\-Z]+","level":"(?:INFO|WARN)","event":"(?<event>[A-Z_]+)"(?:,"phase":"(?<phase>[A-Za-z0-9]+)")?(?:,"mode":"(?<mode>[A-Za-z0-9]+)")?(?:,"transport":"(?<transport>[A-Za-z0-9/]+)")?(?:,"error_type":"(?<error>[A-Za-z0-9_-]+)")?\}$""",
            )
    }
}
