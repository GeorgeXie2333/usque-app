package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.io.RandomAccessFile
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.concurrent.ConcurrentHashMap

/** Non-secret policy. Every read and patch shares a stable cross-process lock. */
internal class AtomicPolicyStore(
    private val file: File,
    private val migrate: () -> Map<String, Any?> = { emptyMap() },
) {
    private data class State(
        val revision: Long,
        val values: Map<String, Any?>,
    )

    private fun <T> locked(action: () -> T): T {
        val path = file.canonicalPath
        return synchronized(locks.computeIfAbsent(path) { Any() }) {
            val parent = requireNotNull(file.parentFile)
            check(parent.isDirectory || parent.mkdirs()) { "Policy directory unavailable" }
            RandomAccessFile(File(parent, "${file.name}.lock"), "rw").use { lockFile ->
                lockFile.channel.lock().use { action() }
            }
        }
    }

    private fun readLocked(): State {
        if (!file.exists()) {
            val initial = State(1, migrate())
            writeLocked(initial)
            return initial
        }
        check(file.length() <= 4 * 1024 * 1024) { "Policy exceeds size limit" }
        val json = JSONObject(file.readText(Charsets.UTF_8))
        check(json.getInt("schema") == 1) { "Unsupported policy schema" }
        val revision = json.getLong("revision")
        check(revision >= 0) { "Invalid policy revision" }
        val values = json.getJSONObject("values")
        return State(
            revision,
            values.keys().asSequence().associateWith { key ->
                when (val value = values.get(key)) {
                    is JSONArray -> {
                        (0 until value.length())
                            .map {
                                val item = value.get(it)
                                check(item is String) { "Policy set contains a non-string value" }
                                item
                            }.toSet()
                    }

                    JSONObject.NULL -> {
                        null
                    }

                    else -> {
                        value
                    }
                }
            },
        )
    }

    private fun writeLocked(state: State) {
        val values = JSONObject()
        state.values.forEach { (key, value) ->
            values.put(key, if (value is Set<*>) JSONArray(value.toList()) else value)
        }
        val bytes =
            JSONObject()
                .put("schema", 1)
                .put("revision", state.revision)
                .put("values", values)
                .toString()
                .toByteArray(Charsets.UTF_8)
        check(bytes.size <= 4 * 1024 * 1024) { "Policy exceeds size limit" }
        val pending = File(file.parentFile, "${file.name}.new")
        try {
            FileOutputStream(pending).use { output ->
                output.write(bytes)
                output.fd.sync()
            }
            // Android minSdk 26 and Windows host tests support java.nio. Never
            // fall back to a non-atomic overwrite if the filesystem rejects it.
            Files.move(
                pending.toPath(),
                file.toPath(),
                StandardCopyOption.ATOMIC_MOVE,
                StandardCopyOption.REPLACE_EXISTING,
            )
        } finally {
            if (pending.exists() && !pending.delete()) throw IOException("Policy temporary cleanup failed")
        }
    }

    fun snapshot(): Map<String, Any?> = locked { readLocked().values.toMap() }

    fun revision(): Long = locked { readLocked().revision }

    fun contains(key: String): Boolean = locked { readLocked().values.containsKey(key) }

    fun getString(
        key: String,
        fallback: String?,
    ): String? =
        locked {
            val value = readLocked().values[key]
            check(value == null || value is String) { "Policy value is not a string" }
            value ?: fallback
        }

    fun getBoolean(
        key: String,
        fallback: Boolean,
    ): Boolean =
        locked {
            val value = readLocked().values[key]
            check(value == null || value is Boolean) { "Policy value is not a boolean" }
            value ?: fallback
        }

    fun getStringSet(
        key: String,
        fallback: Set<String>?,
    ): Set<String>? =
        locked {
            val value = readLocked().values[key]
            check(value == null || value is Set<*>) { "Policy value is not a string set" }
            value
                ?.map {
                    require(it is String)
                    it
                }?.toSet() ?: fallback
        }

    fun edit(): Editor = Editor()

    fun edit(action: Editor.() -> Unit) {
        val editor = edit()
        editor.action()
        check(editor.commit()) { "Policy commit failed" }
    }

    inner class Editor {
        private val patch = linkedMapOf<String, Any?>()
        private var clear = false

        fun putString(
            key: String,
            value: String?,
        ): Editor = apply { patch[key] = value }

        fun putBoolean(
            key: String,
            value: Boolean,
        ): Editor = apply { patch[key] = value }

        fun putStringSet(
            key: String,
            value: Set<String>?,
        ): Editor = apply { patch[key] = value?.toSet() }

        fun remove(key: String): Editor = apply { patch[key] = null }

        fun clear(): Editor = apply { clear = true }

        fun commit(): Boolean =
            try {
                locked {
                    val current = readLocked()
                    val next = if (clear) mutableMapOf() else current.values.toMutableMap()
                    patch.forEach { (key, value) -> if (value == null) next.remove(key) else next[key] = value }
                    writeLocked(State(Math.addExact(current.revision, 1), next))
                }
                true
            } catch (_: IOException) {
                false
            }
    }

    companion object {
        private val locks = ConcurrentHashMap<String, Any>()
    }
}
