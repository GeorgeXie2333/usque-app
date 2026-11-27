package io.github.georgexie2333.usque

import android.content.Context
import android.os.Bundle
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import java.util.UUID

/**
 * Hands large strings between the UI and `:vpn` processes outside Binder.
 *
 * Binder fails a transaction near 1 MiB and a Parcel stores strings as
 * UTF-16. With up to 8192 routing rules, a profile is about 1.2 MB of JSON and
 * a settings result carries several profiles, so those strings travel through
 * an app-private file named by a random UUID. The message carries only the
 * name and the SHA-256 of the contents. The receiver deletes the file after
 * one read; a missing, oversized or altered file reads as an absent value,
 * which every caller already rejects as an invalid request.
 */
internal object LargeIpcPayload {
    /** Strings up to this many chars stay inline (at most 64 KiB as UTF-16). */
    const val INLINE_LIMIT_CHARS = 32 * 1024

    /** Largest accepted payload, above the 8 Mi-char settings result limit. */
    const val MAX_PAYLOAD_BYTES = 24 * 1024 * 1024

    /** One profile with the largest routing rule set; Rust applies the same bound. */
    const val MAX_PROFILE_BYTES = 2 * 1024 * 1024

    /** A settings result carries up to four profiles. */
    const val MAX_SETTINGS_CHARS = 8 * 1024 * 1024

    /**
     * A full endpoint subscription catalogue: 32 lists of 512 endpoints with
     * maximum-length hostnames, identifiers and notes (about 12.3 Mi chars).
     */
    const val MAX_SUBSCRIPTION_CHARS = 16 * 1024 * 1024

    const val STALE_MILLIS = 10 * 60 * 1000L
    private const val NAME_SUFFIX = "#handoff"
    private const val DIGEST_SUFFIX = "#sha256"
    private val namePattern =
        Regex("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\\.json$")

    data class Handoff(
        val name: String,
        val digest: String,
        val file: File,
    )

    fun directory(context: Context): File = File(context.noBackupFilesDir, "ipc-handoff")

    fun needsHandoff(value: String?): Boolean = value != null && value.length > INLINE_LIMIT_CHARS

    fun write(
        directory: File,
        value: String,
    ): Handoff {
        val bytes = value.toByteArray(Charsets.UTF_8)
        require(bytes.size <= MAX_PAYLOAD_BYTES) { "IPC payload exceeds the safety limit" }
        check(directory.isDirectory || directory.mkdirs()) { "IPC handoff directory unavailable" }
        val name = "${UUID.randomUUID()}.json"
        val file = File(directory, name)
        val pending = File(directory, "$name.new")
        try {
            // No fsync: the receiver reads through the page cache, and a crash
            // leaves only a stale file for the sweep. The rename keeps a
            // complete file under every name a sweep or receiver can see.
            FileOutputStream(pending).use { output -> output.write(bytes) }
            Files.move(pending.toPath(), file.toPath(), StandardCopyOption.ATOMIC_MOVE)
        } finally {
            pending.delete()
        }
        return Handoff(name, sha256(bytes), file)
    }

    /** Reads a handoff exactly once. The file is deleted whatever the outcome. */
    fun read(
        directory: File,
        name: String?,
        digest: String?,
    ): String? {
        if (name == null || !namePattern.matches(name)) return null
        val file = File(directory, name)
        return try {
            if (digest == null || !file.isFile || file.length() > MAX_PAYLOAD_BYTES) {
                null
            } else {
                val bytes = file.readBytes()
                if (MessageDigest.isEqual(sha256(bytes).toByteArray(), digest.toByteArray())) {
                    String(bytes, Charsets.UTF_8)
                } else {
                    null
                }
            }
        } catch (_: IOException) {
            null
        } finally {
            file.delete()
        }
    }

    /** Removes handoffs no receiver claimed, such as after a process died. */
    fun sweep(
        directory: File,
        now: Long = System.currentTimeMillis(),
    ) {
        directory.listFiles()?.forEach { file ->
            if (now - file.lastModified() > STALE_MILLIS) file.delete()
        }
    }

    fun discard(handoffs: Collection<Handoff>) {
        handoffs.forEach { it.file.delete() }
    }

    /**
     * Puts [value] inline or as a handoff. The caller must [discard] the
     * returned handoff if the message is never delivered.
     */
    fun putString(
        directory: File,
        bundle: Bundle,
        key: String,
        value: String?,
    ): Handoff? {
        if (value == null || !needsHandoff(value)) {
            bundle.putString(key, value)
            return null
        }
        val handoff = write(directory, value)
        bundle.putString(key + NAME_SUFFIX, handoff.name)
        bundle.putString(key + DIGEST_SUFFIX, handoff.digest)
        return handoff
    }

    fun getString(
        directory: File,
        bundle: Bundle?,
        key: String,
    ): String? {
        bundle ?: return null
        val name = bundle.getString(key + NAME_SUFFIX) ?: return bundle.getString(key)
        return read(directory, name, bundle.getString(key + DIGEST_SUFFIX))
    }

    /** Replaces every handoff in a received bundle with its verified string. */
    fun inline(
        directory: File,
        bundle: Bundle?,
    ) {
        bundle ?: return
        for (nameKey in bundle.keySet().filter { it.endsWith(NAME_SUFFIX) }) {
            val key = nameKey.removeSuffix(NAME_SUFFIX)
            val value = read(directory, bundle.getString(nameKey), bundle.getString(key + DIGEST_SUFFIX))
            bundle.remove(nameKey)
            bundle.remove(key + DIGEST_SUFFIX)
            bundle.putString(key, value)
        }
    }

    private fun sha256(bytes: ByteArray): String =
        MessageDigest
            .getInstance("SHA-256")
            .digest(bytes)
            .joinToString("") { "%02x".format(it) }
}
