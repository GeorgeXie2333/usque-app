package io.github.georgexie2333.usque

import android.content.ContentResolver
import android.net.Uri
import android.provider.OpenableColumns
import java.io.InputStream

/** Bounded, single-use document reads. Paths and URI permissions are never retained. */
internal object ChainConfigurationFiles {
    const val MAX_FILES = 128
    const val MAX_BYTES = 128 * 1024

    fun read(
        resolver: ContentResolver,
        uri: Uri,
    ): Map<String, Any> {
        val name =
            try {
                resolver
                    .query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                    ?.use { cursor ->
                        if (cursor.moveToFirst()) cursor.getString(0) else null
                    }.orEmpty()
                    .substringAfterLast('/')
                    .substringAfterLast('\\')
            } catch (_: Exception) {
                ""
            }
        return read(name) { resolver.openInputStream(uri) }
    }

    internal fun read(
        name: String,
        open: () -> InputStream?,
    ): Map<String, Any> {
        val buffer = ByteArray(MAX_BYTES + 1)
        try {
            val count =
                open()?.use { input ->
                    var total = 0
                    while (total < buffer.size) {
                        val read = input.read(buffer, total, buffer.size - total)
                        if (read < 0) break
                        require(read > 0)
                        total += read
                    }
                    total
                } ?: error("File unavailable")
            if (count > MAX_BYTES) return mapOf("name" to name, "error" to "CHAIN_FILE_TOO_LARGE")
            require(count > 0)
            return mapOf("name" to name, "bytes" to buffer.copyOf(count))
        } catch (_: Exception) {
            return mapOf("name" to name, "error" to "CHAIN_FILE_READ_FAILED")
        } finally {
            buffer.fill(0)
        }
    }

    fun clear(files: List<Map<String, Any>>) {
        files.forEach { (it["bytes"] as? ByteArray)?.fill(0) }
    }
}
